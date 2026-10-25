//! The hosted-model brain: the same functions, sent to a model that runs
//! somewhere else, with the user's own key.
//!
//! The local Needle brain is small (121M parameters) and exists to pick one
//! function out of a list. A hosted model can do that *and* answer questions, so
//! here the reply is allowed to be words as well as calls — which is what makes
//! "why is my fan so loud" a sentence rather than a web search.
//!
//! Each service has its own request shape, so the request is built and the
//! answer parsed per provider. Both halves are pure functions of the config so
//! they can be tested without a network; [`ask`] is the only part that dials out.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::config::{ApiProvider, Config, Shape, Thinking, ThinkingStyle};

/// One previous exchange, kept so a follow-up ("the other one") means something.
#[derive(Debug, Clone)]
pub struct Turn {
    pub ask: String,
    pub said: String,
}

/// A function the model asked for.
#[derive(Debug, Clone)]
pub struct Call {
    pub name: String,
    pub args: Value,
}

/// What the model came back with.
#[derive(Debug, Clone)]
pub struct Reply {
    /// What to say. Empty when the model only called functions.
    pub text: String,
    pub calls: Vec<Call>,
    /// The model's own thinking, where the service exposes it. Only logged.
    pub reasoning: Option<String>,
    pub millis: u128,
}

/// One HTTP request, built but not sent.
#[derive(Debug, Clone)]
pub struct Plan {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

impl ApiProvider {
    /// Where this service lives when the user has not overridden it.
    pub fn default_base(self) -> &'static str {
        match self {
            ApiProvider::OpenAi => "https://api.openai.com/v1",
            ApiProvider::Anthropic => "https://api.anthropic.com",
            ApiProvider::Gemini => "https://generativelanguage.googleapis.com",
            ApiProvider::OpenRouter => "https://openrouter.ai/api/v1",
            ApiProvider::DeepSeek => "https://api.deepseek.com/v1",
            ApiProvider::Groq => "https://api.groq.com/openai/v1",
            ApiProvider::Mistral => "https://api.mistral.ai/v1",
            ApiProvider::XAi => "https://api.x.ai/v1",
            // Anything that speaks the OpenAI shape: Ollama, LM Studio, vLLM,
            // a proxy. Ollama's default endpoint is the most useful starting
            // point because it needs no key and no account.
            ApiProvider::Custom => "http://localhost:11434/v1",
        }
    }

    /// The model to use when the user has not chosen one. Model names come and
    /// go, so the picker can fetch the real list; these are only the opening
    /// suggestion, and "Fetch the list" is the source of truth.
    pub fn default_model(self) -> &'static str {
        match self {
            ApiProvider::OpenAi => "gpt-4.1-mini",
            ApiProvider::Anthropic => "claude-sonnet-4-5",
            ApiProvider::Gemini => "gemini-2.5-flash",
            ApiProvider::OpenRouter => "openai/gpt-4.1-mini",
            ApiProvider::DeepSeek => "deepseek-chat",
            ApiProvider::Groq => "llama-3.3-70b-versatile",
            ApiProvider::Mistral => "mistral-small-latest",
            ApiProvider::XAi => "grok-3",
            ApiProvider::Custom => "",
        }
    }

    pub fn needs_key(self) -> bool {
        // A local model server does not want one.
        self != ApiProvider::Custom
    }
}

/// Does this model name belong to a family that thinks before it answers?
/// Those reject `temperature` and want `max_completion_tokens`.
fn is_reasoning_model(model: &str) -> bool {
    let m = model.to_lowercase();
    m.starts_with("o1") || m.starts_with("o3") || m.starts_with("o4") || m.contains("gpt-5")
}

/// How many tokens of thinking a level asks for, where the service counts them.
fn thinking_budget(level: Thinking) -> u32 {
    match level {
        Thinking::Off => 0,
        Thinking::Low => 1024,
        Thinking::Medium => 4096,
        Thinking::High => 8192,
    }
}

/// The effort word OpenAI and OpenRouter understand.
fn effort_word(level: Thinking) -> &'static str {
    match level {
        Thinking::Off => "none",
        Thinking::Low => "low",
        Thinking::Medium => "medium",
        Thinking::High => "high",
    }
}

/// What the assistant is, said to a model that has never met it.
///
/// This does more work than it looks like: it is what keeps a general model from
/// answering in markdown, from narrating the functions instead of calling them,
/// and from apologising for being an AI — none of which survives being read out
/// loud by a voice.
fn system_prompt(cfg: &Config) -> String {
    let name = cfg.agent_name.trim();
    let name = if name.is_empty() { "the assistant" } else { name };
    let style = match cfg.personality {
        crate::personality::Persona::Cheerful => "Warm and upbeat, never gushing.",
        crate::personality::Persona::Chill => "Laid back and brief.",
        crate::personality::Persona::Sarcastic => "Dry and a little sarcastic, but genuinely useful.",
        crate::personality::Persona::Butler => "Impeccably polite and precise.",
    };
    let mut prompt = format!(
        "You are {name}, a voice assistant that lives on this Windows PC. The user has just \
spoken to you, and everything you say is read out loud by a speech synthesiser.\n\
Answer in one or two short sentences. No markdown, no lists, no code, no emoji, no \
headings, and no URLs unless asked. Never mention functions, tools, APIs or models.\n\
When the request asks for something to be done on this computer, call the function that \
does it and do not describe what you would do instead. When it is a question, answer it \
briefly and directly. When it is too unclear to act on, ask one short question.\n\
Style: {style}"
    );
    let extra = cfg.api_system_prompt.trim();
    if !extra.is_empty() {
        prompt.push_str("\n\nAlso always: ");
        prompt.push_str(extra);
    }
    prompt
}

/// Our tool schema, in each service's own dialect. An empty catalogue stays an
/// empty array rather than a declaration with nothing in it, because a couple of
/// the services reject the latter outright.
fn tools_for(provider: ApiProvider, tools_json: &str) -> Value {
    let tools: Vec<Value> = serde_json::from_str(tools_json).unwrap_or_default();
    if tools.is_empty() {
        return Value::Array(Vec::new());
    }
    match provider.shape() {
        Shape::OpenAi => Value::Array(
            tools
                .into_iter()
                .map(|t| json!({"type": "function", "function": t}))
                .collect(),
        ),
        Shape::Anthropic => Value::Array(
            tools
                .into_iter()
                .map(|t| {
                    json!({
                        "name": t["name"],
                        "description": t["description"],
                        "input_schema": t["parameters"],
                    })
                })
                .collect(),
        ),
        // Gemini wants OpenAPI-flavoured schemas and rejects empty objects in
        // places other services ignore, so the shape is tidied here.
        Shape::Gemini => {
            let declarations: Vec<Value> = tools
                .into_iter()
                .map(|t| {
                    let mut parameters = t["parameters"].clone();
                    if let Some(obj) = parameters.as_object_mut() {
                        if obj.get("properties").and_then(|p| p.as_object()).is_some_and(|p| p.is_empty()) {
                            obj.remove("properties");
                        }
                        if obj.get("required").and_then(|r| r.as_array()).is_some_and(|r| r.is_empty()) {
                            obj.remove("required");
                        }
                    }
                    json!({"name": t["name"], "description": t["description"], "parameters": parameters})
                })
                .collect();
            json!([{"functionDeclarations": declarations}])
        }
    }
}

/// The request for one command. Split out from [`ask`] so the shapes can be
/// checked without a network, and so a provider that changes its mind is a
/// one-function fix.
pub fn plan(cfg: &Config, command: &str, tools_json: &str, history: &[Turn]) -> Result<Plan, String> {
    let key = crate::secret::reveal(&cfg.api_key);
    if cfg.api_provider.needs_key() && key.trim().is_empty() {
        return Err("there's no API key yet — add one in Settings, under Brain".into());
    }
    let base = {
        let configured = cfg.api_base_url.trim().trim_end_matches('/');
        if configured.is_empty() { cfg.api_provider.default_base().to_string() } else { configured.to_string() }
    };
    let model = {
        let chosen = cfg.api_model.trim();
        if chosen.is_empty() { cfg.api_provider.default_model().to_string() } else { chosen.to_string() }
    };
    if model.is_empty() {
        return Err("no model chosen for this service yet — pick one in Settings, under Brain".into());
    }
    let max_tokens = cfg.api_max_tokens.clamp(64, 8192);
    let system = system_prompt(cfg);
    let tools = tools_for(cfg.api_provider, tools_json);

    let provider = cfg.api_provider;
    match provider.shape() {
        // Every one of these speaks the shape OpenAI published: chat
        // completions, a bearer token, tools as function wrappers.
        Shape::OpenAi => {
            let mut messages: Vec<Value> = vec![json!({"role": "system", "content": system})];
            for turn in history {
                messages.push(json!({"role": "user", "content": turn.ask}));
                messages.push(json!({"role": "assistant", "content": turn.said}));
            }
            messages.push(json!({"role": "user", "content": command}));
            let mut body = json!({
                "model": model,
                "messages": messages,
            });
            // A question with no functions to offer should not carry an empty
            // declaration: the connection test uses exactly this shape.
            if tools.as_array().is_some_and(|t| !t.is_empty()) {
                body["tools"] = tools.clone();
                body["tool_choice"] = json!("auto");
            }
            // A reasoning family rejects both of these; every other model wants
            // the token cap under its old name.
            if is_reasoning_model(&model) || provider == ApiProvider::OpenRouter {
                body["max_completion_tokens"] = json!(max_tokens);
            } else {
                body["max_tokens"] = json!(max_tokens);
            }
            let thinking = cfg.api_thinking;
            match provider.thinking() {
                ThinkingStyle::Effort if thinking != Thinking::Off => {
                    body["reasoning_effort"] = json!(effort_word(thinking));
                }
                ThinkingStyle::ReasoningObject if thinking != Thinking::Off => {
                    body["reasoning"] = json!({"effort": effort_word(thinking)});
                }
                _ if thinking == Thinking::Off => {
                    body["temperature"] = json!(cfg.api_temperature.clamp(0.0, 2.0));
                }
                // ByModel: the service chooses by model name, so there is
                // nothing to send — and services that do not know the field
                // reject the whole request for it.
                _ => {}
            }
            let mut headers = vec![("authorization".to_string(), format!("Bearer {key}"))];
            if provider == ApiProvider::OpenRouter {
                headers.push(("x-title".to_string(), crate::PRODUCT.to_string()));
            }
            Ok(Plan { url: format!("{base}/chat/completions"), headers, body })
        }
        Shape::Anthropic => {
            let mut messages: Vec<Value> = Vec::new();
            for turn in history {
                messages.push(json!({"role": "user", "content": turn.ask}));
                messages.push(json!({"role": "assistant", "content": turn.said}));
            }
            messages.push(json!({"role": "user", "content": command}));
            // Thinking tokens come out of max_tokens, and Claude refuses a
            // budget it cannot fit, so the cap grows with the budget.
            let budget = thinking_budget(cfg.api_thinking);
            let max_tokens = if budget > 0 { max_tokens.max(budget + 512) } else { max_tokens };
            let mut body = json!({
                "model": model,
                "max_tokens": max_tokens,
                "system": system,
                "messages": messages,
            });
            if tools.as_array().is_some_and(|t| !t.is_empty()) {
                body["tools"] = tools.clone();
            }
            if budget > 0 {
                body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
            } else {
                body["temperature"] = json!(cfg.api_temperature.clamp(0.0, 1.0));
            }
            Ok(Plan {
                url: format!("{base}/v1/messages"),
                headers: vec![
                    ("x-api-key".to_string(), key),
                    ("anthropic-version".to_string(), "2023-06-01".to_string()),
                ],
                body,
            })
        }
        Shape::Gemini => {
            let mut contents: Vec<Value> = Vec::new();
            for turn in history {
                contents.push(json!({"role": "user", "parts": [{"text": turn.ask}]}));
                contents.push(json!({"role": "model", "parts": [{"text": turn.said}]}));
            }
            contents.push(json!({"role": "user", "parts": [{"text": command}]}));
            let mut generation = json!({"maxOutputTokens": max_tokens});
            let budget = thinking_budget(cfg.api_thinking);
            if budget > 0 {
                generation["thinkingConfig"] = json!({"thinkingBudget": budget});
            } else {
                generation["temperature"] = json!(cfg.api_temperature.clamp(0.0, 2.0));
            }
            let mut body = json!({
                "systemInstruction": {"parts": [{"text": system}]},
                "contents": contents,
                "generationConfig": generation,
            });
            if tools.as_array().is_some_and(|t| !t.is_empty()) {
                body["tools"] = tools.clone();
            }
            Ok(Plan {
                url: format!("{base}/v1beta/models/{model}:generateContent"),
                headers: vec![("x-goog-api-key".to_string(), key)],
                body,
            })
        }
    }
}

/// Pull the service's own complaint out of an error body.
fn api_message(body: &Value) -> String {
    body["error"]["message"]
        .as_str()
        .or_else(|| body["error"].as_str())
        .or_else(|| body["message"].as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Read the answer in the shape the service returns it.
pub fn parse(provider: ApiProvider, body: &Value) -> Result<Reply, String> {
    let mut text = String::new();
    let mut calls = Vec::new();
    let mut reasoning: Option<String> = None;
    match provider.shape() {
        Shape::OpenAi => {
            let message = &body["choices"][0]["message"];
            if let Some(content) = message["content"].as_str() {
                text.push_str(content);
            } else if let Some(parts) = message["content"].as_array() {
                // Some services return the text as content blocks.
                for part in parts {
                    if let Some(t) = part["text"].as_str() {
                        text.push_str(t);
                    }
                }
            }
            if let Some(list) = message["tool_calls"].as_array() {
                for call in list {
                    let name = call["function"]["name"].as_str().unwrap_or_default().to_string();
                    if name.is_empty() {
                        continue;
                    }
                    calls.push(Call { name, args: parse_args(&call["function"]["arguments"]) });
                }
            }
            if let Some(r) = message["reasoning"].as_str().or_else(|| message["reasoning_content"].as_str()) {
                reasoning = Some(r.to_string());
            }
        }
        Shape::Anthropic => {
            let blocks = body["content"].as_array().cloned().unwrap_or_default();
            let mut thinking = String::new();
            for block in blocks {
                match block["type"].as_str().unwrap_or_default() {
                    "text" => text.push_str(block["text"].as_str().unwrap_or_default()),
                    "thinking" => thinking.push_str(block["thinking"].as_str().unwrap_or_default()),
                    "tool_use" => {
                        let name = block["name"].as_str().unwrap_or_default().to_string();
                        if !name.is_empty() {
                            calls.push(Call { name, args: block["input"].clone() });
                        }
                    }
                    _ => {}
                }
            }
            if !thinking.trim().is_empty() {
                reasoning = Some(thinking);
            }
        }
        Shape::Gemini => {
            let parts = body["candidates"][0]["content"]["parts"].as_array().cloned().unwrap_or_default();
            let mut thinking = String::new();
            for part in parts {
                if let Some(t) = part["text"].as_str() {
                    // Gemini marks thought summaries explicitly; they are not
                    // for reading out.
                    if part["thought"].as_bool().unwrap_or(false) {
                        thinking.push_str(t);
                    } else {
                        text.push_str(t);
                    }
                }
                if let Some(call) = part.get("functionCall") {
                    let name = call["name"].as_str().unwrap_or_default().to_string();
                    if !name.is_empty() {
                        calls.push(Call { name, args: call["args"].clone() });
                    }
                }
            }
            if !thinking.trim().is_empty() {
                reasoning = Some(thinking);
            }
        }
    }

    // A finish of "length" or a safety block leaves no text and no calls, and
    // silently saying nothing is the worst possible outcome.
    if text.trim().is_empty() && calls.is_empty() {
        let block = &body["candidates"][0]["finishReason"];
        let reason = body["choices"][0]["finish_reason"].as_str().map(String::from);
        return Err(match reason.as_deref().or_else(|| block.as_str()) {
            Some("length") | Some("MAX_TOKENS") => "the answer was cut off before it said anything".to_string(),
            Some("content_filter") | Some("SAFETY") => "the service declined to answer that".to_string(),
            Some(other) => format!("the model returned nothing (it stopped for {other})"),
            None => format!("the model returned nothing: {}", api_message(body)),
        });
    }
    Ok(Reply { text: clean_spoken(&text), calls, reasoning, millis: 0 })
}

/// Tool arguments arrive as a JSON string on most services and as an object on
/// others, and sometimes as a string that is not JSON at all.
fn parse_args(value: &Value) -> Value {
    match value {
        Value::Object(_) => value.clone(),
        Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or_else(|_| json!({})),
        Value::Null => json!({}),
        other => other.clone(),
    }
}

/// Make the model's prose fit to be read out loud.
///
/// A general model writes for the eye: asterisks for emphasis, backticks around
/// identifiers, bullets, headings. A speech synthesiser reads every one of those
/// characters as a pause, a word, or gibberish.
pub fn clean_spoken(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_code = false;
    for ch in text.chars() {
        match ch {
            '`' => in_code = !in_code,
            '*' | '#' | '_' | '~' => {}
            '|' => out.push(','),
            c => out.push(c),
        }
    }
    let _ = in_code;
    let mut cleaned: String = out
        .lines()
        .map(|l| l.trim().trim_start_matches(['-', '•', '*', '>']).trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    // A numbered list reads as "1. 2." out loud; the numbers carry no meaning
    // once it is one sentence.
    while cleaned.starts_with(char::is_numeric) && cleaned.chars().nth(1) == Some('.') {
        cleaned = cleaned[2..].trim_start().to_string();
    }
    while cleaned.contains("  ") {
        cleaned = cleaned.replace("  ", " ");
    }
    cleaned.trim().to_string()
}

/// The error to show a person, from what the service actually said.
fn describe_failure(status: u16, body: &Value, model: &str) -> String {
    let detail = api_message(body);
    let detail = if detail.chars().count() > 200 {
        format!("{}…", detail.chars().take(197).collect::<String>())
    } else {
        detail
    };
    match status {
        401 | 403 => "the API key was rejected — check it in Settings, under Brain".to_string(),
        404 => format!("this key cannot use the model \"{model}\" — pick another in Settings"),
        429 => "the service is rate-limiting me — try again in a moment".to_string(),
        500..=599 => format!("the service is having trouble (HTTP {status})"),
        _ if !detail.is_empty() => format!("the service refused that (HTTP {status}): {detail}"),
        _ => format!("the service refused that (HTTP {status})"),
    }
}

fn agent(cfg: &Config) -> ureq::Agent {
    let timeout = Duration::from_secs(cfg.api_timeout_secs.clamp(5, 120) as u64);
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(timeout))
        // Read the body of a failed request instead of throwing it away: it is
        // where the service explains itself.
        .http_status_as_error(false)
        .build()
        .into()
}

/// Ask the model. Blocks for up to `api_timeout_secs`, which is why the agent
/// runs it on its own thread.
pub fn ask(cfg: &Config, command: &str, tools_json: &str, history: &[Turn]) -> Result<Reply, String> {
    let plan = plan(cfg, command, tools_json, history)?;
    let started = Instant::now();
    let mut request = agent(cfg).post(&plan.url).header("content-type", "application/json");
    for (key, value) in &plan.headers {
        request = request.header(key.as_str(), value.as_str());
    }
    let payload = serde_json::to_vec(&plan.body).map_err(|e| format!("the request could not be built: {e}"))?;
    let mut response = request.send(payload.as_slice()).map_err(|e| match e {
        ureq::Error::Timeout(_) => "the service took too long to answer".to_string(),
        ureq::Error::HostNotFound => "I couldn't find that service — check the address in Settings".to_string(),
        ureq::Error::ConnectionFailed => "I couldn't reach the service — check your connection".to_string(),
        other => format!("I couldn't reach the service: {other}"),
    })?;
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().map_err(|e| format!("the answer could not be read: {e}"))?;
    let body: Value = serde_json::from_str(&text)
        .unwrap_or_else(|_| json!({"message": text.chars().take(200).collect::<String>()}));
    if !(200..300).contains(&status) {
        let model = cfg.api_model.trim();
        return Err(describe_failure(status, &body, if model.is_empty() { "<default>" } else { model }));
    }
    let mut reply = parse(cfg.api_provider, &body)?;
    reply.millis = started.elapsed().as_millis();
    Ok(reply)
}

/// The models this key can use, for the picker in Settings.
pub fn list_models(cfg: &Config) -> Result<Vec<String>, String> {
    let key = crate::secret::reveal(&cfg.api_key);
    let base = {
        let configured = cfg.api_base_url.trim().trim_end_matches('/');
        if configured.is_empty() { cfg.api_provider.default_base().to_string() } else { configured.to_string() }
    };
    let bearer = vec![("authorization".to_string(), format!("Bearer {key}"))];
    let (url, headers) = match cfg.api_provider.shape() {
        // OpenRouter publishes its catalogue without a key; the rest want one.
        Shape::OpenAi if cfg.api_provider == ApiProvider::OpenRouter => (format!("{base}/models"), Vec::new()),
        Shape::OpenAi => (format!("{base}/models"), bearer),
        Shape::Anthropic => (
            format!("{base}/v1/models"),
            vec![("x-api-key".to_string(), key), ("anthropic-version".to_string(), "2023-06-01".to_string())],
        ),
        Shape::Gemini => (format!("{base}/v1beta/models"), vec![("x-goog-api-key".to_string(), key)]),
    };
    let mut request = agent(cfg).get(&url);
    for (k, v) in &headers {
        request = request.header(k.as_str(), v.as_str());
    }
    let mut response = request.call().map_err(|e| format!("couldn't ask for the model list: {e}"))?;
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let body: Value = serde_json::from_str(&text).map_err(|_| format!("the model list was not JSON (HTTP {status})"))?;
    if !(200..300).contains(&status) {
        return Err(describe_failure(status, &body, ""));
    }
    let mut names: Vec<String> = Vec::new();
    for entry in body["data"].as_array().cloned().unwrap_or_default() {
        if let Some(id) = entry["id"].as_str() {
            names.push(id.to_string());
        }
    }
    if names.is_empty() {
        // Gemini answers with `models: [{name: "models/gemini-2.5-flash"}]`.
        for entry in body["models"].as_array().cloned().unwrap_or_default() {
            if let Some(id) = entry["name"].as_str() {
                names.push(id.trim_start_matches("models/").to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    names.truncate(400);
    if names.is_empty() {
        return Err("the service listed no models for this key".into());
    }
    Ok(names)
}

/// One tiny round trip, to prove a key works. Used by the settings app.
pub fn probe(cfg: &Config) -> Result<String, String> {
    let reply = ask(cfg, "Reply with the single word: ready", "[]", &[])?;
    let said = if reply.text.is_empty() { "(no words, only a function call)".to_string() } else { reply.text };
    Ok(format!("{} answered: {said}", cfg.api_provider.label()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiProvider, BrainKind};

    fn api_cfg(provider: ApiProvider) -> Config {
        let mut cfg = Config::default();
        cfg.brain_kind = BrainKind::Api;
        cfg.api_provider = provider;
        cfg.api_key = "sk-test-key".into();
        // A local server's models are whatever it happens to have, so there is
        // no default to fall back on and the tests name one.
        cfg.api_model = if provider == ApiProvider::Custom { "local-model".into() } else { String::new() };
        cfg
    }

    #[test]
    fn every_service_gets_a_request_it_understands() {
        let tools = crate::tools::tools_json(&Config::default());
        for provider in ApiProvider::ALL {
            let cfg = api_cfg(provider);
            let plan = plan(&cfg, "open chrome", &tools, &[]).unwrap_or_else(|e| panic!("{provider:?}: {e}"));
            assert!(!plan.url.is_empty(), "{provider:?} has no URL");
            assert!(plan.body.is_object(), "{provider:?}");
            // Every request has to carry the functions and the command.
            let text = plan.body.to_string();
            assert!(text.contains("open chrome"), "{provider:?} lost the command");
            assert!(text.contains("focus_window"), "{provider:?} lost the tool list");
        }
    }

    /// DeepSeek publishes the OpenAI shape, so it is that shape plus its own
    /// address and model — and nothing else.
    #[test]
    fn deepseek_is_an_openai_service_with_its_own_address() {
        let cfg = api_cfg(ApiProvider::DeepSeek);
        let plan = plan(&cfg, "open chrome", &crate::tools::tools_json(&Config::default()), &[]).unwrap();
        assert_eq!(plan.url, "https://api.deepseek.com/v1/chat/completions");
        assert_eq!(plan.headers, vec![("authorization".to_string(), "Bearer sk-test-key".to_string())]);
        assert_eq!(plan.body["model"], "deepseek-chat");
        assert_eq!(plan.body["tools"][0]["type"], "function");
        let names: Vec<&str> = plan.body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert!(names.contains(&"focus_window"), "{names:?}");
        assert!(names.len() > 50, "the whole catalogue should travel to a hosted model");
        // The old name for the cap: DeepSeek is not a reasoning-family model.
        assert!(plan.body.get("max_tokens").is_some(), "{}", plan.body);
        assert!(plan.body.get("max_completion_tokens").is_none());
    }

    /// DeepSeek picks thinking by model name (deepseek-chat vs
    /// deepseek-reasoner), so an effort field would be noise at best and a 400
    /// at worst. Saying "high" must not break the request.
    #[test]
    fn thinking_is_only_sent_to_services_that_take_an_effort() {
        for provider in [ApiProvider::DeepSeek, ApiProvider::Groq, ApiProvider::Mistral, ApiProvider::XAi] {
            let mut cfg = api_cfg(provider);
            cfg.api_thinking = Thinking::High;
            let plan = plan(&cfg, "hello", "[]", &[]).unwrap();
            assert!(plan.body.get("reasoning_effort").is_none(), "{provider:?} sent an effort");
            assert!(plan.body.get("reasoning").is_none(), "{provider:?} sent a reasoning object");
            assert!(plan.body.get("thinking").is_none(), "{provider:?} sent a thinking block");
        }
        // OpenAI does take one, and then leaves the temperature alone.
        let mut cfg = api_cfg(ApiProvider::OpenAi);
        cfg.api_thinking = Thinking::Medium;
        let plan = plan(&cfg, "hello", "[]", &[]).unwrap();
        assert_eq!(plan.body["reasoning_effort"], "medium");
        assert!(plan.body.get("temperature").is_none());
    }

    /// DeepSeek's reasoner returns its thinking in `reasoning_content`; a
    /// service that explains itself should be shown in the log, not dropped.
    #[test]
    fn deepseek_reasoning_is_understood() {
        let body = json!({"choices": [{"message": {
            "content": "Opening Chrome.",
            "reasoning_content": "The user wants chrome opened.",
            "tool_calls": [{"function": {"name": "open_app", "arguments": "{\"app\":\"chrome\"}"}}]
        }}]});
        let reply = parse(ApiProvider::DeepSeek, &body).unwrap();
        assert_eq!(reply.text, "Opening Chrome.");
        assert_eq!(reply.reasoning.as_deref(), Some("The user wants chrome opened."));
        assert_eq!(reply.calls[0].name, "open_app");
    }

    /// Every service in the picker has an address, a starting model and a key
    /// page — a half-added provider is a dead option in the UI.
    #[test]
    fn every_offered_service_is_complete() {
        let mut bases = Vec::new();
        for provider in ApiProvider::ALL {
            let base = provider.default_base();
            assert!(base.starts_with("http"), "{provider:?}");
            assert!(!bases.contains(&base), "{provider:?} shares an address with another service");
            bases.push(base);
            if provider != ApiProvider::Custom {
                assert!(!provider.default_model().is_empty(), "{provider:?} has no model");
                assert!(provider.keys_page().starts_with("https://"), "{provider:?} has no key page");
                assert!(provider.needs_key());
            }
            let cfg = api_cfg(provider);
            let plan = plan(&cfg, "hello", "[]", &[]).unwrap_or_else(|e| panic!("{provider:?}: {e}"));
            assert!(plan.url.contains(base.trim_end_matches('/')), "{provider:?}: {}", plan.url);
        }
    }

    /// The three shapes really are different; a single "tools" array for all of
    /// them is the mistake this test exists to catch.
    #[test]
    fn tool_schemas_are_in_each_services_dialect() {
        let plain = crate::tools::tool_json("do_thing", "does a thing", &[("what".into(), String::new())]).to_string();
        let openai = tools_for(ApiProvider::OpenAi, &format!("[{plain}]"));
        assert_eq!(openai[0]["type"], "function");
        assert_eq!(openai[0]["function"]["name"], "do_thing");

        let anthropic = tools_for(ApiProvider::Anthropic, &format!("[{plain}]"));
        assert_eq!(anthropic[0]["name"], "do_thing");
        assert!(anthropic[0].get("input_schema").is_some());

        let gemini = tools_for(ApiProvider::Gemini, &format!("[{plain}]"));
        assert_eq!(gemini[0]["functionDeclarations"][0]["name"], "do_thing");
        assert_eq!(gemini[0]["functionDeclarations"][0]["parameters"]["type"], "object");
    }

    #[test]
    fn replies_are_read_in_each_services_shape() {
        let openai = json!({"choices": [{"message": {
            "content": "On it.",
            "tool_calls": [{"function": {"name": "open_app", "arguments": "{\"app\":\"chrome\"}"}}]
        }}]});
        let r = parse(ApiProvider::OpenAi, &openai).unwrap();
        assert_eq!(r.text, "On it.");
        assert_eq!(r.calls[0].name, "open_app");
        assert_eq!(r.calls[0].args["app"], "chrome");

        let anthropic = json!({"content": [
            {"type": "thinking", "thinking": "they want chrome"},
            {"type": "text", "text": "Opening Chrome."},
            {"type": "tool_use", "name": "open_app", "input": {"app": "chrome"}}
        ]});
        let r = parse(ApiProvider::Anthropic, &anthropic).unwrap();
        assert_eq!(r.text, "Opening Chrome.");
        assert_eq!(r.calls[0].name, "open_app");
        assert_eq!(r.reasoning.as_deref(), Some("they want chrome"));

        let gemini = json!({"candidates": [{"content": {"parts": [
            {"text": "Sure."},
            {"functionCall": {"name": "open_app", "args": {"app": "chrome"}}}
        ]}}]});
        let r = parse(ApiProvider::Gemini, &gemini).unwrap();
        assert_eq!(r.text, "Sure.");
        assert_eq!(r.calls[0].name, "open_app");

        // A reply with neither words nor a call is an error, not silence.
        let empty = json!({"choices": [{"message": {"content": ""}, "finish_reason": "length"}]});
        assert!(parse(ApiProvider::OpenAi, &empty).is_err());
    }

    /// Thinking is asked for in three different currencies, and turning it off
    /// has to leave the field out entirely rather than send a zero.
    #[test]
    fn thinking_levels_reach_each_service_correctly() {
        let tools = "[]";
        let mut cfg = api_cfg(ApiProvider::OpenAi);
        cfg.api_model = "gpt-4.1-mini".into();
        cfg.api_thinking = Thinking::High;
        let plan_hi = plan(&cfg, "hello", tools, &[]).unwrap();
        assert_eq!(plan_hi.body["reasoning_effort"], "high");
        assert!(plan_hi.body.get("temperature").is_none(), "a thinking model cannot take a temperature");
        cfg.api_thinking = Thinking::Off;
        let plan_off = plan(&cfg, "hello", tools, &[]).unwrap();
        assert!(plan_off.body.get("reasoning_effort").is_none());
        assert!(plan_off.body.get("temperature").is_some());

        let mut cfg = api_cfg(ApiProvider::Anthropic);
        cfg.api_thinking = Thinking::Medium;
        cfg.api_max_tokens = 300;
        let plan_think = plan(&cfg, "hello", tools, &[]).unwrap();
        assert_eq!(plan_think.body["thinking"]["budget_tokens"], 4096);
        assert!(plan_think.body["max_tokens"].as_u64().unwrap() > 4096, "the budget has to fit");

        let mut cfg = api_cfg(ApiProvider::Gemini);
        cfg.api_thinking = Thinking::Low;
        let plan_gem = plan(&cfg, "hello", tools, &[]).unwrap();
        assert_eq!(plan_gem.body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 1024);
        cfg.api_thinking = Thinking::Off;
        let plan_gem = plan(&cfg, "hello", tools, &[]).unwrap();
        assert!(plan_gem.body["generationConfig"].get("thinkingConfig").is_none());
    }

    /// A reasoning model wants its token cap under the new name; everything else
    /// still rejects it there.
    #[test]
    fn token_caps_use_the_right_field_name() {
        let mut cfg = api_cfg(ApiProvider::OpenAi);
        cfg.api_model = "o4-mini".into();
        let modern = plan(&cfg, "hi", "[]", &[]).unwrap();
        assert!(modern.body.get("max_completion_tokens").is_some(), "{}", modern.body);
        assert!(modern.body.get("max_tokens").is_none());
        cfg.api_model = "gpt-4.1-mini".into();
        let classic = plan(&cfg, "hi", "[]", &[]).unwrap();
        assert!(classic.body.get("max_tokens").is_some());
        assert!(classic.body.get("max_completion_tokens").is_none());
    }

    /// The key travels in whatever header the service wants, and a key-less
    /// local server is allowed to have none.
    #[test]
    fn credentials_go_where_each_service_looks_for_them() {
        let openai = plan(&api_cfg(ApiProvider::OpenAi), "hi", "[]", &[]).unwrap();
        assert_eq!(openai.headers[0], ("authorization".to_string(), "Bearer sk-test-key".to_string()));
        let anthropic = plan(&api_cfg(ApiProvider::Anthropic), "hi", "[]", &[]).unwrap();
        assert!(anthropic.headers.iter().any(|(k, _)| k == "anthropic-version"));
        assert!(anthropic.headers.iter().any(|(k, v)| k == "x-api-key" && v == "sk-test-key"));
        let gemini = plan(&api_cfg(ApiProvider::Gemini), "hi", "[]", &[]).unwrap();
        assert!(gemini.headers.iter().any(|(k, _)| k == "x-goog-api-key"));

        let mut local = api_cfg(ApiProvider::Custom);
        local.api_key = String::new();
        let plan_local = plan(&local, "hi", "[]", &[]).unwrap();
        assert_eq!(plan_local.url, "http://localhost:11434/v1/chat/completions");
        // ...but a missing key on a service that needs one is a clear refusal.
        let mut missing = api_cfg(ApiProvider::OpenAi);
        missing.api_key = String::new();
        let error = plan(&missing, "hi", "[]", &[]).unwrap_err();
        assert!(error.contains("API key"), "{error}");
    }

    /// The bytes that go to a speech synthesiser must not contain markdown.
    #[test]
    fn answers_are_cleaned_for_speech() {
        assert_eq!(clean_spoken("**Done!**"), "Done!");
        assert_eq!(clean_spoken("- first\n- second"), "first second");
        assert_eq!(clean_spoken("Use `git status` now"), "Use git status now");
        assert_eq!(clean_spoken("1. Open the door"), "Open the door");
        assert_eq!(clean_spoken("  lots   of\n\nspace  "), "lots of space");
        assert_eq!(clean_spoken("a | b"), "a , b");
        assert_eq!(clean_spoken(""), "");
    }

    /// The prompt is what keeps a chatty model sounding like an assistant.
    #[test]
    fn the_system_prompt_describes_a_voice_assistant() {
        let cfg = Config::default();
        let prompt = system_prompt(&cfg);
        assert!(prompt.contains("read out loud"), "{prompt}");
        assert!(prompt.contains(&cfg.agent_name), "{prompt}");
        assert!(prompt.contains("short sentences"), "{prompt}");
        let mut cfg = Config::default();
        cfg.api_system_prompt = "Always call me Captain.".into();
        assert!(system_prompt(&cfg).contains("Captain"));
    }
}
