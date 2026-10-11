# Feature Technical Notes: Floating Window Stash for niri

<!--
This file captures implementation mechanics that are load-bearing for the behavioral specification
of the floating window stash: places where naming a specific mechanic was the only way to specify a
behavior correctly, and the mechanic is not discoverable from the code repo (the repo is new).
Behavioral statements live in [../feature-specification.md](../feature-specification.md).

Cross-referencing invariants:
- `Supports decisions:` — D# IDs from [decision-log.md](decision-log.md).
- `Driven by findings:` — F# IDs from [team-findings.md](team-findings.md). `—` if captured in the interview.
- `Referenced in spec:` — sections of [../feature-specification.md](../feature-specification.md) that cite it.

niri source citations refer to the v26.04 tag of https://github.com/YaLTeR/niri, read on 2026-10-11,
which matches the installed `niri 26.04 (8ed0da4)`.
-->

## T1: niri's off-screen limit and coordinate frame

> **Amended 2026-10-11 (A1):** the daemon no longer resizes. A stash is one push toward the edge that niri clamps; a restore is one nudge back; any hand change to a stashed window forgets it. Sentences below about resizing, pop-back, or telling a move from a resize describe the first build.

- **Context:** Primary Flow steps 4 and 5, and the edge cases for windows that cannot shrink and for
  usable-area changes. The spec promises a roughly 10-pixel sliver at the edge of the usable area,
  and a strip of up to 75 pixels when the application refuses the small size. Both numbers come from
  niri, not from the daemon.
- **Technical detail:** niri's floating layout (`src/layout/floating.rs`,
  `Data::recompute_logical_pos`) clamps every floating window's position so that at least
  `clamp(size / 4, 10, 75)` logical pixels stay on screen on each axis independently. With a
  32-pixel dimension that floor is 10 pixels; with a 300-pixel dimension it is 75. The clamp is
  applied relative to the **working area** (output minus exclusive layer-shell zones such as the
  bar), not the full output. Positions are stored as fractions of the working area and re-clamped
  whenever the window size or the working area changes (`Data::update`, `Data::update_config`).
  Position requests (`move-floating-window`, `PositionChange::SetFixed`) are in logical pixels
  relative to the working area. Size requests (`set-window-width` / `set-window-height`,
  `SizeChange::SetFixed`) are clamped by niri to the window's advertised min/max size before being
  sent (`FloatingSpace::set_window_width`, `ensure_min_max_size`), and are sent as a one-time size
  request the application may still answer differently. The daemon therefore stashes by first
  resizing, then requesting a position well past the edge and letting niri clamp it; the geometry
  niri reports once the window settles (T3) is the "stashed geometry". Reported positions
  (`WindowLayout.tile_pos_in_workspace_view`) are rounded to physical pixels, which at the current
  scale of 1.75 means fractional logical values. Confirmed live (user's event log 1, 2026-10-11): a
  32 px wide window pushed left is reported at x = -22.3 (10 px visible); a 900 px window pushed right
  shows 73.7 px; a scripted resize or move is reported once or twice with no animation, and a push
  against an edge the window already touches is not reported at all.
- **Supports decisions:** D3, D4, D14, D16
- **Driven by findings:** F1; review F25
- **Referenced in spec:** Primary Flow; Edge Cases and Failure Modes

## T2: How alt-tab is recognised

- **Context:** The alternate flows "Alt-tab into a stashed window", "Focus returns to the window it
  came from", and "Focus moves to a sliver without passing through no window". The spec's rule is
  that only a focus change that passes through "no window focused" and lands on a different window
  than the one it left unstashes.
- **Technical detail:** niri's event stream emits `WindowFocusChanged { id: None }` whenever keyboard
  focus is not on a toplevel window (`src/ipc/server.rs`, the "extra check for focus becoming
  None"). That is the case for every non-layout keyboard focus in `src/niri.rs`
  `update_keyboard_focus`: a layer-shell surface with keyboard interactivity (noctalia's picker,
  launcher, and panels), niri's own built-in recent-windows switcher (`KeyboardFocus::Mru`), the
  overview, the lock screen, the screenshot UI, the exit dialog, and a layout with no focused
  window (an empty workspace). Confirmed live for the built-in switcher, the lock screen and the
  overview, including the return to the same window on cancel or unlock (user's event logs 2, 4, 5,
  2026-10-11); the user's everyday alt-tab is niri's built-in switcher. The daemon keeps the last two focus events and acts only on the
  triple (`Some(a)`, `None`, `Some(b)`) where `b` is stashed and `b != a`. When `b == a` focus
  returned to where it was (picker cancelled, overview closed, screen unlocked, empty-workspace
  round trip) and nothing happens. `WindowOpenedOrChanged` with `is_focused: true` also implies a
  focus change and is folded into the same tracking, except when it names the window that already
  holds focus: niri re-reports the focused window during a drag (user's event log, 2026-10-11), and
  that is not a transition. Focus changes the daemon itself requests (D8)
  are registered as expected before the request is sent and are not fed to this detector. The
  debounced `WindowFocusTimestampChanged` event and `Window.focus_timestamp` are not used for
  detection or for the "most recently focused" choice in D8; the daemon's own observed focus
  history serves both. `focus-window --id` from a script moves focus window-to-window without a
  `None` step and so never unstashes.
- **Supports decisions:** D6, D8
- **Driven by findings:** F3, F4, F12; review F25
- **Referenced in spec:** Alternate Flows and States; Coordinations

## T3: Telling the daemon's own moves from yours

> **Amended 2026-10-11 (A2):** there is no settling period. The daemon reads the window list back right after its moves, because niri applies a move before answering, and treats that position as the resting one. Any later differing report is yours.

> **Amended 2026-10-11 (A1):** the daemon no longer resizes. A stash is one push toward the edge that niri clamps; a restore is one nudge back; any hand change to a stashed window forgets it. Sentences below about resizing, pop-back, or telling a move from a resize describe the first build.

- **Context:** Primary Flow step 6, the alternate flow "You move or resize a stashed window
  yourself", the quick-double-press row, and the late-resize row. The spec promises that only
  changes the daemon did not request forget the stash, and that a press is complete only when its
  windows have settled. niri attaches no cause to layout changes, and an application may answer a
  resize with a different size than asked.
- **Technical detail:** Every resize and move, including the daemon's own, arrives as
  `WindowLayoutsChanged` with no cause attached (niri-ipc 26.4.0 `Event::WindowLayoutsChanged`).
  Size requests are sent once and answered asynchronously by the application (niri v26.04
  `src/layout/floating.rs`, `FloatingSpace::set_window_width`, `request_size_once`). For each window it acts on, the daemon opens a **settling period**: it
  issues its resize and move requests, then consumes every layout event for that window as
  self-caused until the window's reported size and position have been stable for a short quiet
  interval (on the order of 100–200 ms) or an overall cap (on the order of 500 ms–1 s) has passed.
  These figures are design estimates, not measurements; implementation planning should confirm
  them against a real terminal resize on this machine.
  The final reported geometry becomes the stashed (or restored) geometry and is what later events
  are compared against. After the settling period, a layout event whose geometry differs from the
  expectation by more than a rounding tolerance of about one logical pixel on each value is a
  manual change. A hand drag is recognised explicitly: niri reports the picked-up window as
  `WindowOpenedOrChanged` with `workspace_id: None` and `tile_pos_in_workspace_view: None` while it
  is held, and again with a workspace and a position when it is dropped (user's event log,
  2026-10-11). While held, the window is left alone and stays tracked; the drop report carries the
  geometry the decision rests on. A keybind move to another workspace is not a lift: niri reports the
  window once, on the new workspace, at the same position (user's event log 3). A hand resize is not
  bracketed that way either: the same log shows a corner drag as a stream
  of per-frame `WindowLayoutsChanged` with the position fixed and the size changing, so for it, and
  for any change not bracketed by a lift and a drop, the daemon waits for the quiet interval of no
  further change and judges the stream once at its end. Then: a change in position with the size
  unchanged (within tolerance) is a manual move, and the daemon answers it with a size-restore
  request (which opens its own settling period) and drops the window from tracking; any change in
  size is a manual resize, and the daemon drops the window from tracking without acting. Because applications answer size requests asynchronously, with rounding (terminal
  cell grids) or minimum-size substitution (D16), the expectation is always the settled reported
  size, never the requested one. Commands and events are processed on one sequential path so that
  the settling period is open before the resulting events are examined. A hotkey press is complete
  when every window in it has settled; the next press is not handled until then. Exact interval
  values are an implementation choice; the behavior the spec commits to is that nothing within the
  settling period is a manual change and that a late answer after it is.
- **Supports decisions:** D7, D12, D14, D16
- **Driven by findings:** F1, F5, F6; review F25
- **Referenced in spec:** Primary Flow; Alternate Flows and States; Coordinations; Edge Cases and Failure Modes

## T4: What survives a restart

- **Context:** The alternate flows "The daemon restarts" and "niri restarts", Primary Flow step 4,
  and the state-file rows in Edge Cases. The spec promises that originals are never lost to a daemon
  crash within one login session, and that nothing from a previous niri instance is reused.
- **Technical detail:** niri window ids come from a process-global counter
  (`src/window/mapped.rs`, `MAPPED_ID_COUNTER`), so they restart from the beginning with each niri
  instance and will collide with ids from the previous one. The daemon stores its state in the
  session runtime directory (`$XDG_RUNTIME_DIR`), which the session clears at logout, tagged with
  the identity of the niri instance it belongs to (the niri socket path `$NIRI_SOCKET` embeds the
  Wayland display and niri's pid, e.g. `niri.wayland-1.1572.sock`). The file is a statement of
  **desired state**, not observed state. Per window id it holds the original size and position and
  one of two desired states: `stashed` with the chosen edge, or `restored`. It is written before the
  first niri command of any operation that changes it (a press, an alt-tab unstash, a pop-back) and
  again when an entry is dropped. Settled geometry (T3) lives only in memory; it is re-learned by
  re-applying. At startup the daemon reads the file, discards it entirely if its instance tag does
  not match the current niri instance, fetches the current window list, drops entries whose window
  is absent, and re-applies the desired state to every remaining floating window through the normal
  stash or restore path with its settling period: `stashed` entries are stashed again (idempotent:
  a window already at 32 px against its edge produces no visible change), `restored` entries are
  restored and then dropped. Entries whose window is currently tiled or fullscreen keep only the
  original for the re-float rule (D13). No comparison against a remembered geometry and no
  inference about what a crash interrupted is needed, and anything done to a sliver while the daemon
  was down is undone by the re-apply. If the desired-state write at the start of a press fails, the
  press is refused before any command is sent. The file format is private to the daemon and is not
  a cross-component contract.
- **Supports decisions:** D10, D11, D13
- **Driven by findings:** F2, F7, F8, F10; review F19, F20, F21
- **Referenced in spec:** Primary Flow; Alternate Flows and States; Coordinations; Edge Cases and Failure Modes

## T5: A press acts on a fresh snapshot

- **Context:** Primary Flow step 2 and the "hotkey right after a workspace switch" row. The spec
  promises that a press acts on what is on screen at the moment of the press.
- **Technical detail:** The event stream and the hotkey socket are two independent channels, so when
  a press arrives the daemon may not yet have consumed events niri already emitted (a workspace
  switch, a window opening or floating). Rather than trusting its event-fed picture, the daemon
  answers a press by issuing fresh `Windows` and `Workspaces` requests to niri on the request
  socket; niri answers from its current state, which by construction includes everything it has
  already emitted on the stream (niri v26.04 `src/ipc/server.rs`, `process`: the `Windows` and
  `Workspaces` replies read the same `event_stream_state` that the event stream is generated
  from). The daemon then reconciles its stash state against that answer
  (windows gone, windows no longer floating) before acting. A window that vanishes between the
  snapshot and the action cannot be detected from niri's reply: every `Request::Action` is answered
  `Response::Handled` after a path-only `validate_action`, and the by-id actions silently do nothing
  for an id that no longer exists (niri v26.04 `src/ipc/server.rs` 381–399, `src/input/mod.rs`
  ~1986–1992). The daemon therefore detects it from the stream and a second snapshot: a
  `WindowClosed` for a window in the press removes it from the press and from state; a
  `WindowOpenedOrChanged` showing it no longer floating removes it from the press and keeps only
  its original (D13); and at the end of the press the daemon re-fetches `Windows` and drops any
  press member that is absent. The re-fetch is the backstop, so the order in which stream events
  reach the daemon relative to its own actions does not matter. An `Err` from the request socket
  itself is a transport failure (niri gone), handled by D11, not a per-window failure. A reply of
  `Ok(Err(message))` is logged as "niri declined" and the window is skipped; in v26.04 it only
  arises for screenshot-path validation and never for stash actions.
- **Supports decisions:** D12
- **Driven by findings:** F5; review F18
- **Referenced in spec:** Primary Flow; Coordinations; Edge Cases and Failure Modes
