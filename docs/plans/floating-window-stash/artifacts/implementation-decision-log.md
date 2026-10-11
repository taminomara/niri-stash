# Implementation Decision Log: Floating Window Stash for niri

<!--
This file records every implementation decision committed while planning the floating window stash.
Behavioral and implementation statements live in [../feature-implementation-plan.md](../feature-implementation-plan.md) —
this file captures the question, rationale, evidence, and rejected alternatives for each decision.
Round-by-round history lives in [implementation-iteration-history.md](implementation-iteration-history.md).

## Two-tier format: full vs. trivial decisions

A decision is **full** when it has a rejected alternative, rests on evidence beyond the user's
framing, was driven or changed by a round, has a dependent decision, or carries dissent. Otherwise
it is **trivial**. If unsure, full.

Cross-referencing invariants:
- `Driven by rounds:` — R# IDs from implementation-iteration-history.md that added or changed this decision.
- `Dependent decisions:` — D# IDs of later decisions that rest on this one.
- `Referenced in plan:` — sections of ../feature-implementation-plan.md that cite this decision.

Spec decisions D1–D16 and committed mechanics T1–T5 live in [decision-log.md](decision-log.md) and
[feature-technical-notes.md](feature-technical-notes.md). Claim IDs C1–C28 refer to the R1 claim
ledger in implementation-iteration-history.md. niri source citations refer to the v26.04 tag,
matching the installed `niri 26.04 (8ed0da4)`.
-->

## Trivial decisions

- D-2: Licence — the project is `GPL-3.0-or-later`, because it links `niri-ipc`, which is
  GPL-3.0-or-later (crate metadata; R1 C27). — Referenced in plan: Implementation Approach >
  Project and packaging.
- D-4: Install location — `cargo install --path .` puts the binary at `~/.cargo/bin/niri-stash`,
  already on the user's `PATH` and the default for cargo. — Referenced in plan: Implementation
  Approach > Project and packaging.
- D-19: Default log level — `info`, overridable with `RUST_LOG` (the `env_logger` convention). —
  Referenced in plan: Logging.

## Full decisions

### D-1: Single binary with three subcommands

- **Question:** How are the daemon and the hotkey client packaged, and how are arguments parsed?
- **Decision:** A new Cargo project at `~/p/niri-stash`, git-initialised on branch `main`, building
  one binary `niri-stash` with subcommands `daemon`, `toggle`, and `unstash`. Arguments are parsed
  by hand from `std::env::args`; anything else is a usage error (client exit 64, see D-11).
- **Rationale:** The client and daemon share the socket path and the protocol, so one binary keeps
  them from drifting. Three verbs with no options do not need a parser crate.
- **Evidence:** Work item: "Daemon should be implemented in rust, you can set up a project in ~/p"
  (scope-boundary.md). R1 C16 (OCE-17, JD-4). YAGNI sweep row "Command-line parser crate". Discovery
  notes: no `Cargo.toml` or git repository exists yet; global `init.defaultBranch=main`.
- **Rejected alternatives:**
  - Two binaries (daemon and client) — rejected because they would duplicate the socket path and
    protocol constants and could be installed out of step.
  - `clap` for argument parsing — rejected under the simpler-version test: three verbs, no options
    (deferred; see the plan's Deferred section).
- **Specialist owner:** The user (maintainer).
- **Revisit criterion:** A subcommand gains its first option or flag.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** D-10, D-11, D-12, D-16, D-21
- **Referenced in plan:** Project and packaging

### D-3: Dependencies and an exact niri-ipc pin

- **Question:** Which crates does the project depend on, and how tightly is the niri crate pinned?
- **Decision:** `niri-ipc = "=26.4.0"` (exact), `serde` and `serde_json`, `log` and `env_logger`,
  `anyhow`. `panic = "abort"` in both the dev and release profiles. Nothing else.
- **Rationale:** `niri-ipc` provides the request, reply, and event types for the installed niri
  release. A caret requirement could pull a newer crate whose event enum no longer matches the
  running niri. `panic = "abort"` turns a panic in any thread into a whole-process exit that systemd
  restarts, instead of a live process with a dead worker.
- **Evidence:** R1 C11 (OCE-12): niri-ipc `lib.rs:35-44` describes the crate as tracking niri
  releases. R1 C3 (OCE-4): std thread semantics (a panicking thread ends alone unless the panic
  aborts). Discovery notes: niri-ipc 26.4.0 matches installed niri 26.04; all listed crates are in
  the local registry cache.
- **Rejected alternatives:**
  - `niri-ipc = "26.4.0"` (caret) — rejected because a minor bump can add event variants that the
    pinned niri never sends and that the crate's own version may parse differently.
  - `thiserror` for typed errors — rejected under the evidence test: one error consumer (deferred).
  - `tracing` instead of `log` — rejected because the daemon has no spans to correlate beyond a
    press number, which a log field carries.
- **Specialist owner:** The user (maintainer); advice from on-call-engineer.
- **Revisit criterion:** niri on this machine is upgraded (move the pin with it), or a second
  consumer of typed errors appears.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** D-5, D-6, D-18
- **Referenced in plan:** Project and packaging

### D-5: Three std threads and one channel

- **Question:** How is the daemon structured to read events, accept hotkey presses, and act on both
  without races?
- **Decision:** Three std threads. The event-reader thread reads lines from niri's event stream,
  parses them, and forwards each to an unbounded `std::sync::mpsc` channel; it does nothing else.
  The acceptor thread accepts one hotkey connection at a time and forwards the request together with
  a reply sender. The state thread owns all state, the niri request client, and the state file, and
  times settling deadlines with `recv_timeout`. No async runtime. If the state thread's channel
  reports `Disconnected`, the daemon exits 70.
- **Rationale:** One thread processing events and requests in order is what T3 requires ("Commands
  and events are processed on one sequential path"). The reader must never block, because niri
  drops a stream client that falls behind, and a dropped stream ends the daemon.
- **Evidence:** R1 C1 (OCE-1, OCE-3, JD-10): niri-ipc 26.4.0 `socket.rs` offers a blocking API only.
  R1 C2 (OCE-2): niri v26.04 `src/ipc/server.rs:42` `EVENT_STREAM_BUFFER_SIZE = 64`, with the
  disconnect at lines 119-134 (verified). T3. R1 C3 (OCE-4) for the fatal `Disconnected`.
- **Rejected alternatives:**
  - tokio, mio, or calloop — rejected under the simpler-version test: one stream, one socket, and
    one timer need no runtime (deferred).
  - Reading events on the state thread — rejected because a press waiting on settling would stop
    reading, and 64 unread events disconnect the stream.
  - A bounded channel — rejected because a full channel would block the reader, which is the
    failure the separate thread exists to prevent; the state thread's longest stall is one press
    (about 1.3 s), so growth is bounded in practice.
- **Specialist owner:** The user (maintainer); advice from on-call-engineer.
- **Revisit criterion:** The daemon needs to wait on a third input source, or the journal shows the
  stream being dropped for lag.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** D-9, D-10, D-15
- **Referenced in plan:** Implementation Approach; On-Call Resilience Posture

### D-6: Thin niri client with timeouts

- **Question:** How does the daemon send requests to niri without hanging if niri stops answering?
- **Decision:** A small client over the daemon's own `UnixStream` to `$NIRI_SOCKET`, serialising
  `niri_ipc::Request` and parsing `niri_ipc::Reply` with `serde_json`. `set_read_timeout` and
  `set_write_timeout` are 1 s. One connection per press (snapshot, actions, and the end-of-press
  re-fetch share it); it is dropped on any error. An `Err` from the socket is a transport failure
  (niri gone) and follows the spec D11 path. `Ok(Err(message))` is logged as "niri declined" and the
  window is skipped (T5).
- **Rationale:** `niri_ipc::Socket` exposes no timeouts, and a hung request would stall the state
  thread and every press after it. niri never reports an action as failed for a vanished window, so
  socket errors can be read as "niri is gone" without ambiguity.
- **Evidence:** R1 C4 (OCE-5): niri-ipc 26.4.0 `socket.rs:18-61`. R1 C8: niri v26.04
  `src/ipc/server.rs:381-399` (`Request::Action` → `Response::Handled`), `src/input/mod.rs`
  ~1986-1992 (silent no-op for a missing id), verified. T5.
- **Rejected alternatives:**
  - `niri_ipc::Socket` as provided — rejected because it offers no way to bound a read or write.
  - One long-lived request connection — rejected because a connection left in an unknown state
    after an error would poison every later press; a fresh one per press costs one `connect`.
- **Specialist owner:** The user (maintainer); advice from on-call-engineer.
- **Revisit criterion:** A niri request legitimately takes longer than 1 s, shown by spurious
  `err niri-unavailable` replies while niri is healthy.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** D-7, D-14, D-17
- **Referenced in plan:** Talking to niri; On-Call Resilience Posture

### D-7: Relative moves and one-phase stash and restore

- **Question:** Which niri commands stash and restore a window, in what order, and in one round or
  two?
- **Decision:** Every position change is `move-floating-window` with
  `PositionChange::AdjustFixed(delta)` computed from the last output-relative position niri
  reported; `SetFixed` is never used. Stash: resize to 32 px on the edge axis (width for left and
  right, height for top and bottom), then `AdjustFixed(±10000)` on that axis, sent back to back in
  one phase. Restore: `AdjustFixed(original − current)` on both axes, then resize to the original
  width and height, sent back to back in one phase; the move goes first. A hand-move pop-back is a
  resize only (it keeps the position where the user dropped the window).
- **Rationale:** niri reports positions relative to the output but places `SetFixed` relative to the
  working area; the two differ by the bar height on y. `AdjustFixed` adds to the reported position,
  so it needs no knowledge of the working area. One phase is enough for a stash because niri stores
  the requested position unclamped and re-clamps it whenever the size changes, so the sliver lands
  wherever the application's answer leaves it. The restore move must go first because `AdjustFixed`
  adds to the currently clamped position: if the application answered the resize before niri
  processed the move, the clamped position would have jumped (from about −22 to about −725 for an
  800 px window on the left edge) and the delta would be wrong. Moving first, while the window is
  still 32 px, makes the result independent of when the application answers.
- **Evidence:** R1 C23 (JD-2): niri v26.04 `src/layout/floating.rs` `FloatingSpace::move_window`
  (~line 954): `SetFixed(x) => pos.x = x + working_area_loc.x`, `AdjustFixed(x) => pos.x += x`,
  starting from `self.data[idx].logical_pos`. R1 C26 disposition: `Data::set_logical_pos` stores the
  unclamped fraction; `Data::update` re-clamps on every size change (lines 170-185). The restore
  ordering was settled at synthesis from the same lines (the base of `AdjustFixed` is the clamped
  `logical_pos`, recomputed in `Data::update`). Measurement: left sliver reports x ≈ −22
  (`clamp(32/4, 10, 75) = 10` visible).
- **Rejected alternatives:**
  - `SetFixed` to an absolute target — rejected because the working-area origin is not readable over
    IPC (A5), so the target would be off by the bar height.
  - Two phases (resize, wait for the answer, then push) — rejected because niri re-clamps the stored
    position on every size change, so waiting adds latency without changing the result.
  - Restore with resize first, then move — rejected because the result would depend on whether the
    application answered the resize before niri processed the move.
- **Specialist owner:** The user (maintainer).
- **Revisit criterion:** A niri upgrade changes `move_window` or the clamp in `Data::update`, or
  work unit 2 sees restored geometry off by more than 1 px.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** D-9, D-20
- **Referenced in plan:** How a window is stashed and restored; Risks and
  Assumptions

### D-8: Edge distance measured from the output edges

- **Question:** The working area is not readable over IPC; from what does the daemon measure the
  distance between a window's centre and each edge?
- **Decision:** From the output's logical edges (from `Outputs` and the window's reported
  output-relative geometry). Ties resolve left, right, top, bottom (spec D3). The sliver itself
  still lands against the working area, because niri's clamp does that. No `--insets` flag.
- **Rationale:** The work item says "the exact edge of the monitor where the window belongs is the
  closest to the window's center". Spec D3's "usable area" wording came from where niri places the
  sliver, which is unchanged. Measuring from the output differs from measuring from the working area
  only for a window whose centre is within the bar's 28 px of a tie with the top edge.
- **Evidence:** Work item, scope-boundary.md Stated Scope. R1 C22 (JD-1) and OQ-1 resolution.
  Discovery notes: `niri msg layers` has no geometry; bar `thickness = 28` in
  `~/.config/noctalia/config.toml`. T1.
- **Rejected alternatives:**
  - An `--insets` flag giving the bar size — rejected under the evidence test: one bar, fixed on top
    (deferred).
  - Reading noctalia's config file — rejected because it couples the daemon to another program's
    private file format for a ≤ 28 px difference in tie cases.
- **Specialist owner:** The user (maintainer).
- **Revisit criterion:** A window near the bar goes to a side edge when the top was expected, more
  than once; or the bar moves off the top edge (assumption A2).
- **Dissent (if any):** None. This revises the measurement in spec D3 for near-ties only; the plan's
  Sources section lists it as spec wording to touch up.
- **Driven by rounds:** R1
- **Dependent decisions:** D-20
- **Referenced in plan:** How a window is stashed and restored; Sources and
  Plan Records

### D-9: Settling period values and rules

- **Question:** How long does the daemon treat a window's layout changes as its own after sending
  requests, and how are focus requests it makes kept out of alt-tab detection?
- **Decision:** `SETTLE_QUIET_MS = 150` and `SETTLE_CAP_MS = 1000`, as code constants. The quiet
  timer restarts on every layout report for the window, of any kind; a window that produces no
  report waits to the cap. (Build note, work unit 1: the draft rule started the timer only on a size
  change; measured against gnome-calculator, which refuses to shrink, that made every press take
  the full cap, so the rule was widened. niri answers scripted resizes within a frame, log 1.) Settling is skipped when the snapshot geometry already
  equals the target within 1 logical px. All windows of one press settle in parallel. At the cap the
  daemon adopts the latest reported geometry and logs a warning. Every settle is logged at debug
  with its elapsed milliseconds. Expected focus changes the daemon requests (spec D8, T2) expire
  when the press's settling ends. A hand drag is recognised from niri's own reports: a
  `WindowOpenedOrChanged` with `workspace_id: None` and no position means the window is lifted
  (keep the entry, do nothing), and the next report with a workspace and a position is the drop,
  which is the geometry the hand-move or hand-resize decision is made on. A keybind workspace move
  is not a lift: it is reported once on the new workspace at the same position (log 3). A hand
  resize is not bracketed that way either: it arrives as a per-frame stream of `WindowLayoutsChanged` with the position
  fixed, so it, and any other unexpected change outside a settling period, is debounced by
  `SETTLE_QUIET_MS` and classified once at the end of the stream. A `WindowOpenedOrChanged` for the window that already has focus is not a focus
  transition. Work unit 1 confirms both constants against a real WezTerm and gnome-calculator
  resize; the measured figures are recorded in this entry.
- **Rationale:** T3 commits the behavior and leaves the values to planning, asking for a check on a
  real terminal. Starting the quiet timer before the application has answered would end settling
  early and turn the answer into a "manual resize". Expiring expected focus at settle end means a
  `focus-tiling` that niri ignores (no tiled windows) cannot swallow a later real focus change.
- **Evidence:** R1 C6 (OCE-7, JD-7, JD-8), Unverified: resize latency was not reachable with granted
  tools, hence the measurement step. T3. R1 C15 (OCE-16, JD-7): niri v26.04
  `Workspace::switch_focus_floating_tiling` is a no-op on an empty tiling layout (discovery notes).
  User's event log (2026-10-11, after synthesis): a resize produces per-frame `WindowLayoutsChanged`
  with `tile_size` and `window_size` stepping each frame (e.g. 636 → 634.86 → 634.29); a drag
  produces `WindowOpenedOrChanged { workspace_id: None, tile_pos_in_workspace_view: None,
  is_focused: true }` at pick-up and `WindowOpenedOrChanged { workspace_id: Some(1),
  tile_pos_in_workspace_view: Some(..) }` at drop, bracketed by `WorkspaceActiveWindowChanged`
  events; a hand corner-resize is ~75 per-frame `WindowLayoutsChanged` with the position fixed and
  no lift-and-drop pair; a close emits `WindowFocusChanged` to the next window before `WindowClosed`.
  The full log is kept at `artifacts/niri-event-log-2026-10-11.jsonl` and doubles as a unit-test
  fixture (D-20). Five further logs (`-1` to `-5`, same folder) add: a scripted `set-window-width 32`
  and `move-floating-window` are reported once or twice with no animation, a no-op push is not
  reported, and the clamp leaves 10 px at 32 px wide and 73.7 px at 900 px (log 1); niri's built-in
  switcher, the lock screen and the overview report focus `None` and return to the same window on
  cancel or unlock (logs 2, 4, 5); a keybind workspace move is one report on the new workspace at the
  same position, and a tile-float round trip restores the old floating geometry (log 3). This
  supersedes the earlier Unverified label on resize latency; the constants stand, and work unit 1
  only confirms them.
- **Rejected alternatives:**
  - Runtime configuration for the values — rejected under the evidence test: no caller sets them
    (deferred).
  - A fixed wait after every request — rejected because it makes every press as slow as the slowest
    application.
  - Settling windows one after another — rejected because press time would grow with the window
    count.
- **Specialist owner:** The user (maintainer); advice from on-call-engineer.
- **Revisit criterion:** The work unit 1 measurement falls outside 150 ms quiet or 1 s total, or
  after ship the journal shows cap warnings or late-resize forgets more than occasionally.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** D-11, D-17
- **Referenced in plan:** Constraints and Boundaries; How a window is
  stashed and restored; Work Units and Sequencing; Definition of Done; Risks and Assumptions

### D-10: Hotkey socket protocol

- **Question:** What exactly do the hotkey client and the daemon exchange, and where? (Resolves spec
  OI-1.)
- **Decision:**
  - Socket: `$XDG_RUNTIME_DIR/niri-stash/sock`, mode `0600`, in a directory the daemon creates.
  - Request grammar: `request = ("toggle" / "unstash") LF`. The client writes it and shuts down its
    write half.
  - Reply grammar: `reply = ("ok" / "err" SP code SP message) LF`;
    `code = "niri-unavailable" / "state-unwritable" / "busy-timeout" / "unknown-command"`;
    `message` is printable UTF-8 with no LF.
  - Worked example: `C→D: toggle\n` then `D→C: ok\n`. Failure:
    `D→C: err state-unwritable rename /run/user/1000/niri-stash/state.json: Read-only file system\n`.
  - Skipped windows and no-op presses answer `ok`.
  - Daemon side: `set_read_timeout(500 ms)` on each accepted stream; the acceptor waits up to 4 s
    for the state thread's reply, then answers `err busy-timeout`.
- **Rationale:** A line-based text protocol can be tested with `socat` and needs no shared library.
  Codes give the client a stable thing to report; the message carries the detail for the journal.
  The 4 s busy-timeout covers the worst case of one in-flight press plus this one, each bounded by
  the settle cap plus request latency (about 1.3 s).
- **Evidence:** Spec OI-1 and D12. R1 C16 (OCE-17, JD-4), C7 (OCE-8, JD-4) and the OQ-3 resolution.
  Spec edge-case rows: closed window mid-press and no floating windows both exit successfully.
- **Rejected alternatives:**
  - JSON request and reply — rejected because two verbs and four error codes need no structure, and
    a text line is readable in the journal as is.
  - Exit status only, no reply line (connection close means success) — rejected because the client
    could not tell "state unwritable" from "daemon crashed mid-press".
- **Specialist owner:** The user (maintainer).
- **Revisit criterion:** A third request verb (for example the deferred status command) is added.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** D-11, D-12
- **Referenced in plan:** Hotkey socket contract; Definition of Done

### D-11: Hotkey client timeouts and exit codes

- **Question:** How long does the client wait, and what exit code does it return in each case?
- **Decision:** Client read timeout 5 s. Exit codes: 0 reply `ok`; 1 could not connect (`ENOENT` or
  `ECONNREFUSED`); 2 reply `err` (code and message echoed to stderr); 3 no reply within 5 s; 64
  usage error.
- **Rationale:** The client's bound must exceed the daemon's 4 s busy-timeout so that when the
  daemon is busy the client reports the daemon's reason (`err busy-timeout`, exit 2) rather than its
  own guess. Separate codes for "not running" and "answered with an error" tell the user where to
  look. 64 follows `sysexits.h` `EX_USAGE`.
- **Evidence:** R1 C7: OCE-8 proposed 3 s, JD-4 proposed 10 s; OQ-3 resolved by the 4 s busy-timeout
  derivation. R1 C16. Spec edge-case row "daemon is not running or not reachable".
- **Rejected alternatives:**
  - 3 s client timeout (OCE-8) — rejected because it is shorter than a worst-case queued press, so
    the client would give up while the daemon was still working.
  - 10 s client timeout (JD-4) — rejected because nothing in the daemon can take that long; it only
    delays the error.
  - A single non-zero exit code for every failure — rejected because the journal line alone would
    have to tell "daemon down" from "daemon refused".
- **Specialist owner:** The user (maintainer); advice from on-call-engineer.
- **Revisit criterion:** The settle cap changes after the work unit 1 measurement (re-derive the
  4 s / 5 s pair from it).
- **Dissent (if any):** None; the disputed value (C7) was settled by the derivation above.
- **Driven by rounds:** R1
- **Dependent decisions:** D-12
- **Referenced in plan:** Hotkey socket contract; Definition of Done;
  On-Call Resilience Posture

### D-12: Two keybinds through systemd-cat

- **Question:** Which keys run the client, and how do its errors reach the journal?
- **Decision:** In `~/.config/niri/cfg/keybinds.kdl`, add to the Windows section:
  `Mod+S hotkey-overlay-title="Stash floating windows" { spawn "systemd-cat" "-t" "niri-stash" "niri-stash" "toggle"; }`
  and replace the existing `Mod+Shift+S` region-screenshot line, where it stands near the top of the
  file, with:
  `Mod+Shift+S hotkey-overlay-title="Unstash floating windows" { spawn "systemd-cat" "-t" "niri-stash" "niri-stash" "unstash"; }`.
  Fn+F11 sends Super+Shift+S, so Fn+F11 becomes unstash; `Print` keeps the region screenshot. The
  comment above that line that describes Fn+F11 as the snipping chord is updated to say it unstashes.
- **Rationale:** The user chose the keys. niri spawns keybind commands with stderr set to
  `/dev/null`, so without `systemd-cat` the client's error message (D-11, exit 2) would be lost.
- **Evidence:** User input, escalation E2 and its follow-up ("Rebind Mod+Shift+S to unstash").
  R1 C17 (JD-5): niri v26.04 `src/utils/spawning.rs:118-120` (`.stderr(Stdio::null())`), verified.
  `keybinds.kdl` line 25 holds the `Mod+Shift+S` screenshot bind and line 152 the `Print` one; line 23
  documents Fn+F11 as Super+Shift+S.
- **Rejected alternatives:**
  - A free key such as `Mod+G` for unstash, keeping the screenshot bind — rejected by the user.
  - Running the client directly without `systemd-cat` — rejected because its stderr would go to
    `/dev/null`.
  - The client writing to the journal itself — rejected because it adds a dependency for what one
    wrapper command already does.
- **Specialist owner:** The user.
- **Revisit criterion:** The user wants the screenshot back on `Mod+Shift+S`, or niri starts passing
  spawned stderr to its own log.
- **Dissent (if any):** None. The work item does not mention screenshots; the replacement is kept
  by the user's explicit direction, recorded as its justification.
- **Driven by rounds:** R1
- **Dependent decisions:** —
- **Referenced in plan:** Keybinds; Definition of Done; On-Call Resilience
  Posture

### D-13: Desired-state file format and write points

- **Question:** What does the state file hold, when is it written, and what happens when a write
  fails?
- **Decision:** `$XDG_RUNTIME_DIR/niri-stash/state.json`, field layout:
  `{"instance": "<$NIRI_SOCKET>", "windows": {"<id>": {"original": {"x": <f64>, "y": <f64>, "w": <f64>, "h": <f64>}, "desired": "stashed" | "restored", "edge": "left" | "right" | "top" | "bottom" | null}}}`.
  `<id>` is the niri window id as a decimal string; geometry is output-relative logical pixels;
  `edge` is `null` when `desired` is `"restored"`. Worked example:
  `{"instance":"/run/user/1000/niri.wayland-1.1572.sock","windows":{"12":{"original":{"x":540.0,"y":300.0,"w":800.0,"h":500.0},"desired":"stashed","edge":"left"}}}`.
  Written atomically (write `state.json.tmp`, rename), no fsync, no version field, no lock. Written
  before the first niri command of any state-changing operation (press, alt-tab unstash, pop-back)
  and when an entry is dropped. If the write at press start fails, nothing moves and the reply is
  `err state-unwritable <reason>`. A later failed write is logged at error and the daemon continues.
  At startup: discard the file on `instance` mismatch, drop absent windows, re-apply each remaining
  floating window through the normal path (stashed → stash again; restored → restore, then drop);
  tiled or fullscreen entries keep only `original` for the re-float rule (spec D13). A missing or
  unreadable file starts empty and is logged.
- **Rationale:** T4 and spec D10 commit the desired-state model and its write-before-command rule;
  this decision gives the concrete form. tmpfs makes fsync pointless, and a single writer needs no
  lock or version.
- **Evidence:** T4 (as amended by review F19-F21); spec D10. R1 C9 (collapsed by the R1 addendum),
  C12 (OCE-13, JD-12), C13 (OCE-14). `findmnt /run/user/1000` reports tmpfs (discovery notes).
- **Rejected alternatives:**
  - Recording observed settled geometry and reconciling it at startup — rejected by the user during
    the spec review pass (spec D10, review F19/F20).
  - fsync after each write — rejected under the evidence test: tmpfs (deferred with a trigger).
  - A version field and schema — rejected under the evidence test: one writer, one reader (deferred).
  - Writing in place without rename — rejected because a crash mid-write would leave a truncated
    file and lose every original.
- **Specialist owner:** The user (maintainer); advice from on-call-engineer.
- **Revisit criterion:** A second process reads the file, the file moves off tmpfs, or a format
  change must survive a rebuild while windows are stashed.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** D-14
- **Referenced in plan:** State file; On-Call Resilience Posture

### D-14: Startup sequence and stale socket handling

- **Question:** In what order does the daemon start, how long does it wait for niri, and what does
  it do with a leftover socket file?
- **Decision:** Read `XDG_RUNTIME_DIR` and `NIRI_SOCKET` (unset → exit 78). Connect to niri with
  backoff starting at 100 ms, doubling, capped at 2 s, for 60 s in total, then exit 69; log the first
  failure at info and then one warning about every 10 s. After connecting, wait up to 5 s for the
  initial event-stream snapshot, else exit 69. Re-apply the state file (D-13). Only then bind the
  hotkey socket. If the socket file exists: connect to it; refused → unlink and bind; accepted →
  another daemon owns it, exit 78.
- **Rationale:** Spec D11 requires the daemon to keep trying rather than exit fast, so systemd's
  start limit is never hit. Binding after the re-apply means a press cannot observe half-restored
  state. The connect probe tells a stale file left by a crash from a live daemon.
- **Evidence:** R1 C5 (OCE-6, JD-9): niri v26.04 `src/main.rs:217,238,276-320` and `niri.service`
  ordering; OQ-5 resolution. R1 C14 (OCE-15), Anecdotal: the dev-restart scenario from spec D10's
  rationale. R1 C18 (OCE-18). T4.
- **Rejected alternatives:**
  - Exit immediately when niri is unreachable and let systemd retry — rejected by spec D11 (start
    limit).
  - Bind the socket first and queue presses until re-apply finishes — rejected because the client
    would wait on startup work with no bound it can explain.
  - Always unlink an existing socket file — rejected because it would orphan a running daemon.
  - Jitter in the backoff — rejected under the evidence test: one client (deferred).
- **Specialist owner:** The user (maintainer); advice from on-call-engineer.
- **Revisit criterion:** niri at login takes longer than 60 s to accept connections, shown by exit 69
  at session start in the journal.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** D-15, D-16
- **Referenced in plan:** Startup and exit codes; On-Call
  Resilience Posture

### D-15: Daemon exit codes and event-stream end

- **Question:** What does the daemon do when its inputs fail, and how does it tell systemd?
- **Decision:** Exit-code contract between daemon and unit: 69 niri unreachable or no initial
  snapshot; 70 a worker thread ended (`Disconnected` on the state thread's channel); 75 the event
  stream ended (0-byte read or I/O error); 78 config error (`XDG_RUNTIME_DIR` or `NIRI_SOCKET` unset)
  or another daemon owns the socket, not restarted; SIGABRT on panic; SIGTERM is a clean stop and
  needs no handler. An event line that does not parse is logged at warn with its variant name only
  and skipped. The state file is never deleted when the stream closes; the instance tag discards it
  at the next start.
- **Rationale:** Distinct codes make the journal's "exited with status N" line a diagnosis. Codes
  follow `sysexits.h` (`EX_UNAVAILABLE` 69, `EX_SOFTWARE` 70, `EX_TEMPFAIL` 75, `EX_CONFIG` 78).
  Skipping unknown lines keeps one new niri event type from killing the daemon. Leaving the file in
  place means a daemon that wrongly concludes niri is gone still has its originals when it comes
  back to the same instance.
- **Evidence:** R1 C11 (OCE-12): niri-ipc `socket.rs:93-97` (EOF behavior), `lib.rs:35-44`. R1 C18
  (OCE-18). Spec D11 and the "niri restarts" flow (behavior unchanged; wording note in the plan's
  Sources section). Precedent: `~/.local/bin/micmute-led-sync` exits non-zero when its upstream ends
  so systemd restarts it.
- **Rejected alternatives:**
  - Delete the state file when the stream closes (the literal spec D11 wording) — rejected because
    the instance tag already discards it, and deleting loses originals if the close was not a niri
    restart.
  - Reconnect to the stream in a loop — rejected by spec D11: a new niri instance has new window ids.
  - Exit on an unparseable line — rejected because a niri upgrade adding an event type would stop
    the daemon until it is rebuilt.
- **Specialist owner:** The user (maintainer); advice from on-call-engineer.
- **Revisit criterion:** The journal shows a restart loop on one code, or an unparseable-event warning
  for an event the daemon needs.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** D-16
- **Referenced in plan:** Startup and exit codes; On-Call
  Resilience Posture; Sources and Plan Records

### D-16: systemd user unit

- **Question:** What does the user service look like, and how is it installed?
- **Decision:** `~/.config/systemd/user/niri-stash.service`:
  `[Unit] Description=Floating window stash for niri`, `PartOf=graphical-session.target`,
  `After=graphical-session.target`; `[Service] ExecStart=%h/.cargo/bin/niri-stash daemon`,
  `Restart=always`, `RestartSec=2`, `RestartPreventExitStatus=78`;
  `[Install] WantedBy=graphical-session.target`. Installed with `cargo install --path .`, then
  `systemctl --user daemon-reload && systemctl --user enable --now niri-stash.service`; the README
  lists the three commands and the build runs them on this machine. No environment lines:
  `NIRI_SOCKET` and `XDG_RUNTIME_DIR` are already in the user manager environment.
- **Rationale:** It copies the existing `asus-hotkey-notify.service` shape, so it behaves like the
  user's other session daemons. `RestartPreventExitStatus=78` stops a restart loop when the config is
  wrong or another daemon is running. `RestartSec=2` follows `micmute-led-sync.service`; with the
  60 s connect window, fast exits cannot trip the start limit.
- **Evidence:** R1 C19 (JD-6), C18 (OCE-18), C24 (JD-6). `~/.config/systemd/user/asus-hotkey-notify.service`
  (read at synthesis: `PartOf=`, `After=`, `Restart=always`, `RestartSec=3`, `WantedBy=`).
  Discovery notes: `micmute-led-sync.service` (`RestartSec=2`); `systemctl --user show-environment`
  includes `NIRI_SOCKET`. Spec Outcome and D11: the unit is a deliverable.
- **Rejected alternatives:**
  - `Restart=on-failure` — rejected because exit 75 after a niri restart must always bring the daemon
    back, and `always` matches the closer precedent.
  - An install script or Makefile — rejected under the simpler-version test (deferred).
  - niri `spawn-at-startup` — rejected by spec D11.
- **Specialist owner:** The user.
- **Revisit criterion:** The daemon fails to come back after a niri restart or a login.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** —
- **Referenced in plan:** Startup and exit codes; Definition of
  Done

### D-17: Vanished and tiled windows mid-press

- **Question:** How does the daemon notice a window that closes or leaves the floating layout while
  a press is settling it, given niri never reports the action as failed?
- **Decision:** During settling, `WindowClosed` for a press member removes it from the press (and
  from state); a `WindowOpenedOrChanged` showing it no longer floating removes it from the press and
  keeps only its original (spec D13). A `WindowOpenedOrChanged` with `workspace_id: None` is a
  window being dragged or moved between workspaces, not a vanished one: it stays in the press and in
  state, and settling resumes when a later report gives it a workspace and a position. At press end
  the daemon re-fetches `Windows` on the same request connection and drops any press member that is
  absent from the list (a member present with no workspace is kept). The reply is `ok`.
- **Rationale:** T5 as amended commits this mechanism. The re-fetch is the backstop, so the order in
  which stream events arrive relative to the daemon's own actions does not matter.
- **Evidence:** R1 C8 (OCE-9, JD-3): niri v26.04 `src/ipc/server.rs:381-399`, `src/input/mod.rs`
  ~1986-1992, verified. T5; review F18 and F22. User's event log (2026-10-11): a dragged window is
  reported with `workspace_id: None` while held (see D-9 evidence).
- **Rejected alternatives:**
  - Detect from the action reply — rejected because niri replies `Handled` for every action (A4).
  - Settle to the cap and save whatever is reported — rejected because a vanished window would be
    saved as stashed (review F18).
- **Specialist owner:** The user (maintainer); advice from on-call-engineer.
- **Revisit criterion:** A niri upgrade makes by-id actions report a missing window.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** —
- **Referenced in plan:** How a window is stashed and restored; On-Call
  Resilience Posture

### D-18: Logging content and correlation

- **Question:** What does the daemon log, at which level, and how does a reader follow one press?
- **Decision:** `env_logger` to stderr (the journal), timestamps off, a `press=<n>` field on every
  line about a press. info: startup re-apply outcomes, each press (workspace, direction, window ids),
  each forget or pop-back with its reason and the discarded original geometry, each alt-tab unstash,
  exit code and reason. warn: settle cap reached, unparseable event skipped, connect retries. error:
  state write failures. debug: each settle with elapsed ms. Windows are named by id and `app_id`,
  never by title.
- **Rationale:** The journal is the only operator surface. Logging the discarded original lets the
  user restore a window by hand if a bug forgets it. Titles can carry private content (document
  names, chat partners).
- **Evidence:** R1 C20 (OCE-19), Anecdotal: operator-surface reasoning and the spec's User
  Interactions ("the error goes to the session log").
- **Rejected alternatives:**
  - `tracing` with spans — rejected because a press number on each line gives the same correlation
    (D-3).
  - Timestamps in the log format — rejected because the journal adds them.
  - Logging window titles — rejected for privacy; `app_id` is enough to identify the window.
- **Specialist owner:** The user (maintainer); advice from on-call-engineer.
- **Revisit criterion:** A stranded sliver happens and the journal cannot explain it.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** D-19
- **Referenced in plan:** Logging; On-Call Resilience Posture; Risks and
  Assumptions

### D-20: Pure core unit tests plus a manual checklist

- **Question:** How is the feature tested?
- **Decision:** Unit tests on the pure core, kept free of I/O: edge pick and tie order (spec D3,
  D-8), the focus triple including the same-window return (T2, spec D6), manual move versus resize
  classification with its 1 px tolerance (T3, spec D7), the startup re-apply decisions (T4), and the
  sliver target arithmetic (D-7). The rest is the spec's "Done looks like this" list checked by hand
  with `niri msg -j windows` (stashed width 32 or the settled minimum; a left stash reports x ≈ −22;
  restored geometry within 1 px). Floating terminal: WezTerm. Calculator: `org.gnome.Calculator`.
  No nested-niri end-to-end tests.
- **Rationale:** The decision logic is where bugs would hide and it is cheap to test in isolation.
  The integration with niri is checked against the real session, which is the only environment the
  feature runs in.
- **Evidence:** R1 C25 (JD-11), Anecdotal. R1 C28: `~/.config/niri/cfg/rules.kdl` line 36 floats
  `org.gnome.Calculator` (verified at synthesis). Spec Outcome "Done looks like this".
- **Rejected alternatives:**
  - A nested-niri end-to-end harness — rejected under the evidence test: no regression yet
    (deferred).
  - Mocking the niri client to test the press engine — rejected because a mock would encode the same
    assumptions about niri that the manual checklist tests for real.
- **Specialist owner:** The user (maintainer).
- **Revisit criterion:** A bug escapes the unit tests and the manual checklist.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** —
- **Referenced in plan:** Implementation Approach; Definition of Done

### D-21: Unstash ships in work unit 2

- **Question:** Does the explicit unstash command get its own work unit?
- **Decision:** No. The `unstash` client command and its daemon handling ship in work unit 2 with the
  toggle; its keybind ships in work unit 4 with the toggle's keybind.
- **Rationale:** Unstash is the toggle's press engine with the direction fixed to restore, so it costs
  a few lines once the toggle works, and binding both keys in one unit keeps the config edit in one
  place.
- **Evidence:** Spec D12 (unstash restores every stashed window on the workspace; no-op is `ok`).
  User input E2 (reinstated the command).
- **Rejected alternatives:**
  - A separate fifth work unit — rejected because it would re-test the same press engine in
    isolation for no new behavior.
  - Folding it into work unit 4 — rejected because the daemon logic belongs with the press engine,
    and unit 4 is lifecycle work.
- **Specialist owner:** The user (maintainer).
- **Revisit criterion:** Unstash grows behavior the toggle does not have.
- **Dissent (if any):** None.
- **Driven by rounds:** R1
- **Dependent decisions:** —
- **Referenced in plan:** Work Units and Sequencing

## Amendments after the build

### D-22: Resizing removed; the daemon only moves windows

- **Date:** 2026-10-11. Follows spec amendment A1 in `decision-log.md`.
- **Decision:** A stash sends one relative move of 10 000 px toward the chosen edge and lets niri
  clamp it; a restore sends one relative move back to the original position, or nothing when the
  window already sits there. There is no resize, no pop-back, and no tracking of tiled or lifted
  windows: a stashed window that reports a geometry the engine did not ask for, leaves the floating
  layout, or is picked up by hand is forgotten at once.
- **Supersedes:** the resize half of D-7, the move-versus-resize judgement in D-9 (the quiet and cap
  timers are unchanged), and the drag and re-float handling in D-13 and D-18.
- **Known cost:** stashing a window that already sits at its edge produces no event from niri, so
  that press waits the full settle cap (1 s) before replying. The same case already existed for
  startup re-apply (R3).
- **Referenced in plan:** the amendment banner at the top of `../feature-implementation-plan.md`.

### D-23: No settling: read the window list back after each batch

- **Date:** 2026-10-11. Follows spec amendment A2 in `decision-log.md`.
- **Decision:** After `execute`, `transition_to` calls `list_windows` once and records each moved
  window's geometry as observed; restored windows are dropped and written out in the same call,
  and a moved window missing from the list is forgotten. `on_press` returns the reply directly.
  The engine takes no clock: `settle.rs`, the observed-state enum, press tokens, `on_tick`, and
  `next_deadline` are gone, and the state thread is a plain receive loop. If the read-back itself
  fails, the moved windows are forgotten rather than tracked blind.
- **Evidence:** niri `src/ipc/server.rs` (26.04): `Request::Action` runs the action in an idle
  callback and awaits it before replying `Handled`; `Request::Windows` is served from
  `event_stream_state`, which `refresh_and_flush_clients` updates after every dispatch. The reply
  therefore follows the refresh, so a `Windows` request after `Handled` sees the moved geometry.
  Live check: toggle and unstash reply in under 5 ms with the window already at its final place.
- **Supersedes:** D-9 entirely, the busy semantics in D-10 and D-11 (the `busy-timeout` reply code
  stays as the acceptor's guard against a stuck state thread), the press-end re-fetch in T5
  (it is now the read-back), and the settle logging lines in the observability section.
- **Referenced in plan:** the amendment banner at the top of `../feature-implementation-plan.md`.

### D-24: One thread, one poll call

- **Date:** 2026-10-11. Follows D-23: with no deadlines left, the daemon waits on exactly two
  things, niri's event stream and the hotkey socket.
- **Decision:** The daemon is a single thread. Each turn of its loop polls niri's stream, the
  listener, and every accepted client that has not yet sent its request line, then handles what
  is readable: one niri event, one accept, or one client. A client is only read once the kernel
  says it has data, so an idle or probing connection never holds the loop; a 500 ms read timeout
  remains as a backstop for a client that sends part of a line. Events already buffered by the
  stream reader are drained with a zero-timeout poll before waiting again. The acceptor and
  event-reader threads, the channel, the reply channels, `hotkey::server`, the `busy-timeout`
  reply code, and exit code 70 (worker thread ended) are gone.
- **Mechanism:** `rustix::event::poll` with the `event` feature, chosen over the `polling`
  crate because `polling`'s registration call is `unsafe` in 3.x and the user asked for no
  unsafe code. Adds 5 crates to the lock file.
- **Supersedes:** D-5 (three threads and one channel), the acceptor timeout in D-10 and D-11,
  and the exit code 70 entry in D-15. The hotkey line protocol is unchanged apart from the
  removed code.
- **Referenced in plan:** the amendment banner at the top of `../feature-implementation-plan.md`.

### D-25: Focus policy flag and niri's focus timestamps

- **Date:** 2026-10-11. Follows spec amendment A3.
- **Decision:** `FocusPolicy::{AltTabOnly, AnyFocus}` is passed from `main` (`daemon
  --no-focus-follows-mouse` selects `AnyFocus`) through `run_daemon` to the engine and its
  `FocusDetector`. Under `AnyFocus` a landing is any focus change to a window other than the
  current one, except a return to the window focus left while no window was focused. A restore
  press focuses the stashed window with the greatest `focus_timestamp` from the press snapshot,
  falling back to the first on the workspace; the detector's recency map is gone.
- **Rejected:** reading `focus-follows-mouse` from the niri config. niri-ipc has no config query;
  `ConfigLoaded` only says a reload happened; parsing the KDL with includes needs the unpublished
  `niri-config` crate. A flag is one line in the unit file and the setting rarely changes.
- **Referenced in plan:** the amendment banner at the top of `../feature-implementation-plan.md`.

