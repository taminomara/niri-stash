# Team Findings: Floating Window Stash for niri

<!--
This file records every finding raised by the review team for the floating window stash, and how
each was resolved. Behavioral outcomes live in [../feature-specification.md](../feature-specification.md);
decisions the findings affected live in [decision-log.md](decision-log.md); load-bearing mechanics
live in [feature-technical-notes.md](feature-technical-notes.md).

Reviewers: han-core:junior-developer (identifiers JD-n) and han-core:edge-case-explorer (EC-n),
small-team cap of two. Findings raised by both were merged by substance and carry both identifiers.

Both reviewers marked several findings `Unverified` because they had no niri source or session.
The skill then verified each of those against the niri v26.04 source (tag matching the installed
`niri 26.04 (8ed0da4)`) and against the live `niri msg` output on this machine. Where a finding
says "verified by the skill", the reviewer's unverified label has been discharged and the finding
carries its evidence; none of them was escalated as a blocker on the reviewer's word alone.

Cross-referencing invariants:
- `Affected decisions:` — D# IDs from decision-log.md, or —.
- `Affected tech-notes:` — T# IDs from feature-technical-notes.md, or —.
- `Changed in spec:` — sections of ../feature-specification.md, or —.
-->

## Major findings

### F1: The daemon's own resize can read as a manual change and lose the original

- **Agent:** edge-case-explorer, junior-developer
- **Reviewer identifiers:** EC-1, JD-3
- **Finding:** T3 compared layout events against "the size niri reports after the request settles"
  without defining settling. An application that answers the 32-pixel request with a rounded or
  minimum size, or a beat later, would have its stash classified as a manual change the moment it
  was made, and D7 would discard the original. Terminals, the main scratch-window case, round to
  cell grids.
- **Unverified (reviewer):** niri's event timing for client-acknowledged resizes. **Verified by the
  skill:** niri v26.04 `src/layout/floating.rs` `set_window_width` clamps to min/max and issues
  `request_size_once`; the application answers asynchronously and may answer differently.
- **Resolution:** Introduced a settling period on every stash and unstash. Nothing the window does
  inside it counts as a manual change; the settled geometry is what later events are compared
  against; a late answer after the period is a manual change, with a row saying so. Primary Flow
  gained step 6.
- **Resolved by:** evidence
- **Affected decisions:** D7, D16
- **Affected tech-notes:** T1, T3
- **Changed in spec:** Primary Flow; Alternate Flows and States; Edge Cases and Failure Modes

### F2: A crash mid-stash strands slivers with no record of their originals

- **Agent:** edge-case-explorer, junior-developer
- **Reviewer identifiers:** EC-2, JD-6
- **Finding:** The draft saved state after moving the windows (Primary Flow step 7). A daemon crash
  or rebuild between moving and saving left slivers on screen that the restarted daemon had never
  heard of, which breaks the Outcome's promise.
- **Resolution:** Originals are written before any window is moved; settled state is written before
  the press is acknowledged. On restart, an in-flight stash is adopted at the window's current
  geometry so the original stays restorable.
- **Resolved by:** evidence
- **Affected decisions:** D10
- **Affected tech-notes:** T4
- **Changed in spec:** Primary Flow; Alternate Flows and States; Coordinations

### F3: Moving focus to the tiled layout is undefined on a floating-only workspace

- **Agent:** edge-case-explorer, junior-developer
- **Reviewer identifiers:** EC-3, JD-2
- **Finding:** Primary Flow step 6 (now 7) said focus moves to the tiled layout. With no tiled
  windows, the reviewers feared niri would report "no window focused" and the daemon's own stash
  would arm the alt-tab detector.
- **Unverified (reviewer):** niri's behavior for focus-tiling on an empty tiling layout. **Verified
  by the skill:** niri v26.04 `src/layout/workspace.rs` `switch_focus_floating_tiling` returns
  without change when the scrolling layout is empty. Focus stays on the sliver; no "no window"
  state is produced.
- **Resolution:** Spec now states focus stays on the sliver in that case, User Interactions notes
  that keystrokes then go into a barely visible window, and focus changes the daemon itself
  requests are excluded from the alt-tab detector.
- **Resolved by:** evidence
- **Affected decisions:** D6, D8
- **Affected tech-notes:** T2
- **Changed in spec:** Primary Flow; User Interactions

### F4: "No window focused" has many sources besides the picker

- **Agent:** edge-case-explorer, junior-developer
- **Reviewer identifiers:** EC-4, JD-1
- **Finding:** The draft rule unstashed on any (no window, stashed window) pair and discussed only
  the pointer-on-sliver case. Cancelling the picker, locking and unlocking, opening and closing the
  overview, and switching through an empty workspace would all pop out a sliver that merely had
  focus before.
- **Unverified (reviewer):** which events niri emits for picker cancel, workspace switch, lock.
  **Verified by the skill:** niri v26.04 `src/niri.rs` `update_keyboard_focus` and
  `src/ipc/server.rs`: focus `None` is reported for every non-layout keyboard focus (layer shell,
  built-in `Mru` switcher, overview, lock screen, screenshot UI, exit dialog, empty layout).
- **Resolution:** D6 now requires the landing window to differ from the departing one. Every
  "went away and came back" case is covered by a new alternate flow, and niri's built-in alt-tab
  switcher turns out to pass through the same state and so unstashes as well. The residual case
  (a different sliver gaining focus for a reason other than alt-tab) stays an accepted limitation.
- **Resolved by:** evidence
- **Affected decisions:** D6
- **Affected tech-notes:** T2
- **Changed in spec:** Alternate Flows and States; Edge Cases and Failure Modes; Coordinations

### F5: Press-versus-event ordering, what "complete" means, and partial failure

- **Agent:** edge-case-explorer
- **Reviewer identifiers:** EC-5
- **Finding:** The hotkey socket and the event stream are independent, so a press could act on a
  stale picture (wrong workspace, missed new window). "Requests complete before the next press" was
  undefined. Nothing said what happens when one window closes or niri declines one action mid-press.
- **Resolution:** A press acts on a fresh snapshot requested from niri when it arrives (new T5); a
  press is complete when every window in it has settled (T3); a failed window is skipped and the
  rest proceeds with state saved and the client still succeeding.
- **Resolved by:** evidence
- **Affected decisions:** D12
- **Affected tech-notes:** T3, T5
- **Changed in spec:** Primary Flow; Edge Cases and Failure Modes; Coordinations

### F6: A forgotten sliver re-stashed by the hotkey loses its original size for good

- **Agent:** edge-case-explorer, junior-developer
- **Reviewer identifiers:** EC-6 (first part), JD-4
- **Finding:** Under D7 a manual change discards the original. The next press stashes the (still
  tiny) window again and records the sliver as the geometry to restore, so the real size is
  unrecoverable. D14 (bar toggle) does this to every sliver at once. The request said "marks window
  as unstashed" and did not say the original is discarded.
- **Resolution:** Escalated to the user (E1). The user chose "pop back to original size": a manual
  move restores the original size at the new position and stops tracking; a manual resize forgets
  the stash as before. D14 inherits the rule, so a bar toggle makes every sliver reappear at full
  size with nothing lost.
- **Resolved by:** user input
- **Affected decisions:** D7, D14
- **Affected tech-notes:** T3
- **Changed in spec:** Alternate Flows and States; Edge Cases and Failure Modes

### F7: A stashed window that is tiled and re-floated comes back as a sliver

- **Agent:** edge-case-explorer
- **Reviewer identifiers:** EC-6 (second part)
- **Finding:** The draft forgot a stashed window when it became tiled. The reviewer believed niri
  remembers a window's last floating size, which would be the sliver, so re-floating would hand back
  a sliver with the original already discarded.
- **Unverified (reviewer):** niri's floating-geometry memory. **Verified by the skill:** niri v26.04
  `src/layout/tile.rs` `floating_window_size` / `floating_pos`, restored in
  `src/layout/floating.rs` `add_tile_at`.
- **Resolution:** The daemon keeps the original while the window is tiled and restores it the
  moment the window returns to floating. New alternate flow.
- **Resolved by:** evidence
- **Affected decisions:** D13
- **Affected tech-notes:** T4
- **Changed in spec:** Alternate Flows and States; Edge Cases and Failure Modes

### F8: Fullscreen, maximized, and overview states were not mentioned

- **Agent:** edge-case-explorer
- **Reviewer identifiers:** EC-7
- **Finding:** The spec was silent on fullscreen or maximized floating windows and on the hotkey
  while the overview is open.
- **Unverified (reviewer):** niri's fullscreen-in-floating and overview focus behavior. **Verified
  by the skill:** niri v26.04 `src/layout/workspace.rs`: "Floating windows are never fullscreen"; a
  fullscreened floating window moves to the scrolling layout with `restore_to_floating`. The
  overview takes keyboard focus (`KeyboardFocus::Overview`), reported as no focused window.
- **Resolution:** Fullscreen is covered by the tile-and-refloat flow (F7). Maximized floating
  windows are ordinary large windows and get a row. The overview gets a row: the press acts as
  usual, and closing the overview onto a different sliver unstashes it under D6.
- **Resolved by:** evidence
- **Affected decisions:** D13
- **Affected tech-notes:** T4
- **Changed in spec:** Alternate Flows and States; Edge Cases and Failure Modes

### F9: Slivers on the same edge can overlap, contradicting D4's rationale

- **Agent:** edge-case-explorer
- **Reviewer identifiers:** EC-8
- **Finding:** D4 claimed keeping the position along the edge keeps slivers apart. Windows that
  overlapped along that axis produce overlapping slivers; the lower one is unreachable by pointer.
- **Resolution:** D4's rationale corrected; edge-case row added stating the hidden sliver still
  comes back by hotkey or alt-tab; spreading slivers apart deferred under YAGNI with a trigger.
- **Resolved by:** evidence
- **Affected decisions:** D4
- **Affected tech-notes:** —
- **Changed in spec:** Edge Cases and Failure Modes; Deferred (YAGNI)

### F10: The niri-restart flow and the saved state file disagree

- **Agent:** edge-case-explorer, junior-developer
- **Reviewer identifiers:** EC-9, JD-5
- **Finding:** "Drops all state" on niri restart did not say the saved file is cleared, and the
  runtime directory survives until logout. A new niri instance reuses window ids, so stale entries
  could attach to unrelated windows.
- **Unverified (reviewer):** whether niri reuses window ids across restarts. **Verified by the
  skill:** niri v26.04 `src/window/mapped.rs` `MAPPED_ID_COUNTER` is a process-global counter, so
  ids restart with the process.
- **Resolution:** The daemon discards its saved state when the stream closes and tags saved state
  with the niri instance identity; a fresh daemon ignores state from a different instance.
- **Resolved by:** evidence
- **Affected decisions:** D10, D11
- **Affected tech-notes:** T4
- **Changed in spec:** Alternate Flows and States; Edge Cases and Failure Modes

### F11: A stashed window moved to another monitor may be forgotten by D7

- **Agent:** edge-case-explorer, junior-developer
- **Reviewer identifiers:** EC-10, JD-7
- **Finding:** D13 said a moved window stays stashed; a destination with a different usable area
  makes niri re-clamp it, which D7 reads as a manual change. The winning rule was unstated.
- **Resolution:** Row split: same-monitor moves stay stashed; a move to a monitor with a different
  usable area counts as a manual change and is marked untested.
- **Resolved by:** evidence
- **Affected decisions:** D13, D15
- **Affected tech-notes:** —
- **Changed in spec:** Edge Cases and Failure Modes

### F12: "Most recently used" focus on unstash relied on a debounced signal

- **Agent:** edge-case-explorer, junior-developer
- **Reviewer identifiers:** EC-11, JD-9 (`Category: YAGNI candidate`, simpler version)
- **Finding:** D8's recency choice cited niri's focus timestamp, which T2 says skips briefly focused
  windows, leaving the pick unspecified. JD-9 proposed a simpler rule.
- **Resolution:** Recency now comes from the daemon's own focus history, which it keeps anyway for
  the alt-tab detector, with a fallback to the first window restored. The user's chosen behavior
  (most recently used) is kept; the mechanism is simpler and always defined.
- **Resolved by:** evidence
- **Affected decisions:** D8
- **Affected tech-notes:** T2
- **Changed in spec:** Alternate Flows and States

### F13: Mechanics leaking into the spec

- **Agent:** edge-case-explorer, junior-developer
- **Reviewer identifiers:** EC-12, JD-10
- **Finding:** Coordinations described event pairing and attribution ("two consecutive focus events
  form the alt-tab signal"); the reviewers also questioned the 10 and 75 pixel figures and the
  naming of systemd and the socket.
- **Resolution:** Coordinations rows reworded as outcomes with the mechanics left to T2 and T3. The
  pixel figures stay: they are what the user sees on screen, which is the test the spec-content rule
  applies. systemd and the socket are named in the work item itself and stay. At synthesis, the
  remaining leaks were rewritten: "logical pixels" in Primary Flow step 5 became "pixels", and the
  "Session runtime storage" Coordinations row became "The daemon's saved stash state"; the storage
  location stays in T4.
- **Resolved by:** evidence
- **Affected decisions:** —
- **Affected tech-notes:** —
- **Changed in spec:** Primary Flow; Coordinations

### F14: Multi-monitor rows specified for hardware that does not exist

- **Agent:** edge-case-explorer, junior-developer
- **Reviewer identifiers:** EC-13, JD-12 (`Category: YAGNI candidate`)
- **Finding:** The second-monitor row and the "monitor added, removed, or rescaled" enumeration
  rest on no evidence; one output is connected.
- **Resolution:** Rows reduced to one-line consequences marked "not tested"; the D14 row now gives
  the bar as its example; Out of Scope states nothing is built or tested for a second monitor.
- **Resolved by:** evidence
- **Affected decisions:** D14, D15
- **Affected tech-notes:** —
- **Changed in spec:** Edge Cases and Failure Modes; Out of Scope

### F15: No definition of done, and the setup work had no owner

- **Agent:** junior-developer
- **Reviewer identifiers:** JD-11
- **Finding:** No acceptance checks; nobody owned adding the keybind and installing the user
  service; the direction-of-travel answer said the keybind config "stays as it is" while the feature
  needs a new binding.
- **Resolution:** Outcome gained a "done looks like" checklist; the user service and the keybind are
  named as deliverables (D11); Out of Scope states the config gains one keybind and loses nothing.
  The boundary record is unchanged because the answer recorded there is the user's own wording.
- **Resolved by:** evidence
- **Affected decisions:** D11
- **Affected tech-notes:** —
- **Changed in spec:** Outcome; Out of Scope

### F16: The daemon could be dead when the hotkey is first pressed

- **Agent:** junior-developer
- **Reviewer identifiers:** JD-13
- **Finding:** If the daemon starts before niri is ready and exits, repeated fast exits could trip the
  service manager's start limit.
- **Resolution:** New alternate flow: the daemon keeps retrying the connection at startup rather than
  exiting. Operational tuning of the unit belongs to implementation planning.
- **Resolved by:** evidence
- **Affected decisions:** D11
- **Affected tech-notes:** —
- **Changed in spec:** Alternate Flows and States; Coordinations

## Minor edits

- F17: Note in User Interactions that a focused sliver receives keystrokes under focus-follows-mouse — junior-developer (JD-8) — User Interactions

## Escalation register

### E1: When you move a sliver by hand, should it pop back to its original size, or stay tiny and lose that size for good?

- **Answer:** "Pop back to original size." A move that leaves the size alone restores the original
  size at the new position and stops tracking; a hand resize forgets the stash and restores nothing;
  a bar toggle pops every sliver back to full size.
- **Landed in:** D7 (retitled), D14 (retitled), T3; spec sections Outcome, Alternate Flows and
  States (two flows replace "You move or resize a stashed window yourself"), Edge Cases and Failure
  Modes, Deferred (YAGNI).

## Unaudited evidence classes

- niri v26.04 source citations (file and function) added to the decision log and technical notes
  while resolving F1, F3, F4, F7, F8, and F10 were read by the skill after both reviewers had
  returned, so neither reviewer saw them. Affected decisions: D6, D7, D8, D10, D11, D13.
