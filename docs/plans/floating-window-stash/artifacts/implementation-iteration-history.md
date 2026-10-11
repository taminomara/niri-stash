# Implementation Iteration History: Floating Window Stash for niri

<!--
This file records how the implementation plan for the floating window stash evolved across
discussion rounds. Committed decisions live in [implementation-decision-log.md](implementation-decision-log.md)
and the primary plan lives in [../feature-implementation-plan.md](../feature-implementation-plan.md).

This file also consolidates the per-round aggregation output; no separate facilitation files are
written. The claim ledger, Open Questions, and spec-maturity tags from each round live as fields on
that round's entry below. Size band: small (round cap 1).

Cross-referencing invariants:
- `Decisions produced:` — D-N IDs from implementation-decision-log.md that this round added or changed.
- `Changed in plan:` — sections of ../feature-implementation-plan.md that this round updated.
-->

## R1: Parallel specialist review

- **Specialists engaged:** han-core:on-call-engineer (OCE-1..OCE-22, OQ1), han-core:junior-developer
  (JD-1..JD-12, OQ1..OQ7). han-core:plan-synthesizer is dispatched once at synthesis.
- **New input provided:** the approved feature specification, its decision log, team findings and
  technical notes T1–T5, the scope boundary, and `.discovery-notes.md` (tech stack, session
  environment, niri v26.04 mechanics verified in source, measurements, gaps).
- **Claim ledger** (merged by substance; every originating identifier kept):

  | # | Claim | Identifiers | Category | State | Tag |
  | - | ----- | ----------- | -------- | ----- | --- |
  | C1 | std threads + one mpsc channel into a single state thread; no async runtime (tokio/mio/calloop out) | OCE-1, OCE-3, JD-10 | recommendation / YAGNI | Evidenced (niri-ipc 26.4.0 `socket.rs` blocking API; discovery notes cache listing) | plan-level |
  | C2 | Event reader must be an isolated thread feeding an unbounded channel: niri disconnects a stream client 64 events behind (`src/ipc/server.rs:42,119-134`); a disconnect must never wipe state | OCE-2 | finding | Evidenced (verified by the skill: `EVENT_STREAM_BUFFER_SIZE = 64`) | plan-level |
  | C3 | `panic = "abort"`; channel `Disconnected` on the state thread is fatal | OCE-4 | recommendation | Anecdotal (std thread semantics; no artifact) | plan-level |
  | C4 | niri request calls need timeouts; `niri_ipc::Socket` exposes none, so use a thin client over own `UnixStream` with 1 s read/write timeouts and a fresh connection per press | OCE-5 | finding | Evidenced (`socket.rs:18-61`) | plan-level |
  | C5 | Startup connect retry: backoff 100 ms ×2 capped 2 s, total window 60 s then exit 69; `NIRI_SOCKET` is fixed at process start and niri exports it before READY | OCE-6, JD-9 | recommendation | Evidenced (niri `src/main.rs:217,238,276-320`, verified by the skill; `niri.service` ordering in discovery notes) | plan-level |
  | C6 | Settling: constants QUIET=150 ms, CAP=1 s; quiet timer starts only after a reported size change for windows whose size was requested; skip settling when already at target; settle all windows of a press in parallel; log every settle | OCE-7, JD-7, JD-8 | recommendation | Unverified (application resize latency not reachable with granted tools) — disposition: `not reachable with granted tools`; values are constants with a measurement step in the first work unit | plan-level |
  | C7 | Socket-side timeouts: acceptor read 500 ms; daemon waits for state-thread reply then answers `busy-timeout`; client read timeout | OCE-8 (3 s), JD-4 (10 s) | recommendation | Disputed on the client value only | plan-level |
  | C8 | T5 contradiction: niri replies `Handled` for every action, so "the action's error" never arrives; detect vanished windows via `WindowClosed`, a non-floating `WindowOpenedOrChanged`, and a re-fetched `Windows` at press end | OCE-9, JD-3 | T# contradiction (T5) | Evidenced (`src/input/mod.rs` ~1986, `src/ipc/server.rs:381-399`, verified by the skill) | T#-contradiction |
  | C9 | State writes W1 (after snapshot, before first action), W2 (settle end, before reply), W3 (after every event-driven change) | OCE-10 | recommendation; T4 confirm | Evidenced (T4; D10 / Decision:) | plan-level |
  | C10 | T4 contradiction (low): an in-flight entry whose current geometry equals its original is dropped at reconcile, not adopted | OCE-11 | T# contradiction (T4) | Anecdotal (reasoning from T4's rule) | T#-contradiction |
  | C11 | Never delete the state file on stream close; EOF (0 bytes) → exit 75; unparseable line → warn and skip (new event variants); pin `niri-ipc = "=26.4.0"` | OCE-12 | finding | Evidenced (`socket.rs:93-97`; niri-ipc `lib.rs:35-44`) | plan-level |
  | C12 | State file: `$XDG_RUNTIME_DIR/niri-stash/state.json`, JSON `{instance, windows}`, write tmp + rename, no fsync (tmpfs), no version field or lock | OCE-13, JD-12 | recommendation / YAGNI | Evidenced (`findmnt /run/user/1000` = tmpfs) | plan-level |
  | C13 | Spec gap: W1 write failure → move nothing, reply `err state-unwritable`; W2/W3 failure → log error, keep running | OCE-14 | finding | Evidenced (D10 / Decision: "written down before any window is moved") | spec-level |
  | C14 | Socket file exists at bind: connect-probe; refused → unlink and bind; accepted → exit 78 (another daemon) | OCE-15 | recommendation | Anecdotal (dev-restart scenario from D10 rationale) | plan-level |
  | C15 | Expected focus changes (T2) expire at the press's settling end; a no-op `focus-tiling` must not leave a stale expectation | OCE-16, JD-7 | finding; T2 confirm | Evidenced (`switch_focus_floating_tiling` no-op, discovery notes) | plan-level |
  | C16 | Client ⇄ daemon contract: one binary `niri-stash` with `daemon` / `toggle`; socket under `$XDG_RUNTIME_DIR`; request `toggle\n`; reply `ok` / `err <code> <message>`; client exit codes | OCE-17, JD-4 | contract | Evidenced (OI-1; D12 / Decision:) | plan-level |
  | C17 | niri nulls a spawned command's stderr, so the keybind must wrap the client in `systemd-cat -t niri-stash` for the error to reach the journal | JD-5 | finding | Evidenced (`src/utils/spawning.rs:118-120`, verified by the skill) | plan-level |
  | C18 | Daemon exit-code contract for the unit: 69 niri unreachable, 70 thread died, 75 stream ended, 78 config/duplicate (no restart), SIGABRT panic | OCE-18 | contract | Evidenced (D11; precedent units) | plan-level |
  | C19 | Unit copies `asus-hotkey-notify.service` precedent; `ExecStart=%h/.cargo/bin/niri-stash daemon`; `Restart=always`; no install script; three README commands | JD-6 | recommendation | Evidenced (discovery notes precedent units) | plan-level |
  | C20 | Logging: `env_logger` to stderr, no timestamps, `press=<n>` correlation, log discarded originals, never log titles | OCE-19 | recommendation | Anecdotal (operator-surface reasoning; spec User Interactions) | plan-level |
  | C21 | YAGNI: no metrics/dashboards/alerts/health endpoint; no runtime config for timings; no kill switch beyond `systemctl --user stop`; no clap; no tokio; no state schema/version; no install script | OCE-20, OCE-21, OCE-22, OCE-3, JD-4, JD-8, JD-12 | YAGNI candidate | Evidenced (single-user laptop; D12; T4 "private to the daemon") | plan-level |
  | C22 | Edge choice cannot read the working area over IPC; options: measure from the output (≤28 px bias in the tie zone), an `--insets` flag, or read noctalia's config | JD-1 | finding | Evidenced (discovery notes: `niri msg layers` has no geometry; bar thickness 28) | plan-level |
  | C23 | All position changes as `AdjustFixed(delta)` from the last reported output-relative position, never `SetFixed` (working-area-relative) | JD-2 | recommendation; T1 confirm | Evidenced (`FloatingSpace::move_window`, verified by the skill) | plan-level |
  | C24 | Install deliverables: who edits `keybinds.kdl` and `~/.config/systemd/user/`, which key, what "installed" means | JD-6 | finding | Evidenced (spec Outcome; D11 / Decision:) | plan-level |
  | C25 | Done by hand via `niri msg -j windows`; unit-test the pure core (edge pick, focus triple, move/resize classification, reconcile); nested-niri e2e is a test-engineer question | JD-11 | recommendation | Anecdotal | plan-level |
  | C26 | Must the push wait for the resize to settle (two phases) or can both go together? | OCE OQ1, JD-2 | open question | Unverified (reviewer) — disposition: `verified: niri stores the requested position as an unclamped fraction (Data::set_logical_pos) and re-clamps on every size change (Data::update), so one phase suffices` | plan-level |
  | C27 | Repo licence: niri-ipc is GPL-3.0-or-later | JD OQ7 | open question | Evidenced (crate metadata) | plan-level |
  | C28 | Which floating calculator the Outcome checklist means | JD OQ6 | open question | Evidenced after the fact (`rules.kdl` floats `org.gnome.Calculator`) | plan-level |

- **Open Questions raised:**
  - OQ-1 (C22): how edge distance is measured without the working area.
  - OQ-2 (C24): which hotkey, and whether the build edits the user's niri and systemd config directly.
  - OQ-3 (C7): client read-timeout value.
  - OQ-4 (C26): one settling phase or two.
  - OQ-5 (C5): `NIRI_SOCKET` freshness across niri restarts.
  - OQ-6 (C8, C10): the two T# contradictions.
  - OQ-7 (C13): spec gap for an unwritable state file.
  - OQ-8 (C27, C28): licence; calculator app.
- **Spec-maturity tags:** plan-level 25 (C1–C7, C9, C11, C12, C14–C28); spec-level 1 (C13);
  T#-contradiction 2 (C8 from OCE-9 + JD-3; C10 from OCE-11). **Gate: tripped** on the first
  condition (≥ 2 T#-contradictions from ≥ 2 distinct specialists: on-call-engineer and
  junior-developer). The second condition (≥ 5 spec-level from ≥ 3 specialists) did not trip. See
  the facilitator review field below and escalation E1.
- **Contract pinning:** N at round open. Unpinned at this point: the client ⇄ daemon socket protocol
  (OI-1), the daemon exit-code contract, the keybind line, the systemd unit's decision-bearing
  values. All four are pinned by the decisions this round produced (see `Decisions produced:` after
  synthesis); the Step 9 check verifies the plan text.
- **Resolution source:**
  - OQ-1: evidence. The work item's own words are "the exact edge of the monitor … closest to the
    window's center" (scope-boundary.md, Stated Scope); the spec's "usable area" wording came from
    the clamp, which still governs where the sliver lands. Distance is measured from the output
    edges; the ≤ 28 px bias affects only windows whose centre sits within the bar height of a tie.
    An `--insets` flag is deferred under YAGNI.
  - OQ-2: user input (E2) for the key; evidence for the rest: the spec's Outcome names the unit and
    the keybind as deliverables of this feature, so the build installs the unit into
    `~/.config/systemd/user/` and adds one line to `~/.config/niri/cfg/keybinds.kdl`, following the
    two existing precedents.
  - OQ-3: evidence. Worst case is one in-flight press plus this one, each bounded by CAP plus
    request latency (~1.3 s), so the daemon's busy-timeout is 4 s and the client's read timeout is 5
    s; the client's bound must exceed the daemon's so the daemon's `err busy-timeout` is what the
    client reports.
  - OQ-4: evidence (C26 disposition). One phase: resize and push are issued back to back.
  - OQ-5: evidence. niri runs `systemctl --user import-environment` for `NIRI_SOCKET` before it
    signals READY (`src/main.rs:217,238`); `niri.service` is `Before=graphical-session.target` and the
    daemon unit is `After=`/`PartOf=` it, so a session restart gives the daemon a fresh environment.
    The 60 s retry window and exit 69 cover the remaining race.
  - OQ-6: user input (E1). The user chose to pause for a spec review pass rather than amend T4 and
    T5 in place. The corrections (C8, C10, the facilitator's M1 and M4, and the C13 row) are handed
    to `han-planning:iterative-plan-review` on the specification; this run resumes after it.
  - OQ-7: handed to the spec review pass with OQ-6 (facilitator M3: a new user-observable row should
    be approved consciously, not added from the plan stage).
  - OQ-8: evidence. Licence `GPL-3.0-or-later` (niri-ipc is GPL-3.0-or-later and is linked in);
    calculator is `org.gnome.Calculator` (floated by `~/.config/niri/cfg/rules.kdl`).
- **Facilitator review (gate-trip pass):** Verdict: mostly a literal-rule artifact; C8 re-verified
  in niri source (`src/ipc/server.rs` 381–402, `src/input/mod.rs` ~1986–1992); C10 is a derivation
  from T4's own text (re-tag Evidenced, single source). No correction changes normal-use behavior;
  C10 changes the crash case in the spec's "The daemon restarts" flow (two presses → one). Missed by
  the aggregator: **M1** an interrupted *unstash* has no reconcile rule in T4 and would fall through to
  "manually changed", forgetting the original, which conflicts with T4's promise; **M2** this entry's
  draft wording presumed the user's answer to E1 and cited spec findings F18–F20 that did not exist
  (corrected in this revision); **M3** the C13 row is a user-observable commitment that should be
  approved consciously; **M4** C8's replacement relies on the press-end re-fetch as the backstop for
  event ordering, and a window tiled mid-press has no spec row; **M5** C3, C14, C20, C25 stay
  Anecdotal (plan-level only). Open: OQ-A how T4 reconciles an interrupted unstash (blocks the T4
  amendment); OQ-B tiled-mid-press wording; OQ-C E1 must list the actual edits. Handoff named:
  on-call-engineer or behavioral-analyst for the interrupted-unstash rule. Recommendation left to
  the user: the edit set is small enough to amend in place if no new behavioral decision is needed.
- **Decisions produced:** D-1, D-2, D-3, D-4, D-5, D-6, D-7, D-8, D-9, D-10, D-11, D-12, D-13,
  D-14, D-15, D-16, D-17, D-18, D-19, D-20, D-21 (all added at synthesis after the R1 addendum;
  full except trivial D-2, D-4, D-19). D-10 and D-11 pin spec OI-1; D-7's restore ordering (move
  before resize) was settled at synthesis from niri v26.04 `FloatingSpace::move_window`.
- **Changed in plan:** Outcome; User Stories; Constraints and Boundaries; Implementation Approach; Project and packaging; Talking to niri; How a window is stashed and restored; Hotkey socket contract; Keybinds; State file; Startup and exit codes; Logging; Work Units and Sequencing; Definition of Done; Testing Strategy; On-Call Resilience Posture; Risks and Assumptions; Deferred (YAGNI); Open Items; Sources and Plan Records; Recommendation
- **Next-step recommendation:** Pause and sharpen the spec (gate tripped). Escalated as E1 with the
  facilitator's edit list; the user chose to pause. `plan-implementation` stops here and hands the
  following to `han-planning:iterative-plan-review` on `../feature-specification.md`: C8 (T5 and the
  "declines" wording in the Edge Cases row and D12), C10 and M1 (T4 reconcile for interrupted stash
  and unstash, and the "The daemon restarts" flow), M4 (window tiled mid-press), C13 (unwritable
  state file row). Everything else in this ledger is plan-level and stands for the resumed run.

### R1 addendum: spec review pass completed

The `han-planning:iterative-plan-review` pass requested at E1 completed in one lightweight
iteration (its record: `review-findings.md` F18–F23, `review-iteration-history.md` R1). During it the
user redirected the restart model: the state file records **desired state** (original geometry plus
stashed-at-edge or restored), is written before any command is sent to niri, and a restarted daemon
re-applies it. Effects on this round's ledger: C8 and C13 are now spec commitments (T5, Edge Cases
rows); C10 is resolved by the desired-state model rather than by a drop rule; C9's three write points
collapse to "write desired state before the first command of an operation, and when an entry is
dropped"; the facilitator's M1 and M4 are closed (interrupted unstash is re-applied; mid-press tiling
has a row). All remaining ledger items are plan-level. The resumed run proceeds to E2, the YAGNI
sweep, and synthesis without a further specialist round (small band, cap 1).

### R1 addendum: user observation after synthesis

After the plan was written the user reported, from their own use of niri: application resize
latency is short and niri emits one layout change per frame during a resize; a hand drag emits one
layout report at drag start and one at drag end. Effect: C6's Unverified label on resize latency is
discharged (disposition: `verified: user's event log 2026-10-11`). The user then pasted the raw
events: a drag is `WindowOpenedOrChanged { workspace_id: None, tile_pos_in_workspace_view: None }`
at pick-up and a second report with the workspace and position at drop; a resize is per-frame
`WindowLayoutsChanged`. D-9 recognises the lift-and-drop pair and judges the hand move on the drop
report, keeping a `SETTLE_QUIET_MS` debounce only for unbracketed changes; D-17 keeps a window with
no workspace in the press rather than treating it as vanished; spec T2 and T3 carry the same rules
(review F25). The user then dumped the full stream to `~/niri-drag-log.jsonl`, copied here as
`artifacts/niri-event-log-2026-10-11.jsonl`: it confirms the drag pair, shows a hand resize as ~75
per-frame layout reports with no lift-and-drop pair (so the quiet-interval path is the primary path
for hand resizes), and shows a close emitting the focus change before `WindowClosed`. D-9, D-20's
testing strategy (replay fixture), T3, and assumption A9 were updated. Five more logs followed
(`artifacts/niri-event-log-2026-10-11-1.jsonl` to `-5.jsonl`), confirming: scripted resizes and moves
are reported at once with no animation and a no-op push is unreported (so settling relies on the cap
or the already-at-target shortcut when nothing changes); the clamp values 10 px at 32 px and ~75 px at
900 px; focus `None` for niri's built-in switcher, the lock screen and the overview with same-window
returns; and that a keybind workspace move does **not** lift the window (one report on the new
workspace, same position), which corrects an inference in D-9 and T3. New assumptions A10 and A11
record the evidence. Work unit 1's measurement step stays as a confirmation only.

## YAGNI and scope sweep (Step 7.5)

Every committed item was walked through the evidence test, the simpler-version test, and the scope
test against `scope-boundary.md`. Nothing was cut for scope: every subsystem the plan touches (daemon,
hotkey client, niri IPC, systemd unit, niri keybinds, state file) is named in the work item or is a
necessity of it, and the two reinstatements below carry the user's direction as justification.

| Item | Gate failed | Resolution | Source |
| ---- | ----------- | ---------- | ------ |
| Async runtime (tokio/mio/calloop) | simpler-version | replaced: std threads + one channel; `recv_timeout` is the only timer | R1 C1 (OCE-1, OCE-3, JD-10) |
| Command-line parser crate | simpler-version | replaced: two verbs parsed by hand | R1 C16 (JD-4) |
| Runtime configuration for settling and timeout values | evidence (knob no caller sets) | deferred; reopen when two applications need different values | R1 C21 (OCE-21) |
| Metrics, dashboards, alerts, health endpoint | evidence (speculative observability) | deferred; reopen when stranded slivers recur and the journal cannot explain them | R1 C21 (OCE-20) |
| Kill switch / feature flag | simpler-version | replaced: `systemctl --user stop`; slivers stay put and state is kept | R1 C21 (OCE-22) |
| State-file version field, schema, lock file | evidence (single writer) | deferred; reopen when a second process reads the file | R1 C12 (JD-12) |
| Install script or Makefile | simpler-version | replaced: three commands in the README, run by the build on this machine | R1 C19 (JD-6) |
| `--insets` flag for the working area | evidence (one bar, fixed) | deferred; reopen when a window near the bar is sent to a side edge when you expected the top, more than once | R1 C22 (JD-1) |
| Nested-niri end-to-end tests | evidence (no regression yet) | deferred; reopen when a bug escapes the pure-core unit tests and the manual checklist | R1 C25 (JD-11) |
| `thiserror` | evidence (one error consumer) | deferred; reopen when a second consumer of typed errors appears | R1 C1 (OCE-3) |
| `fsync` on the state file | evidence (tmpfs) | not included; a kernel crash voids every window id anyway | R1 C12 (OCE-13) |
| Jitter in the startup backoff | evidence (single client) | not included | R1 C5 (OCE-6) |
| Explicit unstash command and second hotkey | — (was deferred in the spec) | **reinstated by the user (E2)**; the user's direction is the justification | E2 |
| Replacing the Mod+Shift+S screenshot binding | scope (the work item never mentions screenshots) | **kept by the user's explicit direction (E2 follow-up)**; recorded as the justification | E2 |

No item in the sweep changes a behavior the spec commits to, so none was escalated beyond E2.

## Unaudited evidence classes

- niri v26.04 source lines the skill read while resolving this round (`Data::set_logical_pos`,
  `Data::update`, `src/utils/spawning.rs`, `src/main.rs` environment export, `src/ipc/server.rs`
  action dispatch) were not seen by either specialist before they reported. They support the
  resolutions of OQ-1, OQ-4, OQ-5 and C17.

## Escalation register

### E1: The process gate says two committed mechanics were contradicted; continue with them corrected in place, or pause for spec work?

- **Round:** R1
- **Answer:** "Pause for a spec review pass."
- **Landed in:** this round's Next-step recommendation; the spec review pass receives the edit list.

### E2: Which key should the stash hotkey use?

- **Round:** R1
- **Answer:** "Mod+S to toggle, and bind Mod+Shift+S to unstash." Follow-up on the conflict with the
  existing region-screenshot binding (which Fn+F11 also sends): "Rebind Mod+Shift+S to unstash."
- **Landed in:** the keybind decision in the plan (two lines; the screenshot line is replaced, Print
  keeps the region screenshot); spec F24 reinstating the explicit unstash command; D12.
