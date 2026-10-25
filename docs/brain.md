# The brain: whose model answers

A command like *"how much battery is left"* is a sentence with a function to
find in it. What finds the function is the **brain**. Wisp has two, and switching
between them changes nothing else about how it behaves — the wake word, the
voice, the functions, the alarms, the bubble and the privacy of everything except
the command itself all stay exactly the same.

**Settings → Brain** is where you choose.

## On this PC (the default)

[Needle 3](https://github.com/cactus-compute/needle), a 121M-parameter
tool-calling model, 35 MB, running in this process on your CPU. It is loaded when
it is first needed, about 100 ms, and freed again after a period of quiet.

* **Free, offline, private.** No key, no account, no network. Nothing about your
  command leaves the machine.
* **It is a router, not a conversationalist.** It reads the command and the list
  of functions and answers with the one to call. It cannot answer a general
  question — *"why is my fan so loud"* has no function to call, so it becomes a
  web search.
* **Most commands never reach it.** Anything with a phrase in the catalogue is
  understood instantly by pattern matching, which is why *"open chrome"* is
  immediate and *"could you fire up the browser for me"* is not.
* **Depth** is the knob worth knowing: 20 is the whole model, 12 is about a third
  faster and slightly worse at unusual phrasing. Commands that skip the model are
  not affected by it.

## A hosted model (your API key)

Point it at a model you already pay for and it becomes something closer to an
assistant: it answers questions in a sentence, holds a little context between
commands, and still calls the functions when the request is one of those.

| Service | Key from | Notes |
|---|---|---|
| **OpenAI** | platform.openai.com/api-keys | Models that think (`o4-mini`, GPT-5 family) get their reasoning effort set and their token cap under the name they expect |
| **Anthropic** | console.anthropic.com/settings/keys | Thinking is a real token budget; the answer cap grows to fit it |
| **Google Gemini** | aistudio.google.com/apikey | Thinking is a `thinkingBudget`; thought summaries are logged, not spoken |
| **OpenRouter** | openrouter.ai/keys | One key for many vendors; thinking is normalised to an effort level |
| **DeepSeek** | platform.deepseek.com/api_keys | `deepseek-chat` for everyday commands, `deepseek-reasoner` when a request is worth thinking about. Its thinking arrives as `reasoning_content` and is logged, not spoken |
| **Groq** | console.groq.com/keys | Very fast open models. `llama-3.3-70b-versatile` is a good function-caller |
| **Mistral** | console.mistral.ai/api-keys | `mistral-small-latest` and friends |
| **xAI** | console.x.ai | Grok models |
| **Custom / local** | — | Anything that speaks the OpenAI API: Ollama, LM Studio, vLLM, llama.cpp's server, Azure OpenAI, a gateway. The address is yours to set |

DeepSeek, Groq, Mistral and xAI speak the same JSON as OpenAI — one row in the
provider table in `crates/wisp-core/src/config.rs`, and the request, the tool
schema, the reply and the model list all follow. Adding another is that row plus
a test case.

### Setting it up

1. **Settings → Brain → A hosted model.** Pick the service.
2. **Paste the key** and press **Save key**. The field is a password field; the
   card shows only the last four characters afterwards, and *Forget it* removes
   the key entirely. A key typed into `config.toml` by hand is encrypted on the
   next save too.
3. **Choose the model.** Leave the box empty to use the service's default, type a
   name, or press **Fetch the list** to ask the service what this key can use.
4. **Pick a thinking level** (below).
5. **Test the connection.** One short question; it costs a fraction of a cent and
   it tells you immediately whether the key and the model agree.

### Thinking levels

"How hard should it think before answering" maps to each service's own control:

| Level | OpenAI | OpenRouter | Anthropic | Gemini | Feels like |
|---|---|---|---|---|
| **Off** | no reasoning field; temperature is used instead | nothing | no thinking block | no thinking config | Fastest, cheapest. Right for *"open Chrome"* |
| **Low** | `reasoning_effort: low` | `reasoning.effort: low` | 1,024-token budget | `thinkingBudget: 1024` | A good default for a voice command |
| **Medium** | `medium` | `medium` | 4,096 | 2,048 | Noticeably slower, better on fiddly requests |
| **High** | `high` | `high` | 8,192 | 8,192 | Several seconds. For problems worth it |

**DeepSeek, Groq, Mistral, xAI and custom servers choose thinking by model**, not
by a parameter — `deepseek-chat` against `deepseek-reasoner`, or a Groq R1 build.
Nothing is sent to them, because a service that does not know a field rejects the
whole request for it. Pick the thinking model in the model box instead; the level
is ignored there, and the UI says so.

Thinking tokens cost money and time, and a spoken answer is one or two sentences
either way — which is why **Low** is the default rather than the maximum. A
reasoning model (OpenAI's `o`-series, GPT-5 family) rejects `temperature`, so Wisp
leaves it out whenever a thinking level is set, and uses `max_completion_tokens`
rather than `max_tokens` for those models.

### What it does with the answer

The model may answer in words, call a function, or both. Words are spoken —
cleaned first, because a chat model writes for the eye: asterisks, backticks,
bullets and numbered lists all become punctuation a speech synthesiser would read
out. If a function it called fails, the failure is what gets said, not whatever
the model promised beforehand.

The last three exchanges are kept as context, so *"open the other one"* means
something. Saying *"never mind"* clears it.

### When it cannot answer

A dropped connection, an expired key, a rate limit, a service outage — none of
those leave the assistant mute. Wisp logs the reason and asks **Needle 3** on this
PC instead; the command still works, one route further down. If the local model
is missing as well, the reason itself is spoken: *"the API key was rejected —
check it in Settings, under Brain"*.

Requests time out after the amount set in **Advanced** (25 seconds by default)
and the whole exchange happens on the agent's own thread, so the bubble stays up
and nothing else stalls.

## Privacy, precisely

| | On this PC | Hosted model |
|---|---|---|
| Audio | Never leaves the machine | Never leaves the machine |
| Transcript of your command | Never leaves the machine | Sent to the service you chose |
| Files, clipboard, screenshots, window list | Only read when a function needs them | Same — the model never sees them; it only asks for a function by name |
| API key | — | Encrypted with DPAPI for your Windows account, never logged |
| History kept | None | The last three exchanges, in memory only |

Function results are never sent back to the model. It picks the function; the
function runs locally and the answer is spoken. That is a deliberate limit: it
keeps what the model can learn about your machine to the command you spoke.

## Cost, roughly

A command is a short prompt plus the function list plus a one-or-two-sentence
answer: a few thousand tokens. On a small hosted model that is hundredths of a
cent per command; on a large one, tenths. The thinking level multiplies the
answer side, not the prompt side. Nothing is spent when the model is not used,
which is most of the time — the phrase list and the local model still handle the
everyday requests, whichever brain is selected.
