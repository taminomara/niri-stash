# Feature Implementation Plan: Floating Window Stash for niri

> **Amended after the build.** Resizing was removed (spec amendment A1): a stash is one nudge
> toward the edge, a restore one nudge back, and any hand change to a stashed window forgets it.
> The settling period was removed too (spec amendment A2, plan D-23): the engine reads the window
> list back after each batch and has no timers. The daemon is then a single thread around one
> `poll` call (plan D-24), with no channel, no `busy-timeout` reply, and no exit code 70.
> A `--no-focus-follows-mouse` flag relaxes the alt-tab rule to any focus change, and the restore
> focus comes from niri's focus timestamps (plan D-25). Paragraphs below about the 32-pixel resize,
> pop-back, tiled windows, settling, deadlines, press completion, the three threads, and the
> daemon's own focus history describe the first build.

This plan builds `niri-stash`, a small Rust program that runs as a user service beside niri. Two
hotkeys talk to it over a local socket: one tucks the floating windows on the current workspace
into slivers at the nearest screen edge or brings them back, and the other always brings them back.
The daemon also watches niri's event stream to unstash a window you alt-tab into. The approach is
deliberately plain: one binary, three standard threads, no async runtime. The daemon writes the
desired state to a file before it sends niri any command, so a restart can re-apply it. The one
thing to know first: every move is a relative nudge from the position niri last reported, because
niri's absolute-move command uses a different origin than the one it reports positions in.

<!--
HOW THIS FILE IS LAYERED. This file is the reader's layer: intention, goals, and sequence in
plain language. Decision rationale and rejected alternatives live in
artifacts/implementation-decision-log.md; team composition and round history live in
artifacts/implementation-iteration-history.md.
-->

## Outcome

When this plan is done, `~/.cargo/bin/niri-stash` exists, a user service starts it with the
graphical session, and two hotkeys are bound in niri. `Mod+S` stashes the floating windows on the
focused workspace as slivers and, pressed again, restores them at their exact size and place.
`Mod+Shift+S` (also sent by Fn+F11) always restores. Alt-tabbing into a sliver brings that one
window back. Dragging a sliver pops it back to full size where you drop it.

The daemon survives its own restarts within a login session without losing the original geometry
of any stashed window, and when niri itself restarts it starts fresh. It serves one person on one
laptop. Every failure ends up in the journal under the `niri-stash` tag.

## User Stories

- **US-1:** As the user, I want one hotkey to tuck every floating window on my workspace into a
  sliver at its nearest screen edge, so that my tiled windows are unobstructed.
- **US-2:** As the user, I want the same hotkey to bring every sliver back at exactly the size and
  position it had, so that stashing costs me nothing.
- **US-3:** As the user, I want alt-tabbing into a sliver to bring only that window back, while a
  pointer passing over a sliver does nothing, so that I can reach one window without unstashing the
  rest.
- **US-4:** As the user, I want a sliver I drag somewhere to pop back to full size where I drop it,
  and a sliver I resize by hand to stay as I left it, so that my own hand changes win.
- **US-5:** As the user, I want my slivers to still come back after the daemon restarts, so that a
  rebuild or crash never strands a window at 32 pixels.
- **US-6:** As the user, I want a second hotkey that only ever unstashes, so that I do not have to
  think about which way the toggle will go.
- **US-7:** As the operator of my own session, I want the daemon to start with the session, wait for
  niri, restart after a failure or a niri restart, and report every problem in the journal, so that
  the hotkeys work whenever I can press them.

## Constraints and Boundaries

- **Driving constraint:** No deadline. niri has no scratch workspaces, and this is the user's
  workaround. The work item fixes two constraints: the daemon is written in Rust, and the project
  lives in `~/p`.
- **Recorded boundary:** The operator's typed request, recorded word for word in
  [artifacts/scope-boundary.md](artifacts/scope-boundary.md). Nothing was cut for scope. Two items
  the work item does not name are in by the user's direction (escalation E2): the explicit unstash
  command with its own hotkey, and replacing the existing `Mod+Shift+S` region-screenshot binding
  with it (`Print` keeps the region screenshot).
- **Out of scope:** Everything in the specification's Out of Scope list (tiled windows, other
  workspaces and monitors, any visible indicator, launch-or-summon, changes to niri or noctalia,
  multi-monitor behavior), plus the specification's own Deferred list (per-app exemptions, an
  explicit stash-only command and a status command, re-stashing after the usable area changes,
  spreading overlapping slivers, feedback on a no-op press).
- **Watch after ship:** How often the journal shows a settle reaching its cap, and how often a
  stash is forgotten because an application answered a resize late. Either one recurring means the
  settling values need another look
  ([D-9](artifacts/implementation-decision-log.md#d-9-settling-period-values-and-rules)).

## Implementation Approach

The daemon is one long-running process with three threads. One thread reads niri's event stream
and does nothing but hand each event on. One thread accepts hotkey connections. One thread owns all
state and does all the work: it takes events and hotkey requests from a single queue, talks to niri
over short-lived request connections, and writes the state file. Because one thread handles
everything in order, a press and the events it causes can never race each other
([D-5](artifacts/implementation-decision-log.md#d-5-three-std-threads-and-one-channel)).

The hotkey client is the same binary with a different subcommand. It sends one line, waits for one
line, and exits with a code that says what happened. niri runs it through `systemd-cat`, so its
errors reach the journal.

Most of the logic is pure and kept apart from I/O so it can be unit tested: picking the edge,
computing the sliver target, recognising alt-tab from focus events, telling a hand move from a hand
resize, and deciding what to re-apply at startup
([D-20](artifacts/implementation-decision-log.md#d-20-pure-core-unit-tests-plus-a-manual-checklist)).

- Module layout as built (after the structural pass that followed the first build): `core/`
  holds the pure logic with no compositor dependency: `engine` keeps two maps per window, the
  persisted desired state and the runtime `Observed` state (`window`), and routes every change,
  startup included, through one `transition_to` step that diffs the new desired map against the
  current one, writes it, and sends what the diff calls for; `settle` timers; `focus`; `geometry`;
  `command` and `input` as the core's own outbound and inbound types; `ports` with the `StateStore`
  and `Compositor` traits. `niri/` is the adapter (IPC client, event reader, and `translate` between niri's types
  and the core's). `hotkey/` holds the line protocol, the socket server, and the client command.
  `daemon/` holds startup (retry, socket claim), the runtime threads and state loop, and the exit
  codes. `state_file` is the serde model with atomic load and save. The library's public surface is
  `run_daemon`, `run_client`, `Request`, and the usage exit code; everything else is crate-private.

### Project and packaging

A new Cargo project at `~/p/niri-stash`, a git repository on branch `main`, building one binary
with three subcommands. Arguments are parsed by hand because there are three verbs and no options
([D-1](artifacts/implementation-decision-log.md#d-1-single-binary-with-three-subcommands)). The
licence follows the one GPL crate it links
([D-2](artifacts/implementation-decision-log.md#trivial-decisions)). The dependency list is short
and the niri crate is pinned to the exact niri release on this machine, because a new niri event
type would otherwise break parsing silently
([D-3](artifacts/implementation-decision-log.md#d-3-dependencies-and-an-exact-niri-ipc-pin)).

- Subcommands: `niri-stash daemon`, `niri-stash toggle`, `niri-stash unstash`.
- Licence: `GPL-3.0-or-later`.
- Dependencies: `niri-ipc = "=26.4.0"`, `serde`, `serde_json`, `log`, `env_logger`, `anyhow`.
- `panic = "abort"` in both the dev and release profiles.
- Installed with `cargo install --path .` to `~/.cargo/bin/niri-stash`
  ([D-4](artifacts/implementation-decision-log.md#trivial-decisions)).

### Talking to niri

The daemon uses its own small client over a Unix socket, built on the `niri-ipc` request and reply
types. It opens one connection per press, with a 1-second read and write timeout, and drops the
connection on any error. niri never reports an action as failed for a window that has gone away,
so an error from the socket always means niri itself is gone
([D-6](artifacts/implementation-decision-log.md#d-6-thin-niri-client-with-timeouts)).

### How a window is stashed and restored

Every position change is a relative nudge from the last position niri reported, never an absolute
move. niri reports positions from the corner of the output but places absolute moves from the
corner of the working area (the output minus the bar), and the two differ by the bar height.

A stash shrinks the window to 32 pixels on the axis that points at its edge, then nudges it 10,000
pixels toward that edge. Both requests go out back to back. niri stores the requested position
without clamping and re-clamps it every time the size changes, so the window lands as a sliver
once the application answers the resize.

A restore nudges the window back by the difference
between its original position and its current one, then asks for the original size. The move goes
first because niri adds the nudge to the position it currently shows, and that position jumps when
the resize lands
([D-7](artifacts/implementation-decision-log.md#d-7-relative-moves-and-one-phase-stash-and-restore)).

- Stash: `set-window-width`/`set-window-height` to 32 on the edge axis, then
  `move-floating-window` with `AdjustFixed(±10000)` on that axis.
- Restore: `move-floating-window` with `AdjustFixed(original − current)`, then resize to the
  original width and height.
- On this laptop a left-edge sliver reports `x ≈ −22` (32 px wide, 10 px visible).

The edge is the one closest to the window's centre, measured from the output's edges (the work
item says "edge of the monitor"). The sliver still lands against the bar on the top edge, because
niri's clamp works on the working area. Measuring from the output only shifts the choice for a
window whose centre sits within the bar's 28 pixels of a tie. Ties go left, right, top, bottom
([D-8](artifacts/implementation-decision-log.md#d-8-edge-distance-measured-from-the-output-edges)).

After sending its requests the daemon waits for the window to settle, and treats every layout
change during that wait as its own. The wait ends after 150 ms with no further change, counted from
the first layout report of any kind, or at a 1-second cap. Work unit 1's measurement changed the
rule: it had started the quiet interval only on a size change, and an application that refuses to
shrink (gnome-calculator) then never changes size, so every press waited the full second. niri
answers a scripted resize within a frame, so a late answer after the quiet interval is not a risk
worth that cost ([D-9](artifacts/implementation-decision-log.md#d-9-settling-period-values-and-rules)).

A hand drag announces itself. The user's event log shows that when a floating window is picked up,
niri sends a window-changed report with no workspace and no position, and when it is dropped it
sends another with the workspace and the new position. A report with no workspace therefore means
"in the user's hand": the daemon keeps the entry and does nothing. The next report that carries a
position is the drop, and that is the geometry the hand-move or hand-resize decision is made on. A
keybind move to another workspace does not lift the window: it is reported once, on the new
workspace, at the same position, and the entry simply follows it. A hand resize looks different: the same log shows a corner drag as about 75 layout reports, one per
frame, with the size shrinking and the position fixed, and no lift-and-drop pair around them. For
that, and for any change not bracketed by a lift and a drop, the daemon waits 150 ms of no further
change and judges the stream once, at its end. Closing a window reports the focus moving to the next
window first and the close itself second; both are direct transitions and neither feeds alt-tab
detection ([D-9](artifacts/implementation-decision-log.md#d-9-settling-period-values-and-rules)).

- A scripted resize or move from the daemon is reported once or twice, at once, with no per-frame
  animation, and a request that changes nothing (a push against an edge the window already touches)
  is reported not at all. Settling must therefore end on the cap, or on the "already at target"
  shortcut, when no report is coming. Measured: a 32 px window pushed left settles at x = -22.3 (10 px
  visible); a 900 px window pushed right shows 73.7 px.
- The captured logs live at `artifacts/niri-event-log-2026-10-11.jsonl` (float, hand resize, hand
  move, close) and `artifacts/niri-event-log-2026-10-11-1.jsonl` to `-5.jsonl` (scripted stash and
  restore; alt-tab, cancel, and pick-same through niri's switcher; keybind workspace moves and a
  tile-float round trip; lock and unlock, overview, close in overview; close in the switcher). They
  are the replay fixtures for the focus detector, the classifier, and the settling engine.

- `SETTLE_QUIET_MS = 150`, `SETTLE_CAP_MS = 1000`. The same `SETTLE_QUIET_MS` debounces unexpected
  changes that are not bracketed by a lift-and-drop pair.
- A window-changed report for the window that already has focus is not a focus transition and does
  not feed alt-tab detection.
- No settling when the snapshot already matches the target within 1 logical pixel.
- All windows of one press settle in parallel. At the cap the daemon adopts the latest reported
  geometry and logs a warning.
- Focus changes the daemon itself asks for (spec D8) are marked as expected and stop being expected
  when the press's settling ends, so a focus request niri ignores leaves nothing stale behind.

If a window closes or leaves the floating layout while a press is settling it, the daemon drops it
from the press. A window that merely has no workspace at that moment is being dragged or moved, not
gone: it stays in the press and in state, and its settling resumes when it lands
([D-17](artifacts/implementation-decision-log.md#d-17-vanished-and-tiled-windows-mid-press)). A fresh window list at the end of the press catches anything the event stream has
not shown yet. The press still answers `ok`
([D-17](artifacts/implementation-decision-log.md#d-17-vanished-and-tiled-windows-mid-press)).

### Hotkey socket contract

This resolves the specification's open item OI-1. The hotkey client, the daemon, and the niri
keybind all follow it ([D-10](artifacts/implementation-decision-log.md#d-10-hotkey-socket-protocol)).

The client connects, writes one request line, shuts down its write half, and reads one reply line.
A press that skipped a vanished window, or had nothing to do, still answers `ok`.

- Socket path: `$XDG_RUNTIME_DIR/niri-stash/sock`, mode `0600`.
- Request grammar: `request = ("toggle" / "unstash") LF`
- Reply grammar: `reply = ("ok" / "err" SP code SP message) LF`
  - `code = "niri-unavailable" / "state-unwritable" / "busy-timeout" / "unknown-command"`
  - `message` = printable UTF-8 with no LF.
- Worked example, success: `C→D: toggle\n`, then `D→C: ok\n`.
- Worked example, failure: `D→C: err state-unwritable rename /run/user/1000/niri-stash/state.json: Read-only file system\n`.
- Daemon side: 500 ms read timeout on each accepted connection; if the state thread has not replied
  within 4 s the daemon answers `err busy-timeout`.

The client reports the outcome as an exit code. Its read timeout is 5 s, longer than the daemon's
4 s, so when the daemon is busy the client reports the daemon's reason rather than its own timeout
([D-11](artifacts/implementation-decision-log.md#d-11-hotkey-client-timeouts-and-exit-codes)).

| Client exit code | Meaning |
| ---------------- | ------- |
| 0 | Reply was `ok`. |
| 1 | Could not connect to the daemon (`ENOENT` or `ECONNREFUSED`). |
| 2 | Reply was `err`; the code and message are printed to stderr. |
| 3 | No reply within 5 s. |
| 64 | Usage error (unknown or missing subcommand). |

### Keybinds

Two lines in the user's niri config call the client through `systemd-cat`, because niri runs
keybind commands with stderr sent to `/dev/null`. The toggle goes in the Windows section. The
unstash line replaces the existing `Mod+Shift+S` region-screenshot line where it stands, by the
user's direction. `Print` keeps the region screenshot. The nearby comment about Fn+F11 should
say it now unstashes
([D-12](artifacts/implementation-decision-log.md#d-12-two-keybinds-through-systemd-cat)).

- File: `~/.config/niri/cfg/keybinds.kdl`.

```kdl
Mod+S hotkey-overlay-title="Stash floating windows" { spawn "systemd-cat" "-t" "niri-stash" "niri-stash" "toggle"; }
Mod+Shift+S hotkey-overlay-title="Unstash floating windows" { spawn "systemd-cat" "-t" "niri-stash" "niri-stash" "unstash"; }
```

### State file

The state file records what each tracked window should be, not what it was last seen as. It is
written before the first niri command of anything that changes it (a press, an alt-tab unstash, a
pop-back) and again whenever an entry is dropped. If the write at the start of a press fails,
nothing moves and the client gets `err state-unwritable`. A failed later write is logged as an
error and the daemon keeps running
([D-13](artifacts/implementation-decision-log.md#d-13-desired-state-file-format-and-write-points)).

- Path: `$XDG_RUNTIME_DIR/niri-stash/state.json`, written to `state.json.tmp` and renamed into
  place; no fsync (the directory is tmpfs), no version field, no lock.
- Field layout: `instance` (string, the value of `$NIRI_SOCKET`); `windows` (object keyed by the
  niri window id as a decimal string). Each entry has `original` (object with numbers `x`, `y`,
  `w`, `h`, output-relative logical pixels) and `desired` (`"stashed"`, in which case `edge` is
  also present with `"left"`, `"right"`, `"top"`, or `"bottom"`; or `"restored"`, with no `edge`
  field).
- Worked example:
  `{"instance":"/run/user/1000/niri.wayland-1.1572.sock","windows":{"12":{"original":{"x":540.0,"y":300.0,"w":800.0,"h":500.0},"desired":"stashed","edge":"left"}}}`

At startup the daemon discards the file if its `instance` does not match the current
`$NIRI_SOCKET`, drops entries whose window is gone, and re-applies the rest through the normal
path: stashed entries are stashed again, restored entries are restored and dropped. Entries whose
window is tiled or fullscreen keep only the original, so the window can be restored when it floats
again (spec D13).

### Startup and exit codes

At startup the daemon retries the niri connection for up to 60 seconds, waits up to 5 seconds for
niri's initial event snapshot, re-applies the saved state, and only then opens the hotkey socket.
A press can therefore never see half-restored state
([D-14](artifacts/implementation-decision-log.md#d-14-startup-sequence-and-stale-socket-handling)).

- Connect backoff: 100 ms, doubling, capped at 2 s, for 60 s in total. The first failure is logged
  at info, then one warning about every 10 s.
- If the socket file already exists: try to connect to it. Refused means a stale file, so unlink
  and bind. Accepted means another daemon owns it, so exit 78.

When niri's event stream ends, the daemon exits and lets systemd bring it back. It never deletes
the state file on the way out; the instance tag makes the next daemon ignore it. An event line it
cannot parse (a newer niri event type) is logged by variant name and skipped
([D-15](artifacts/implementation-decision-log.md#d-15-daemon-exit-codes-and-event-stream-end)).

| Daemon exit | Meaning | systemd restarts it? |
| ----------- | ------- | -------------------- |
| 69 | niri unreachable after 60 s, or no initial snapshot within 5 s | yes |
| 70 | a worker thread ended (the state thread's queue disconnected) | yes |
| 75 | niri's event stream ended (EOF or read error) | yes |
| 78 | `XDG_RUNTIME_DIR` or `NIRI_SOCKET` unset, or another daemon owns the socket | no |
| SIGABRT | a panic (`panic = "abort"`) | yes |
| SIGTERM | clean stop from systemd; no handler needed | — |

The user service copies the existing `asus-hotkey-notify.service` pattern
([D-16](artifacts/implementation-decision-log.md#d-16-systemd-user-unit)).

`NIRI_SOCKET` and `XDG_RUNTIME_DIR` are already in the user manager's environment, because niri
imports them before it reports ready.

- File: `~/.config/systemd/user/niri-stash.service`.
- `[Unit]`: `Description=Floating window stash for niri`, `PartOf=graphical-session.target`,
  `After=graphical-session.target`.
- `[Service]`: `ExecStart=%h/.cargo/bin/niri-stash daemon`, `Restart=always`, `RestartSec=2`,
  `RestartPreventExitStatus=78`.
- `[Install]`: `WantedBy=graphical-session.target`.
- Install: `cargo install --path .`, then
  `systemctl --user daemon-reload && systemctl --user enable --now niri-stash.service`. The README
  lists these commands; the build runs them on this machine.

### Logging

The daemon logs to stderr, which the journal captures. Each hotkey press gets a number, and every
line about that press carries it, so one press can be followed through the journal. Window titles
are never logged
([D-18](artifacts/implementation-decision-log.md#d-18-logging-content-and-correlation)). The
default level is info, and `RUST_LOG` overrides it
([D-19](artifacts/implementation-decision-log.md#trivial-decisions)).

- `env_logger`, timestamps off (the journal adds them), correlation field `press=<n>`.
- info: startup re-apply outcome, each press (workspace, direction, window ids), each forget or
  pop-back with its reason and the discarded original geometry, each alt-tab unstash, exit code and
  reason.
- warn: settle reached its cap, unparseable event skipped, connection retries.
- error: state file write failures.
- Windows are identified by id and `app_id`, never by title.
- debug: every settle with its elapsed milliseconds.

## Work Units and Sequencing

Four units, each ending in something you can try. The explicit `unstash` command ships in unit 2,
because it is the toggle with the direction fixed, and its keybind ships in unit 4 with the other
one ([D-21](artifacts/implementation-decision-log.md#d-21-unstash-ships-in-work-unit-2)).

| #   | Work Unit | Story | Delivers | Justification | Depends On | Verification |
| --- | --------- | ----- | -------- | ------------- | ---------- | ------------ |
| 1   | Scaffold, niri client, event logger, settling measurement | US-7 | The Cargo project and git repository; the thin niri client with timeouts; `niri-stash daemon` connecting to niri and logging every event; a measurement of how long WezTerm and gnome-calculator take to answer a resize, with the two settling constants set from it ([D-9](artifacts/implementation-decision-log.md#d-9-settling-period-values-and-rules)) | "Daemon should be implemented in rust, you can set up a project in ~/p"; "It reads niri's event stream"; the measurement is a necessity of T3, which asks planning to confirm the settling figures on a real terminal | — | `cargo build` and `cargo clippy` clean; daemon logs events while you move windows; measured resize times recorded in the D-9 entry of the decision log |
| 2   | Toggle and unstash end to end | US-1, US-2, US-6 | Fresh snapshot per press, edge pick, sliver target, press engine with settling, the desired-state file, the hotkey socket, the `toggle` and `unstash` client commands, and focus handling on stash and unstash (spec D8) | "a hotkey that puts all floating windows on the current workspace to the size where they barely stick out"; "Hotkeys interact with daemon via a socket"; unstash by user direction (E2) | 1 | Unit tests for edge pick, tie order, and sliver target pass; from a shell, `niri-stash toggle` stashes a floating WezTerm and `org.gnome.Calculator` and stashes again restores them; `niri msg -j windows` shows stashed width 32 (or the app's settled minimum), a left sliver at `x ≈ −22`, and restored geometry within 1 px; `niri-stash unstash` restores and is a no-op `ok` when nothing is stashed |
| 3   | Event-driven behavior | US-3, US-4 | Alt-tab detection with the same-window rule; a hand move pops the window back, a hand resize forgets it; closed, tiled, and re-floated windows; usable-area shifts treated as hand moves | "It reads niri's event stream to detect when a stashed window is alt-tabbed into"; "It also marks window as unstashed if user modifies its size/position manually"; the focus rule in the request | 2 | Unit tests for the focus triple (including return to the same window) and for move-versus-resize classification pass; by hand: alt-tab through noctalia's picker restores only that window, hovering a sliver does nothing, dragging a sliver pops it back where dropped, tiling and re-floating a sliver restores its original |
| 4   | Lifecycle and install | US-5, US-6, US-7 | Startup retry, instance tag and re-apply, the exit-code contract, stale-socket handling, the systemd unit, both keybinds (the unstash line replacing the screenshot line), and a README with the three install commands | "All of these operations are performed by a systemd-managed daemon process"; the spec's Outcome names the unit and the keybinds as deliverables (spec D11); the keybind replacement by user direction (E2) | 2, 3 | Unit tests for the re-apply decisions pass; stash two windows, `systemctl --user restart niri-stash`, press `Mod+S`, and both come back; `Mod+Shift+S` and Fn+F11 unstash and `Print` still takes a region screenshot; a second `niri-stash daemon` exits 78; stopping the daemon and pressing `Mod+S` puts a client error in `journalctl --user -t niri-stash` |

## Definition of Done

- [ ] Every item in the specification's "Done looks like this" list holds when checked by hand,
      with `niri msg -j windows` confirming the geometry
      ([D-20](artifacts/implementation-decision-log.md#d-20-pure-core-unit-tests-plus-a-manual-checklist)).
- [ ] Unit tests cover edge pick and tie order, the sliver target, the focus triple including the
      same-window return, move-versus-resize classification with its 1 px tolerance, and the startup
      re-apply decisions ([D-20](artifacts/implementation-decision-log.md#d-20-pure-core-unit-tests-plus-a-manual-checklist)).
- [ ] The hotkey client and daemon follow the socket contract exactly, and each client exit code
      can be produced on purpose ([D-10](artifacts/implementation-decision-log.md#d-10-hotkey-socket-protocol),
      [D-11](artifacts/implementation-decision-log.md#d-11-hotkey-client-timeouts-and-exit-codes)).
- [ ] The service is enabled, starts with the session, and is not restarted after exit 78
      ([D-16](artifacts/implementation-decision-log.md#d-16-systemd-user-unit)).
- [ ] Both keybinds are in `keybinds.kdl`, the old `Mod+Shift+S` screenshot line is gone, and
      `Print` still takes a region screenshot
      ([D-12](artifacts/implementation-decision-log.md#d-12-two-keybinds-through-systemd-cat)).
- [ ] The two settling constants come from the work unit 1 measurement, and the measurement is
      recorded in the decision log ([D-9](artifacts/implementation-decision-log.md#d-9-settling-period-values-and-rules)).
- [ ] Nothing in `## Deferred (YAGNI)` was built, or its `Reopen when:` trigger fired and is
      recorded.

## Testing Strategy

- **Observable behaviors to test:** One press stashes every floating window on the focused
  workspace and the next restores them within 1 px. Alt-tab through "no window focused" onto a
  different sliver restores only that sliver. A direct focus change, or a return to the same
  window, does nothing. A hand move pops the window back at its original size; a hand resize forgets
  it. A daemon restart re-applies the saved desired state.
- **Edge cases requiring coverage:** Edge ties (left, right, top, bottom order). A window whose
  application refuses 32 px (it settles larger and the slivers show up to 75 px). A window that
  closes or is tiled mid-press. A state file from another niri instance. An unwritable state file at
  press start (`err state-unwritable`, nothing moves). A second daemon starting (exit 78).
- **Test doubles posture and levels:** Unit tests on the pure core only, with no niri connection:
  geometry, focus detection, move-versus-resize classification, and re-apply decisions take plain
  values and return plain values. Integration is the manual checklist on the live session, read
  through `niri msg -j windows` and `journalctl --user -t niri-stash`. A nested-niri end-to-end
  harness is deferred.

- **Replay fixtures:** six real event streams in `artifacts/` (`niri-event-log-2026-10-11.jsonl`
  and `-1` to `-5`). Expected readings: the base log has no alt-tab, one hand resize (lines 21–96),
  one hand move landing at (573.1, 413.1); log 2 has one alt-tab (72 → none → 73) and two returns to
  the same window; log 3 has two keybind workspace moves that keep the entry and one tile-float round
  trip that triggers the re-float restore; log 4 has a lock-unlock return and an overview close that
  lands on a different window; log 1 has a scripted 32 px stash settling at x = -22.3.

## On-Call Resilience Posture

The on-call engineer's review shaped these. Each one bounds how long something can hang, or makes
a failure visible in the journal.

- **Timeouts:** niri request connections 1 s read and write
  ([D-6](artifacts/implementation-decision-log.md#d-6-thin-niri-client-with-timeouts)). Each
  accepted hotkey connection 500 ms read; 4 s for the state thread's reply, then `err busy-timeout`.
  Client 5 s read ([D-11](artifacts/implementation-decision-log.md#d-11-hotkey-client-timeouts-and-exit-codes)).
  Settling capped at 1 s per press. Startup: 60 s to reach niri, 5 s for the first snapshot
  ([D-14](artifacts/implementation-decision-log.md#d-14-startup-sequence-and-stale-socket-handling)).
- **Retries:** Only the startup connection retries, with doubling backoff. A niri request that
  fails mid-press is not retried: the press answers `err niri-unavailable`, and the event stream's
  end then exits the daemon with 75 so systemd restarts it.
- **Isolation of the event reader:** niri disconnects a stream client that falls 64 events behind,
  so the reader thread only parses and forwards into an unbounded queue and never waits on the
  state thread ([D-5](artifacts/implementation-decision-log.md#d-5-three-std-threads-and-one-channel)).
- **Idempotency and data integrity:** The desired state is written atomically before any command.
  Re-applying it at startup is safe to repeat, because stashing an existing sliver changes nothing
  visible. The instance tag keeps a previous niri's window ids from being reused
  ([D-13](artifacts/implementation-decision-log.md#d-13-desired-state-file-format-and-write-points)).
- **Degradation:** A window that vanishes mid-press is skipped and the rest of the press completes
  ([D-17](artifacts/implementation-decision-log.md#d-17-vanished-and-tiled-windows-mid-press)).
  A failed state write after a press has started is logged and the daemon keeps running.
- **Kill switch:** `systemctl --user stop niri-stash`. Slivers stay where they are and the state file
  is kept; starting the service again re-applies it.
- **Failure-path observability:** Every exit carries a code from the contract and a log line with
  its reason ([D-15](artifacts/implementation-decision-log.md#d-15-daemon-exit-codes-and-event-stream-end)).
  Client errors reach the journal through `systemd-cat`
  ([D-12](artifacts/implementation-decision-log.md#d-12-two-keybinds-through-systemd-cat)). Every
  forgotten original is logged with its geometry, so a window lost to a bug can be restored by hand
  ([D-18](artifacts/implementation-decision-log.md#d-18-logging-content-and-correlation)).

## Risks and Assumptions

### Risks

| ID  | Risk | Impact | Mitigation | Owner |
| --- | ---- | ------ | ---------- | ----- |
| R1  | The settling values are wrong for a real application. | Too short: a late resize answer reads as a hand resize and the original is forgotten. Too long: presses feel slow. | Work unit 1 measures WezTerm and gnome-calculator before the values are fixed; every settle is logged at debug and a cap hit at warn ([D-9](artifacts/implementation-decision-log.md#d-9-settling-period-values-and-rules)). | User (maintainer) |
| R2  | A restore whose resize lands before its move computes the nudge from the wrong base, because niri adds the nudge to the position it shows and that position jumps when the size changes. | The window comes back at the right size in the wrong place. | The restore sends the move before the resize ([D-7](artifacts/implementation-decision-log.md#d-7-relative-moves-and-one-phase-stash-and-restore)); work unit 2 checks restored geometry within 1 px. | User (maintainer) |
| R3  | Without the working area, the daemon cannot compute a top-edge sliver's exact target, so the "already at target" shortcut may not fire for top slivers. | Startup re-apply of a top sliver waits for the 1 s cap; nothing visible changes. | Accepted; the cap bounds it. | User (maintainer) |
| R4  | A crash between the state write for a pop-back or forget and the niri command leaves a window the daemon no longer tracks. | One window needs resizing by hand. | The discarded original geometry is logged at info before the command ([D-18](artifacts/implementation-decision-log.md#d-18-logging-content-and-correlation)). | User (maintainer) |

### Assumptions

| ID  | Assumption | What Changes If Wrong | Status |
| --- | ---------- | --------------------- | ------ |
| A1  | 150 ms quiet and a 1 s cap fit how WezTerm and gnome-calculator answer a resize. | The constants change after the work unit 1 measurement. | Runtime-only; the user reports resizes answer quickly with one layout report per frame, so the risk is a too-long rather than a too-short wait |
| A9  | A hand drag of a floating window produces a window-changed report with no workspace and no position at pick-up, and one with the workspace and the new position at drop; a hand resize produces one layout report per frame with the position fixed and no lift-and-drop pair; a keybind workspace move is reported once on the new workspace at the same position; on close, the focus change to the next window precedes the close report. | If a niri upgrade reports drags differently, the lift-and-drop rule degrades to the 150 ms debounce, which still judges the final geometry. | Verified by the user's event logs in `artifacts/` (`niri-event-log-2026-10-11.jsonl` lines 97–100 drag, 21–96 resize, 101–103 close; `-3.jsonl` keybind move and tile-float round trip) |
| A10 | Scripted resizes and moves are reported at once (one or two reports), a no-op request is not reported, and niri's clamp leaves `clamp(size/4, 10, 75)` px visible. | Settling would need per-frame handling for scripted changes too, and the sliver arithmetic would change. | Verified by `artifacts/niri-event-log-2026-10-11-1.jsonl`: 32 px at x = -22.3; 900 px at the right edge with 73.7 px visible; no report for a push against an edge already reached |
| A11 | niri's built-in alt-tab switcher, the lock screen, and the overview all report "no window focused" while open, and cancelling or unlocking returns focus to the same window. | The same-window rule would need another signal. | Verified by `artifacts/niri-event-log-2026-10-11-2.jsonl`, `-4.jsonl`, `-5.jsonl` |
| A2  | The noctalia bar stays on the top edge at 28 px. | The edge-choice bias moves to another edge, and top-sliver positions shift. A recurring surprise reopens the deferred `--insets` flag. | Open |
| A3  | After a niri restart, the restarted daemon sees the new `NIRI_SOCKET`. | The daemon would keep connecting to a dead socket and exit 69 in a loop. | Verified: niri imports its environment before signalling ready (niri v26.04 `src/main.rs:217,238`); `niri.service` is `Before=graphical-session.target` and the daemon unit is `After=`/`PartOf=` it. |
| A4  | niri actions never fail for a window id that no longer exists. | The vanished-window path would need to read action replies. | Verified: niri v26.04 `src/ipc/server.rs:381-399`, `src/input/mod.rs` ~1986-1992. |
| A5  | The working area cannot be read over niri's IPC. | Edge choice could measure from the working area directly. | Verified: `niri msg layers` reports namespace and layer but no geometry or exclusive zone (discovery notes). |
| A6  | `AdjustFixed` adds to the position niri currently reports, measured from the output. | The relative-move rule would need a different base. | Verified: niri v26.04 `src/layout/floating.rs` `FloatingSpace::move_window` starts from `data.logical_pos` and adds the delta. |
| A7  | `NIRI_SOCKET` and `XDG_RUNTIME_DIR` are visible to user services. | The unit would need a wrapper to pass them. | Verified: `systemctl --user show-environment` (discovery notes). |
| A8  | `$XDG_RUNTIME_DIR` is tmpfs. | A durable disk would make fsync worth reconsidering. | Verified: `findmnt /run/user/1000` reports tmpfs. |

## Deferred (YAGNI)

This is work no evidence supports yet, not work the work item excludes. Every entry carries the trigger that would justify
revisiting it.

### Async runtime (tokio, mio, or calloop)

- **Why deferred:** Simpler-version test. Three std threads and one channel cover one event stream
  and one socket; `recv_timeout` is the only timer needed.
- **Reopen when:** The daemon has to wait on more than one source per thread, or thread count grows
  past what one queue can serve.
- **Source:** R1 C1, on-call-engineer (OCE-1, OCE-3) and junior-developer (JD-10).

### Command-line parser crate

- **Why deferred:** Simpler-version test. Three verbs and no options parse by hand.
- **Reopen when:** A subcommand gains its first option or flag.
- **Source:** R1 C16, junior-developer (JD-4).

### Runtime configuration for settling and timeout values

- **Why deferred:** Evidence test; a configuration knob no caller sets.
- **Reopen when:** Two applications in daily use need different settling values.
- **Source:** R1 C21, on-call-engineer (OCE-21).

### Metrics, dashboards, alerts, and a health endpoint

- **Why deferred:** Evidence test; observability for failures that have not happened, on a
  single-user laptop.
- **Reopen when:** Stranded slivers recur and the journal cannot explain them.
- **Source:** R1 C21, on-call-engineer (OCE-20).

### Kill switch or feature flag

- **Why deferred:** Simpler-version test. `systemctl --user stop niri-stash` stops it; slivers stay
  and the state is kept.
- **Reopen when:** The daemon needs to keep running with stashing disabled.
- **Source:** R1 C21, on-call-engineer (OCE-22).

### State-file version field, schema, and lock file

- **Why deferred:** Evidence test. One process writes and reads the file.
- **Reopen when:** A second process reads the file, or a format change has to survive a rebuild
  while windows are stashed.
- **Source:** R1 C12, junior-developer (JD-12).

### Install script or Makefile

- **Why deferred:** Simpler-version test. Three commands in the README.
- **Reopen when:** The install grows past three commands or a second machine needs it.
- **Source:** R1 C19, junior-developer (JD-6).

### `--insets` flag for the working area

- **Why deferred:** Evidence test. One bar, fixed on top at 28 px.
- **Reopen when:** A window near the bar goes to a side edge when you expected the top, more than
  once.
- **Source:** R1 C22, junior-developer (JD-1).

### Nested-niri end-to-end tests

- **Why deferred:** Evidence test. No regression has escaped yet.
- **Reopen when:** A bug escapes the pure-core unit tests and the manual checklist.
- **Source:** R1 C25, junior-developer (JD-11).

### `thiserror`

- **Why deferred:** Evidence test. One error consumer (`anyhow` at the top level, error codes
  mapped by hand).
- **Reopen when:** A second consumer of typed errors appears.
- **Source:** R1 C1, on-call-engineer (OCE-3).

### fsync on the state file

- **Why deferred:** Evidence test. The file lives on tmpfs, and a kernel crash ends the niri
  instance whose window ids the file refers to.
- **Reopen when:** The state file moves off tmpfs.
- **Source:** R1 C12, on-call-engineer (OCE-13).

### Jitter in the startup backoff

- **Why deferred:** Evidence test. One client retries against one niri socket.
- **Reopen when:** More than one process retries against the same socket at login.
- **Source:** R1 C5, on-call-engineer (OCE-6).

## Open Items

None. The specification's OI-1 (the hotkey client ⇄ daemon contract) is pinned in
[Hotkey socket contract](#hotkey-socket-contract) and [Keybinds](#keybinds).

## Sources and Plan Records

- **Feature specification:** [feature-specification.md](feature-specification.md)
- **Specification companions:** [decision log](artifacts/decision-log.md),
  [team findings](artifacts/team-findings.md),
  [technical notes](artifacts/feature-technical-notes.md),
  [review findings](artifacts/review-findings.md),
  [review iteration history](artifacts/review-iteration-history.md),
  [scope boundary](artifacts/scope-boundary.md),
  [discovery notes](artifacts/.discovery-notes.md)
- **Specification decisions inherited / open items to respect:** D1–D16 and committed mechanics
  T1–T5 / OI-1, resolved by this plan in [Hotkey socket contract](#hotkey-socket-contract).
- **Specification wording that predates this plan** (behavior unchanged; a light spec touch-up
  would bring the text in line):
  - Out of Scope says the niri configuration "gains one keybind and nothing is removed from it", and
    the scope boundary's Direction of Travel says the keybind config stays as it is. After E2 it
    gains `Mod+S` and its `Mod+Shift+S` screenshot line is replaced.
  - D3 measures distance from the usable area. This plan measures from the output edges, per the
    work item's "edge of the monitor"; the sliver still lands against the usable area, and the two
    rules differ only within 28 px of a tie
    ([D-8](artifacts/implementation-decision-log.md#d-8-edge-distance-measured-from-the-output-edges)).
  - D11 and the "niri restarts" flow say the daemon "discards its saved state" when the stream
    closes. This plan leaves the file in place and lets the instance tag discard it at the next
    start, which looks the same to the user
    ([D-15](artifacts/implementation-decision-log.md#d-15-daemon-exit-codes-and-event-stream-end)).
- **Decision rationale and rejected alternatives:** [artifacts/implementation-decision-log.md](artifacts/implementation-decision-log.md)
- **Team composition and round-by-round history:** [artifacts/implementation-iteration-history.md](artifacts/implementation-iteration-history.md)

## Recommendation

Ship as planned. Every contract is pinned, every niri mechanic the plan depends on is verified in
the v26.04 source, and the one runtime unknown, the settling values, is measured in work unit 1
before anything depends on it.
