# When it mis-hears you

A voice assistant's worst failure is not a wrong answer. It is a command that
never arrived: you say exactly the right words, the recogniser hands the brain
something else, and the assistant answers a sentence nobody said. A function you
added yourself is the most exposed to this, because its name is a word the
recogniser has never been taught — and the model then gets blamed for not knowing
a function it knows perfectly.

## What happens now

The first transcript is still the fast one, and almost always the right one. When
it leads nowhere, three things are true and the assistant uses all of them:

1. **The caption is already there.** While you were speaking, a streaming model
   was showing the words on screen. It is a different recogniser from the one that
   answers, and it has already run — asking it costs nothing and needs nothing
   loaded. If the caption says something different, that gets a turn first.
2. **Another engine re-hears the same clip.** Every engine on the PC other than
   the chosen one, quickest first: Moonshine v2 (0.3 s), the other Moonshine pack,
   SenseVoice (0.3 s), Dolphin, and Whisper last — Whisper is the most accurate
   and the slowest by an order of magnitude (about 20 s for a three-second clip on
   this CPU), so it is the last thing tried rather than, as it used to be, the
   only thing tried.
3. **Then it asks you to repeat it.** If two recognisers agree on the words, that
   is the end of the argument: the words really were unclear, and saying the
   apology is honest.

Nothing is said out loud until one of those has finished. The old behaviour —
"Sorry, I didn't catch that" spoken over the top of a command it could have found
with a second listen — is what this replaces.

## What it sounds like in the log

```
heard: "ibeneat the computer" -> command "ibeneat the computer"
command "ibeneat the computer" -> [] via Api in 1284 ms
nothing understood in "ibeneat the computer" — holding "I didn't quite catch that" in case another ear can hear it
the caption heard "hibernate a computer" (the answer heard "ibeneat the computer"); listening to it instead
command "hibernate a computer" -> [Custom { name: "hibernate", params: [] }] via Instant in 2 ms
  ✓ hibernating
```

## Measured on real recordings

Four clips of one person saying *"hibernate a computer"*, saved by the assistant
itself (`%APPDATA%\Wisp\clips`), run through every installed engine:

| Engine | Heard | Time |
|---|---|---|
| SenseVoice (what was selected) | *iening a computer*, *ierne the computer*, *ibeneat the computer*, *ibeat the computer* — **0 of 4 right** | 0.3 s |
| Moonshine v2 | *hibernate a computer*, *hibernate a computer*, *hibernate the computer* — **3 of 4 right** | 0.3 s |
| The streaming caption model | *hibernate a computer* — **4 of 4 right** | already running |

That is the whole bug in one table: the engine was fast and the ear was wrong, the
caption had the right words on screen the entire time, and the model — which had
the `hibernate` function in its tool list — was handed gibberish.

With the second-opinion chain, all four clips now reach
`[Custom { name: "hibernate" }]`.

## Phrase matching is forgiving, on purpose

A function's own phrases can run without any model at all, which is why they are
matched against a few near-misses as well as the exact words:

| Said (or heard) | Matches |
|---|---|
| *"hibernate the computer"*, *"hibernate a computer"*, *"hibernate computer"* | the phrase, with the article ignored |
| *"hibernet the computer"*, *"hybernate the computer"* | one long word may lose up to two letters |
| *"hibernating the computer"* | **not** matched instantly — a different real word goes to the brain instead |

That last line is deliberate. This path runs without the model, so a wrong match
runs the wrong function with nobody to catch it: two wrong words is a different
sentence that happens to share a shape, and guessing is worse than asking.

## Picking an engine

Settings → Listening lists the engines, whether each one's models are actually on
disk, and — under **If it mis-hears you** — what will be tried before it gives up.
Switching engine takes effect at the next command; nothing needs reinstalling.

`Whisper` with a big model is the most accurate, and the most expensive: about six
seconds per command on this CPU for `medium.en`, and more for a larger one. It is
worth it only if the fast engines keep missing you.

## Checking a specific clip

```powershell
# what the configured engine hears
Wisp.exe --transcribe "$env:APPDATA\Wisp\clips\cmd-1234-5678ms.wav" --out out.txt

# what each engine hears, with timings
Wisp.exe --bench-stt sensevoice "$env:APPDATA\Wisp\clips\*.wav"
Wisp.exe --bench-stt moonshine-v2 "$env:APPDATA\Wisp\clips\*.wav"
Wisp.exe --bench-stt whisper "$env:APPDATA\Wisp\clips\*.wav"
```

A clip is saved for every command when **Save what it hears** is on, so "it
mis-heard me" is always something that can be looked at afterwards rather than
argued about.
