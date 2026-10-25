# Credits and third-party licences

Wisp's own code is MIT — see [LICENSE](LICENSE).

It is a thin layer of glue around other people's work, though, and everything
below is used as published, with the licence it came with. This file is also the
NOTICE that the Apache-2.0 components ask for: their licences and authors are
recorded here, and the models themselves are downloaded from their publishers
rather than redistributed in this repository.

The name is deliberately not the name of any model. Wisp ships with **Needle 3**
as its on-device brain and can use a hosted model with your own key, and the
product outlives both: the interesting parts belong to the projects below.

## The default brain: Needle 3

The tool calling — turning "put the kettle on and tell me the time" into two
ordered function calls — is done by **Needle 3**, a foundation tool-calling
model for tiny devices by **Cactus Compute, Inc.**

- Model: <https://huggingface.co/Cactus-Compute/needle3>
- Source: <https://github.com/cactus-compute/needle>
- Project page: <https://cactuscompute.com/needle>
- Licence: **Apache-2.0**
- Weights file: `models/needle3.cact` (the 20-layer, CQ2-quantised release)

If you use Needle in your own work, the authors ask for this citation:

```bibtex
@misc{needle3_2026,
  title        = {Needle: Foundation Tool-Calling Model for Tiny Devices},
  author       = {Ndubuaku, Henry and Mosoyan, Karen and Mroz, Jakub and Cylich, Noah and
                  Kumar, Satyajit and Sandhu, Parkirat and Shemet, Roman and Lee, Justin H.},
  year         = {2026},
  organization = {Cactus Compute, Inc.},
  howpublished = {\url{https://github.com/cactus-compute/needle}}
}
```

The Rust inference engine that loads the `.cact` file is a separate project:

- **needle-infer** (crate `needle-infer`, MIT) — <https://github.com/geekgineer/needle-rs>

## The hosted brain: your own key, your own service

Wisp does not ship a hosted model and is not affiliated with any of these. If you
choose one in Settings → Brain, it is called with **your** key, under **their**
terms, and your command is the only thing sent:

- **OpenAI** — <https://openai.com/policies>
- **Anthropic (Claude)** — <https://www.anthropic.com/legal>
- **Google (Gemini)** — <https://ai.google.dev/terms>
- **OpenRouter** — <https://openrouter.ai/terms>
- **Anything else you point it at** — Ollama, LM Studio, vLLM and llama.cpp are
  all commonly used locally, and their own licences apply to them.

The wire formats used are each service's public HTTP API; nothing of theirs is
bundled here.

## Speech

- **whisper.cpp** — <https://github.com/ggml-org/whisper.cpp> (MIT), through the
  `whisper-rs` crate (<https://codeberg.org/tazz4843/whisper-rs>, Unlicense).
  The bundled `models/ggml-tiny.en-q5_1.bin` is a quantisation of OpenAI's
  Whisper `tiny.en` (MIT) published by the whisper.cpp project.
- **sherpa-onnx** by the k2-fsa / Next-gen Kaldi project —
  <https://github.com/k2-fsa/sherpa-onnx> (Apache-2.0). It runs the wake-word
  spotter, Moonshine and SenseVoice, the Piper voices and the Kokoro voices.
- **Moonshine** — <https://github.com/usefulsensors/moonshine> (MIT), as the
  sherpa-onnx ONNX packs. It is the default recogniser: built for short
  utterances, and a fraction of a second per command.
- **SenseVoice** — <https://github.com/FunAudioLLM/SenseVoice> (MIT), via
  sherpa-onnx. The second opinion, and the multilingual option.
- **The wake-word model** — `sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01`
  from the sherpa-onnx `kws-models` release (Apache-2.0). Its `bpe.model` is also
  what turns your assistant's name into the sub-word tokens the spotter listens
  for; the tokeniser that does it (`crates/wisp-core/src/bpe.rs`) is ours, written
  against that model.
- **Piper** — <https://github.com/rhasspy/piper> (MIT). Default voice
  `vits-piper-en_US-lessac-medium`; the other voices on the Voice tab come from
  the same collection.
- **Kokoro** — <https://huggingface.co/hexgrad/Kokoro-82M> (Apache-2.0), via the
  `kokoro-multi-lang-v1_0` sherpa-onnx build.
- **WebRTC VAD** — <https://github.com/kaegi/webrtc-vad> (MIT), the voice
  activity detector that keeps the recogniser from running on silence.

## Everything else

| Component | Licence | Used for |
| --- | --- | --- |
| [egui / eframe](https://github.com/emilk/egui) | MIT OR Apache-2.0 | the settings app and installer windows |
| [windows-rs](https://github.com/microsoft/windows-rs) | MIT OR Apache-2.0 | microphone, media keys, volume, tray, windows, clipboard, drives, power, DPAPI |
| [cpal](https://github.com/RustAudio/cpal) | Apache-2.0 | audio capture |
| [ureq](https://github.com/algesten/ureq) | MIT OR Apache-2.0 | model downloads and the hosted model's HTTP calls |
| [png](https://github.com/image-rs/image-png) | MIT OR Apache-2.0 | screenshots |
| [tar](https://github.com/composefs/tar-rs), [bzip2](https://github.com/alexcrichton/bzip2-rs) | MIT OR Apache-2.0 | unpacking model packs |
| [zstd](https://github.com/gyscos/zstd-rs) | BSD-3-Clause | the installer payload |
| [serde](https://github.com/serde-rs/serde), [toml](https://github.com/toml-rs/toml), [log](https://github.com/rust-lang/log), [strsim](https://github.com/rapidfuzz/strsim-rs) | MIT OR Apache-2.0 | settings, logging, fuzzy app matching |

The clipboard, the window control and the machine statistics are Win32 calls
through `windows-rs` rather than another library, and the API key is encrypted
with Windows' own DPAPI (`CryptProtectData`). The icon is ours.

## What this repository does not include

`models/`, `dist/` and `target/` are not tracked. Run
`scripts/fetch-models.ps1` to download the models listed above from their
original publishers; each keeps its own licence.
