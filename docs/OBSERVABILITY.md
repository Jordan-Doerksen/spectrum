# Spectrum — logs, and how to find a fault with them

The observability law has two halves. The first half is log generation, which is the code
in `crates\engine\src\log.rs`. This file is the second half: the bug-identification-and-fix
plan. It gives the log location, the way to read a line, the triage order, and the areas
that are still silent. [CR-1 chunk 0, Definition of Done item 6]

> **Do not run the engine on the development machine.** A poll reads the stale
> `data\seen.json` (about 2281 keys) through Ollama and then posts to the live Discord
> channels. Use `.\scripts\dev.ps1 check`, `build` or `test`. [DECISIONS.md, chunk 0 DoD
> item 8]

---

## 1. Where the log is

| What | Value |
|---|---|
| Directory | `data\logs\`, beside `config.local.json` |
| File name | `spectrum-{date}-{time}-{pid}.jsonl` — one file per run |
| Format | One JSON object per line (JSON Lines). One event is one line. |
| Console | The same event, as one readable line. Warnings and errors go to stderr. |
| Rotation | The file rotates at `max_file_bytes` (5 MB). One run keeps `max_files_per_run` files (3). |
| Old runs | `keep_run_files` (20) earlier run files stay in the directory. The logger deletes the rest when it starts. |

Every value above is a key in the `logging` block of `config.local.json`. The block is
optional: leave it out and these defaults apply. See `config.example.json` and
`crates\engine\src\config.rs`.

Three facts about the file:

* **The panel and the runner write to different files.** Each process opens its own file,
  because the name holds the process id. Read both when both ran.
* **No secret reaches the file.** Every event name and every string value passes through
  `log::redact` (`crates\engine\src\log\redact.rs`). A Discord webhook keeps its id and
  loses its token. The repository is public, so this rule has no exception.
* **The logger reports its own failure.** A write error goes to stderr once per distinct
  message, and `log::health()` counts the errors, the rotations and the bytes.
  `runner.stopped` prints these counts at the end of a clean run.

To turn the file off, set `logging.enabled` to `false`. The logger then writes one
`log.file.disabled` event and keeps the console lines.

---

## 2. How to read a line

```json
{"ts":"2026-09-18T20:14:03.123Z","level":"warn","event":"feed.fetch.failed",
 "feed":"Reuters Business","url":"https://…","error":"timed out",
 "not_doing":"read any item from this feed",
 "why":"the cycle continues with the other feeds, so this source is missing from this poll"}
```

| Key | Meaning |
|---|---|
| `ts` | UTC. The clock is UTC everywhere, with no zone guessing. |
| `level` | `error`, `warn`, `info` or `debug`. |
| `event` | The event name. The names are a fixed vocabulary — see section 3. |
| `error` | What went wrong. This is the error text from the operation that failed. |
| `not_doing` | The action that did **not** happen because of this failure. |
| `why` | The consequence of that action not happening. |

`error` and `why` are different keys with different jobs. A failure record carries both.

Read the log with PowerShell:

```powershell
$log = Get-ChildItem data\logs\*.jsonl | Sort-Object LastWriteTime | Select-Object -Last 1

# every failure, newest last
Get-Content $log | Where-Object { $_ -match '"level":"(error|warn)"' }

# one event type
Get-Content $log | Select-String 'card.dropped'

# the two summary lines of every cycle
Get-Content $log | Select-String 'poll.summary|drip.summary'

# as objects, for counting
Get-Content $log | ForEach-Object { $_ | ConvertFrom-Json } |
    Group-Object event | Sort-Object Count -Descending
```

---

## 3. The event vocabulary

### Start and stop

| Event | Level | Says |
|---|---|---|
| `log.started` | info | The log file is open. It names the file, the levels and the caps. |
| `log.file.disabled` | warn | `logging.enabled` is false. No file is written for this run. |
| `log.init.failed` | error | The log directory is not writable. The console is the only record. |
| `log.level.unknown` | warn | A level word in the config is not a level. The logger uses `info`. |
| `engine.started` | info | The engine is built. It names the config, the store, the feed count and the tuning. |
| `runner.started` / `runner.stopped` | info | The headless runner. `runner.stopped` carries the log health counts. |
| `runner.stopped.failed` | error | The runner exited on an error. It stopped polling. |
| `engine.init.failed` | error | The engine could not be built. The poll loop never started. |
| `panel.started` | info | The Tauri panel started. It names the config file and the autostart flag. |
| `panel.runtime.failed` | error | The panel's engine thread has no Tokio runtime. The window shows an empty dashboard and polls nothing. |
| `panel.config.unreadable` | warn | The panel could not read the config for its logging settings. It uses the defaults. |
| `panel.mutex.poisoned` | error | A thread panicked while holding a shared lock. The panel recovers the guard and keeps serving the last published values. |

### The single-instance lock (headless runner)

| Event | Level | Says |
|---|---|---|
| `lock.taken` | info | This copy holds the lock. `how` is `fresh`, `stale`, `unreadable` or `same-pid`. |
| `lock.busy` | error | Another copy holds the lock. **This copy did not start, and it posted nothing.** |
| `lock.stale.recovered` | warn | The holder is gone. This copy took the lock. It names the dead pid and the check that decided. |
| `lock.unreadable` / `lock.self` | warn | The lock file holds no pid, or it names this process. It is treated as stale. |
| `lock.heartbeat.failed` | warn | The lock file cannot be rewritten. After the stale window another copy can take it and double-post. |
| `lock.release.failed` | warn | The lock file was not deleted on exit. The next start recovers it. |
| `runner.panel.running` | warn | `spectrum-pro.exe` runs on this host. The lock does not cover it, so both copies can post. |

### One cycle

| Event | Level | Says |
|---|---|---|
| `feed.fetch.failed` | warn | One feed did not answer. The poll continues without it. |
| `feeds.all.failed` | error | Every feed failed. Nothing reached the analyzer. Check the network. |
| `analyze.failed` | warn | Ollama did not answer for this headline. The item stays unseen and every later poll reads it again. |
| `analyze.failed.repeated` | warn | The same error hit more items in this poll. The roll-up carries the count. |
| `analyze.unmapped` | warn | The model answered with a word the analyzer does not know. The item was degraded to a drop, or to severity 1, and marked seen. It can never be offered again. |
| `analyze.unmapped.repeated` | warn | The same unknown answer came back. The model's output format has drifted. |
| `seen.absent` | info | No store file yet. This run seeds the backlog and posts nothing. |
| `seen.loaded` | info | The store is loaded. It carries the key count. |
| `seen.read.failed` / `seen.parse.failed` | error | The store is unreadable or corrupt. **The store starts empty and the engine re-seeds.** |
| `seen.save.failed` | error | The store was not written. A restart reads the same headlines again and can post them. |
| `seen.dir.failed` | warn | The store directory could not be created. The save below fails too. |
| `config.reloaded` | debug | The panel's edit is live. |
| `config.reload.failed` | warn | The edited config is not valid. The config in memory stays live. |
| `config.load.failed` | error | The config file is missing or invalid. Nothing starts. |
| `webhook.missing` | warn | A band has no webhook. Every card it clears leaves the queue and is lost. |
| `queue.backlog` | warn | The queue holds more cards than the drip can post before the next poll. The tail waits, and a restart loses it. |
| `poll.summary` | info | One line per poll: feeds, items, what was deduped, what failed, what was queued. |

### Delivery

| Event | Level | Says |
|---|---|---|
| `card.posted` | info | One card reached Discord. It names the band, the webhook id and the headline. |
| `card.dropped` | warn/error | One card left the queue and never posted. `reason` is `dry-run`, `post-failed` or `no-webhook`. **The card is gone: its headline is already marked seen.** |
| `drip.summary` | info | One line per drip tick, with the session totals for posted and dropped cards. |
| `feedcheck.ok` / `feedcheck.failed` / `feedcheck.done` | info/error | The `--feedcheck` pass only. It posts nothing and takes no lock. |

---

## 4. Triage order

Read the events in this order. Each step answers one question, and the first step that
fails is the fault.

1. **Did this run start at all?** Find the newest file in `data\logs\`. No file means the
   process did not reach `log::init`: check that `config.local.json` exists, and that
   `data\logs\` is writable. The console still carries `log.init.failed`.
2. **Is this the only copy?** `lock.busy` means this copy did nothing. `runner.panel.running`
   means the panel is up as well, and both can post. `lock.stale.recovered` after a crash is
   normal.
3. **Did the config load?** `config.load.failed` and `engine.init.failed` stop everything.
   `webhook.missing` means one band silently loses every card.
4. **Did the feeds answer?** `feeds.all.failed` is a network fault. Repeated
   `feed.fetch.failed` for one source is a dead feed. `poll.summary` gives
   `feeds_ok` against `feeds_failed`.
5. **Did the analyzer answer?** `analyze.failed` with a connection error means Ollama is
   not running. `analyze.unmapped` means Ollama answers but the answer has changed shape:
   check the model name and the prompt (`crates\engine\src\analyze.rs`).
6. **Where did the items go?** `poll.summary` accounts for every item:
   `items_gathered` = `deduped_already_seen` + `read_ok` + `read_failed`, and `read_ok`
   splits into `dropped_category`, `dropped_below_floor` and `queued`. A poll with a high
   `deduped_already_seen` and a `queued` of zero is the normal quiet case.
7. **Did the cards post?** `drip.summary` gives `posted` against `dropped`. Every
   `card.dropped` names its reason. `queue.backlog` means the drip is too slow for the
   poll rate: lower `poll_minutes`, or raise `max_per_drop`.
8. **Will the next run repeat this one?** `seen.save.failed`, `seen.read.failed` and
   `seen.parse.failed` all mean the dedupe store is not on disk as expected. After any of
   them the next run re-seeds, and the whole backlog is marked seen again.

### Symptom to first event

| Symptom | Look for |
|---|---|
| Nothing posts | `lock.busy` · `webhook.missing` · `card.dropped` · `poll.summary` `queued=0` |
| Every card posts twice | `runner.panel.running` · two files in `data\logs\` from one period · `lock.stale.recovered` |
| The same headlines come back | `seen.save.failed` · `seen.parse.failed` · `seen.read.failed` |
| Cards are late | `queue.backlog` · `drip.summary` `queue_len` |
| The engine looks healthy and says nothing | `analyze.unmapped` · `analyze.failed.repeated` · `poll.summary` `read_failed` |
| The panel dashboard does not move | `panel.mutex.poisoned` · `panel.runtime.failed` · `engine.init.failed` |

---

## 5. The areas that are still silent

These are known. They are recorded here so that the next chunk does not find them again.

1. **A panic prints to stderr.** The release panel hides the console
   (`src-tauri\src\main.rs:4`), so a panic message in the engine thread reaches nobody. The
   panel records `panel.mutex.poisoned` once per mutex after the fact, but the panic text
   itself is lost. The runner shows the panic, because it owns a console.
2. **The drip queue is in memory, and it has no bound.** `queue.backlog` warns when the
   queue is longer than the drip can drain, but no record names the cards that a shutdown
   loses. Those cards are already marked seen, so they never come back. [CR-1 chunk 2]
3. **A dropped card is not retried, and nothing records the loss a second time.**
   `card.dropped` is written once. There is no dead-letter file.
4. **The dedupe store holds no timestamps.** A key leaves only by FIFO eviction at the cap
   of 4000, and the eviction writes no record. [CR-1 chunk 2 replaces the store.]
5. **The panel does not check for a headless runner.** The runner warns when it sees
   `spectrum-pro.exe` (`runner.panel.running`), but the panel takes no file lock, so the
   reverse case is invisible. The fix is one lock in the engine crate, taken where the poll
   loop starts.
6. **`deploy\STOP.bat` (Riley's packaged copy) kills the headless process only.** The root
   `STOP.bat` was fixed in chunk 0 and probes all three targets. The packaged copy belongs
   to chunk 5.
7. **The `model` key in `config.local.json` is inert.** `crates\engine\src\analyze.rs`
   hardcodes `llama3.1:8b`. A log line never names the model in use. [CR-1 S7]
8. **No timing is recorded.** No event carries a duration, so a slow feed or a slow Ollama
   read is invisible until it times out.
9. **An unknown key in `config.local.json` is discarded in silence.** There is no
   `deny_unknown_fields`, so `min_severty` gives the default and no warning. The test
   `gap_an_unknown_key_is_ignored_so_a_typo_silently_keeps_the_default` records this.

---

## 6. The tests that hold these claims

| Claim | Test |
|---|---|
| The file is written, stays under its cap, and carries no secret | `crates\engine\tests\log_file_sink.rs` |
| Pruning never deletes the single-instance lock | `crates\engine\tests\log_prune.rs` |
| A token never reaches a log line, a panel line, or an event name | `crates\engine\tests\log_redaction.rs`, `crates\engine\src\engine.rs` (`ring_line`), `crates\engine\src\engine\drip.rs` (`post_failed_line`) |
| An error and a `not_doing` reason survive on one line | `crates\engine\tests\log_redaction.rs` |
| A model answer that cannot be mapped is reported | `crates\engine\src\analyze.rs` |
| The lock refuses a second runner, and its knobs come from the config | `crates\headless\src\lock.rs` |
| Every config default | `crates\engine\tests\config_loading.rs` |

Run them with `.\scripts\dev.ps1 test --workspace`. No test opens a socket, calls Ollama
or Discord, or touches the real `data\` store.
