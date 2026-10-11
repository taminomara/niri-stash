# Review Findings: Floating Window Stash for niri (feature specification)

<!--
This file records every finding raised while reviewing the feature specification with
`han-planning:iterative-plan-review`, and how each was resolved. The specification lives one
directory up. Round-by-round history lives in [review-iteration-history.md](review-iteration-history.md).

Numbering continues from the specification's own review log ([team-findings.md](team-findings.md),
F1–F17) so that every F# in this folder is unique. This review starts at F18.

Mode: lightweight (self-review). Spec-aware mode engaged. The review was requested by
`han-planning:plan-implementation` after its round-1 spec-maturity gate tripped; the inputs are the
round-1 claim ledger in [implementation-iteration-history.md](implementation-iteration-history.md)
(C8, C10, C13, facilitator M1 and M4) and the niri v26.04 source checks recorded in
[.discovery-notes.md](.discovery-notes.md).

Cross-referencing invariants:
- `Raised in round:` — R# IDs from [review-iteration-history.md](review-iteration-history.md).
- `Changed in plan:` — sections of ../feature-specification.md that changed, or —.
- `Changed in tech-notes:` — T# IDs in [feature-technical-notes.md](feature-technical-notes.md) added or edited, or —.
-->

## Major findings

### F18: T5 relied on an error niri never sends for a vanished window

- **Agent:** self-review (raised upstream by han-core:on-call-engineer OCE-9 and han-core:junior-developer JD-3 during implementation planning)
- **Category:** assumption refuted; T#-contradiction (T5)
- **Finding:** T5 said a window that closed between the snapshot and the action is detected because
  "the action's error is logged". The Edge Cases row and D12 spoke of niri "declining" a request.
  niri v26.04 answers every action request with `Response::Handled` after a path-only
  `validate_action`, and the by-id actions silently do nothing when the id is gone. Under the old
  wording a vanished window would have settled to the cap and been saved as stashed.
- **Evidence considered:** niri v26.04 `src/ipc/server.rs` lines 381–399 (`Request::Action` →
  `validate_action` → `Response::Handled`), `src/input/mod.rs` ~1986–1992 (`SetWindowWidthById`
  lookup with silent `None`); both read by the skill (trust class: codebase, read-only clone).
- **Resolution:** T5 now states the detection mechanism: `WindowClosed` and a non-floating
  `WindowOpenedOrChanged` remove the window from the press during settling, and a re-fetched window
  list at press end is the backstop, so stream-event ordering does not matter. The Edge Cases row
  and D12 / Decision: drop "declines" and say niri does not report such a request as failed.
  Behavior (skip the window, finish the rest, client exits 0) is unchanged.
- **Resolved by:** evidence
- **Raised in round:** R1
- **Changed in plan:** Edge Cases and Failure Modes
- **Changed in tech-notes:** T5

### F19: A crash before the first action adopted an untouched window as stashed

- **Agent:** self-review (raised upstream by han-core:on-call-engineer OCE-11)
- **Category:** edge case; T#-contradiction (T4)
- **Finding:** T4 adopted any in-flight stash entry "as stashed at whatever geometry it has now". A
  crash after the originals write and before the first action leaves the window at its original
  geometry; adopting it makes the next press see the whole workspace as stashed and unstash with no
  visible change, so the user presses twice. Nothing was lost.
- **Evidence considered:** T4's own text (the originals are written before the first action, so
  the window between those two moments is at its original); D10 / Decision:. Trust class: provided
  (the specification's own commitments), derivation confirmed by the gate-trip facilitator.
- **Resolution:** Superseded by the user's direction during this pass: the saved file records
  desired state (original plus stashed-at-edge or restored), written before any command, and a
  restart re-applies it. Under that rule the untouched window is simply stashed at startup, which is
  what the interrupted press intended. T4, D10 and the "The daemon restarts" flow were rewritten.
- **Resolved by:** user input
- **Raised in round:** R1
- **Changed in plan:** Alternate Flows and States
- **Changed in tech-notes:** T4

### F20: An interrupted unstash had no recovery rule and would forget the original

- **Agent:** self-review (raised by han-planning:discussion-facilitator, finding M1, during the implementation-planning gate pass)
- **Category:** unhandled failure mode
- **Finding:** T4's in-flight marker covered "a stash or unstash" but its reconcile rules covered only
  stashes. With a one-phase unstash (resize and move issued back to back, resolved in implementation
  planning from `Data::set_logical_pos` storing the unclamped fraction), a crash between the two
  leaves an in-between geometry that fell through to "otherwise treats it as manually changed" and
  discarded the original, contradicting T4's own promise.
- **Evidence considered:** T4 text; niri v26.04 `src/layout/floating.rs` `Data::set_logical_pos` and
  `Data::update` (codebase). The behavioral decision this needed (what an interrupted unstash should
  do) has one answer consistent with D5 and D10: finish restoring the original.
- **Resolution:** Covered by the same desired-state rule: an entry marked restored is restored and
  dropped at startup. No separate case remains. T4, D10 and the "The daemon restarts" flow were
  rewritten.
- **Resolved by:** user input
- **Raised in round:** R1
- **Changed in plan:** Alternate Flows and States
- **Changed in tech-notes:** T4

### F21: No behavior for a state file that cannot be written

- **Agent:** self-review (raised upstream by han-core:on-call-engineer OCE-14; flagged by the facilitator, M3, as a user-visible addition that needs conscious approval)
- **Category:** unhandled failure mode
- **Finding:** The spec had no row for a failed write of the originals at the start of a press, nor
  for a failed write later. D10 / Decision: already commits that originals are written before any
  window moves, which determines the first case.
- **Evidence considered:** D10 / Decision: and Rationale; the runtime directory is tmpfs
  (`findmnt /run/user/1000`), so a write failure means a full or read-only directory that a restart
  would not fix.
- **Resolution:** One Edge Cases row added: when the desired state cannot be written at press
  start, nothing moves and the hotkey client exits with an error to the session log. This is the
  user's own rule ("write desired state to file, then send commands") applied to the failure case.
  Later writes only happen when an entry is dropped, and a failure there is logged. D10 / Decision:
  and T4 carry the same sentence. The row is listed in this review's closing summary so the
  approval is conscious.
- **Resolved by:** evidence
- **Raised in round:** R1
- **Changed in plan:** Edge Cases and Failure Modes
- **Changed in tech-notes:** T4

### F22: No row for a window tiled or fullscreened in the middle of a press

- **Agent:** self-review (raised by han-planning:discussion-facilitator, finding M4)
- **Category:** edge case
- **Finding:** D13 covers a stashed window that is tiled later, but the spec said nothing about a
  window that leaves the floating layout while a press is still settling it.
- **Evidence considered:** D13 / Decision:; niri v26.04 `src/layout/workspace.rs` ("Floating windows
  are never fullscreen"; fullscreen moves a floating window to the scrolling layout).
- **Resolution:** One Edge Cases row added: the window leaves the press, its original is kept, and it
  is restored to that original when it returns to floating, the same rule as D13. T5 names the
  non-floating `WindowOpenedOrChanged` as the signal.
- **Resolved by:** evidence
- **Raised in round:** R1
- **Changed in plan:** Edge Cases and Failure Modes
- **Changed in tech-notes:** T5

### F23: D12 still said niri "declines" a request

- **Agent:** self-review
- **Category:** standards conflict (decision log disagreed with the corrected T5)
- **Finding:** After F18, D12 / Decision: still described a request niri "declines", a case that never
  occurs for stash actions in niri v26.04.
- **Evidence considered:** F18's source citations.
- **Resolution:** D12 / Decision: reworded: a window that closes or leaves the floating layout
  mid-press is detected by the daemon from niri's window events and a fresh window list at the end
  of the press.
- **Resolved by:** evidence
- **Raised in round:** R1
- **Changed in plan:** Edge Cases and Failure Modes
- **Changed in tech-notes:** —

### F24: User reinstated an explicit unstash hotkey

- **Agent:** self-review (user direction received during implementation planning, escalation E2)
- **Category:** YAGNI candidate reinstated
- **Finding:** The spec deferred "explicit stash, unstash, and status commands" under the
  simpler-version test. When asked which key to bind, the user asked for Mod+S to toggle and
  Mod+Shift+S to unstash.
- **Evidence considered:** User direction (escalation E2 in
  [implementation-iteration-history.md](implementation-iteration-history.md)); the YAGNI rule's
  "user always wins" clause.
- **Resolution:** Added the alternate flow "Unstash everything with the unstash hotkey", a second
  trigger, a User Interactions note that the binding replaces the region-screenshot key, and amended
  D12 / Decision:. The Deferred entry now covers explicit stash and status only.
- **Resolved by:** user input
- **Raised in round:** R1
- **Changed in plan:** Outcome; Actors and Triggers; Alternate Flows and States; User Interactions; Deferred (YAGNI)
- **Changed in tech-notes:** —

### F25: A drag reports twice, so the hand-move decision must wait for the drop

- **Agent:** self-review (user observation supplied after implementation-plan synthesis)
- **Category:** edge case
- **Finding:** The user's event log shows that niri emits one layout change per frame during a
  resize, and that a hand drag is reported as a window-changed event with no workspace and no
  position at pick-up and another with the workspace and new position at drop, with the focused
  window re-reported as focused. Acting on the first report would pop a dragged sliver back under the
  cursor; treating "no workspace" as "gone" would drop the window from a press; treating the
  re-reported focus as a transition would confuse alt-tab detection.
- **Evidence considered:** User's event log (2026-10-11), kept at
  [niri-event-log-2026-10-11.jsonl](niri-event-log-2026-10-11.jsonl): a hand resize is a per-frame
  stream with fixed position (lines 21–96), a hand move is a lift-and-drop pair (97–100), a close
  reports the focus change before the close (101–103). Five further logs
  (`niri-event-log-2026-10-11-1.jsonl` to `-5.jsonl`): scripted stash and restore with the clamp
  numbers confirmed and no-op pushes unreported; alt-tab, cancel and pick-same through niri's
  switcher; keybind workspace moves (one report, same position, no lift) and a tile-float round trip
  restoring the old floating geometry; lock/unlock, overview, close in overview; close in the
  switcher. T1, T2 and T3 mechanics.
- **Resolution:** T3 now recognises the lift-and-drop pair explicitly and judges the hand move on the
  drop report, with the quiet interval kept only for changes not bracketed that way; T2 excludes a
  repeated focus report for the already-focused window. No spec behavior changes. Plan D-9 and D-17
  carry the same rules and the log as evidence; a window with no workspace stays in a press.
- **Resolved by:** user input
- **Raised in round:** R1
- **Changed in plan:** Alternate Flows and States
- **Changed in tech-notes:** T1, T2, T3

## Minor edits

None.
