# When: acting later, or on a condition

Wisp does not have a "shutdown timer". It has a *queue*, and **anything** it can
do now can be said with a **when** attached:

```
"shut down in 30 minutes"
"shut down at 9pm"
"shut down when I close this application"
"lock the pc at 11pm"
"close chrome when spotify closes"
"play the music when I unplug the charger"
"open the report when chrome starts"
"take a note call the plumber when the battery drops below 20"
"dim the screen when I lock the pc"
```

The half before the *when* is an ordinary command, resolved exactly as it would
be if you had said it on its own — instant phrases first, then the brain. The
half after it is a **trigger**. Nothing else about how commands work changes: the
same functions, the same voices, the same confirmation out loud.

> It confirms what it understood, and it says it the way you said it:
> **"I'll shut down at 9 pm"**, **"I'll close chrome when spotify closes"**.

## Moments

| Say | When it happens |
|---|---|
| *"in 30 minutes"*, *"in an hour and a half"*, *"in 90 seconds"* | That far from now |
| *"at 9pm"*, *"at 21:00"*, *"at half past nine"* | The next time the clock shows it — today if it is still to come, tomorrow if not |
| *"tomorrow at 9"*, *"tonight at 8"*, *"noon"*, *"midnight"* | The obvious reading |

A moment fires within a few seconds of its time, and it survives a restart:
the queue is a file, so a shutdown queued for tomorrow evening is still queued
after a reboot.

**A moment that is missed is not run late.** If the machine was asleep or off
when the time came, the entry is dropped when it comes back and Wisp says so out
loud — *"I missed what you queued — shut down at 9 pm. The PC was away when it
should have happened."* Shutting down at six in the morning because it was
supposed to happen at nine the night before is not what anyone means. Anything
missed by less than five minutes still runs.

## Conditions

| Say | Fires when |
|---|---|
| *"when I close this application"*, *"when this app closes"* | The window that was in front when you spoke exits. The exact process is recorded, so it cannot be confused with a similarly named one |
| *"when chrome closes"*, *"when spotify quits"*, *"once steam has exited"* | That program stops running |
| *"when chrome starts"*, *"when spotify opens"* | That program starts running |
| *"when the battery drops below 20"*, *"when the battery hits 10 percent"* | On battery, at or below that level |
| *"when I plug the charger in"* / *"when I unplug the charger"* | The mains state *changes* |
| *"when the internet comes back"* / *"when the internet drops"* | A route out appears or disappears |
| *"when the screen locks"* / *"when the screen unlocks"* | The lock screen takes or gives back the desktop |
| *"when I'm idle for 10 minutes"*, *"when I leave the desk for 5 minutes"* | No keyboard or mouse input for that long |
| *"when the build zip appears"*, *"when the download finishes"* | That file starts existing |

Some triggers are about a **change** and some about a **state**, and the
difference is deliberate:

* *"when I unplug the charger"* said while it is already unplugged waits for the
  next unplugging, because the status quo is not an event.
* *"when spotify closes"* said while Spotify is not running also waits: an app
  that is already closed has not closed *again*.
* *"when I close this application"* is the opposite promise — it points at a
  process that is running at that moment, so it fires as soon as that process is
  gone.
* *"when I'm idle for 10 minutes"* fires the first time it is true.

## Seeing and cancelling the queue

Everything queued is listed in **Settings → Alarms & reminders → Queued for
later**, with what it will do and when, a cross next to each one, and *Cancel
them all* at the bottom. The dashboard shows how many are waiting.

By voice:

| Say | Does |
|---|---|
| *"what's queued"* / *"what have you got waiting"* | Lists them |
| *"what's coming up"* | The next alarm *and* the next queued action, whichever is sooner |
| *"cancel everything you queued"* | Clears the queue |
| *"cancel the queued shutdown"* | Removes the ones that mention it |
| *"cancel the shutdown"* | Works whether the shutdown is queued or already counting down |

## What it will not queue

* **Schedule items.** *"Remind me to call mum at 4 pm"* already has a time; it
  becomes a reminder, not a queued command. The same goes for alarms, events and
  to-dos.
* **Dismissals.** *"Never mind at 9pm"* is not a thing to do later.
* **Questions.** *"What's on my schedule at 9"* asks for an answer now, so the
  trigger is left alone.
* **Commands it cannot resolve.** If the half before the *when* is not something
  it can do, the whole sentence is handled the ordinary way instead — with a web
  search, or a question back — rather than queuing a mystery.

## Under the hood

`%APPDATA%\Wisp\pending.json` — plain JSON, one entry per queued action:

```json
{
  "id": 3,
  "trigger": { "kind": "app_closes", "app": "Spotify", "pid": 4812 },
  "actions": [{ "Machine": { "op": "shutdown_pc", "args": [] } }],
  "label": "shut down",
  "created": { "year": 2026, "month": 10, "day": 25, "hour": 21, "minute": 4 },
  "last_seen": null
}
```

The actions are stored exactly as the runner would have run them, so firing one
later is the same code path as doing it now. The agent's scheduler thread checks
the queue once a second, gathering only the facts the queue actually needs — a
queue of nothing but timers never touches the network or the process list.

The trigger parser lives in `crates/wisp-core/src/when.rs` and the queue in
`crates/wisp-core/src/pending.rs`; both have their own tests, including the
sentences that must *not* be turned into queue entries.

## From a command line

```powershell
Wisp.exe --ask "shut down in 30 minutes"          # what it would do, and why
Wisp.exe --ask "shut down in 30 minutes" --do     # actually queue it
Wisp.exe --ask "what's coming up" --do            # read the queue back
```
