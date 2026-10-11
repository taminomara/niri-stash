# Decision Log: Floating Window Stash for niri

<!--
This file records every decision settled while specifying the floating window stash. Behavioral
statements live in [../feature-specification.md](../feature-specification.md); this file captures
the history, rationale, evidence, and rejected alternatives for each decision.

Decisions were classified as full or trivial once, in the synthesis step after the review round.
All sixteen met at least one full signal, so every decision stays in full form. D# numbers never
change.

Cross-referencing invariants:
- `Linked technical notes:` — T# IDs from [feature-technical-notes.md](feature-technical-notes.md).
- `Driven by findings:` — F# IDs from [team-findings.md](team-findings.md). `—` if settled in the interview.
- `Dependent decisions:` — D# IDs of later decisions that rested on this one.
- `Referenced in spec:` — sections of [../feature-specification.md](../feature-specification.md) that cite it.
-->

## Trivial decisions

None. Every decision met at least one full-form signal at synthesis: a plausible rejected
alternative, evidence beyond the request (niri source, niri-ipc docs, the user's config files or
systemd units), or a non-empty `Driven by findings:`, `Linked technical notes:`, or
`Dependent decisions:` field. D15 and D9, the closest candidates, are kept full because D15 is
driven by F11 and F14, and D9 rests on the user's niri window rules as well as the interview
answer.

## Full decisions

### D1: A stash acts on the focused workspace only

- **Question:** Which windows does one hotkey press affect?
- **Decision:** Only floating windows on the workspace that has keyboard focus when the hotkey fires.
  Tiled windows, other workspaces, and other monitors are never touched.
- **Rationale:** The request says "all floating windows on the current workspace". Scoping to the
  focused workspace keeps stash state per workspace without the daemon tracking which workspace you
  are looking at on each monitor.
- **Evidence:** User request (scope-boundary.md, Stated Scope). niri reports a single focused
  workspace and per-window floating state and workspace id through its event stream (niri-ipc 26.4.0
  `Workspace.is_focused`, `Window.is_floating`, `Window.workspace_id`).
- **Rejected alternatives:**
  - All floating windows on every workspace of the focused monitor — rejected because the request
    says current workspace, and windows you cannot see would move without you noticing.
- **Linked technical notes:** —
- **Driven by findings:** —
- **Dependent decisions:** D2, D13, D15
- **Referenced in spec:** Primary Flow

### D2: One hotkey toggles between stash and unstash

- **Question:** What does a press do when some windows are stashed and some are not?
- **Decision:** If any floating window on the focused workspace is unstashed, the press stashes every
  unstashed one and leaves already-stashed windows as they are. Only when none is unstashed does the
  press restore all of them.
- **Rationale:** Direct from the request. The asymmetry means a window that came back via alt-tab,
  or a newly opened floating window, is tucked away by the next press rather than causing everything
  to pop back out.
- **Evidence:** User request: "when stash command is activated, and there are unstashed windows on
  the current workspace, they are stashed; ... no unstashed windows ... all windows are unstashed".
- **Rejected alternatives:**
  - Strict toggle of the whole set (stash all, then unstash all, alternating) — rejected because the
    request defines the asymmetric rule and the strict toggle would unstash everything whenever one
    window was manually brought back.
- **Linked technical notes:** —
- **Driven by findings:** —
- **Dependent decisions:** D8, D12
- **Referenced in spec:** Primary Flow; Alternate Flows and States; Edge Cases and Failure Modes

### D3: Each window goes to the nearest edge of the usable area

- **Question:** Which edge does a window stash to, and what is "the edge"?
- **Decision:** The edge of the usable screen area (the monitor minus the bar) whose distance from
  the window's center is smallest. All four edges are candidates. Ties resolve in the order left,
  right, top, bottom. Implementation note (plan D-8): the distance is computed from the monitor's
  edges, which is what the request said, because the bar's height is not readable from niri; the
  sliver still lands against the usable area, and the two differ only for a window within the bar's
  height of a tie.
- **Rationale:** The request names the window center and the closest edge. niri measures floating
  windows against the working area rather than the full monitor, so "edge of the monitor" is really
  the edge of the usable area, and a window pushed to the top ends up against the bar. A fixed tie
  order keeps repeated stashes deterministic.
- **Evidence:** User request: "the exact edge of the monitor where the window belongs is the closest
  to the window's center". niri v26.04 `src/layout/floating.rs`, `Data::recompute_logical_pos`:
  positions are stored and clamped relative to `working_area`. See T1.
- **Rejected alternatives:**
  - Left and right edges only — rejected because the request says "closest edge" with no
    restriction, and a window hugging the bottom of a short-and-wide layout is better tucked
    downward.
  - Measure against the full monitor rather than the usable area — rejected because niri will not
    place a floating window behind the bar, so the request could not be honored literally.
- **Linked technical notes:** T1
- **Driven by findings:** —
- **Dependent decisions:** D4, D15, D16
- **Referenced in spec:** Primary Flow; Edge Cases and Failure Modes

### D4: Stashed geometry is a 32-pixel sliver pushed to the edge

- **Question:** What size and position does a stashed window take?
- **Decision:** The dimension perpendicular to the chosen edge becomes 32 logical pixels (width for
  left and right, height for top and bottom). The other dimension and the position along the edge
  are kept. The window is then pushed over the edge until niri stops it, which leaves about 10
  pixels visible.
- **Rationale:** The request names 32 pixels and relies on niri's refusal to let a window go fully
  off screen. niri keeps a quarter of the window dimension visible, clamped to between 10 and 75
  pixels, so 32 pixels gives the 10-pixel floor. Keeping the position along the edge keeps slivers
  apart when the windows were apart along that edge; windows that already overlapped along the edge
  produce overlapping slivers, and the hidden one stays reachable by hotkey or alt-tab (spreading
  them apart is deferred).
- **Evidence:** User request: "if we make shrink width of a window to 32px, it will only stick out
  horizontally a bit". niri v26.04 `src/layout/floating.rs`, `Data::recompute_logical_pos`:
  `min_on_screen_hor = clamp(size.w / 4, 10, 75)`. See T1.
- **Rejected alternatives:**
  - Shrink both dimensions — rejected because only the dimension facing the edge affects how much
    sticks out, and shrinking the other would make the sliver harder to find along the edge.
  - Collapse all slivers to one corner — rejected because every sliver would then hide every other.
- **Linked technical notes:** T1
- **Driven by findings:** F9
- **Dependent decisions:** D5, D16
- **Referenced in spec:** Primary Flow; Edge Cases and Failure Modes

### D5: Unstash restores the recorded size and position

- **Question:** Where does a window go when it comes back?
- **Decision:** To the exact size and position the daemon recorded immediately before stashing it.
  If that geometry no longer fits, niri clamps it and the window lands wherever niri allows.
- **Rationale:** Direct from the request. Recording at stash time is the only moment the daemon
  knows the geometry you chose.
- **Evidence:** User request: "when a window is unstashed, its original position and size is
  restored".
- **Rejected alternatives:**
  - Restore to a centered default — rejected because the request asks for the original geometry.
- **Linked technical notes:** —
- **Driven by findings:** —
- **Dependent decisions:** D7, D10
- **Referenced in spec:** Alternate Flows and States; Edge Cases and Failure Modes

### D6: A focus change through "no window" unstashes; a direct one does not

- **Question:** How does the daemon tell alt-tab from focus-follows-mouse?
- **Decision:** When niri reports that no window has focus and the very next focus change lands on a
  stashed window **other than the window that had focus before**, that one window is unstashed.
  When focus returns to the same window it left (picker cancelled, overview closed, screen unlocked,
  empty-workspace round trip), nothing happens even if that window is a sliver. When focus moves
  directly from one window to a stashed window, nothing happens. Focus reaching a different sliver
  through "no window" for any other reason also unstashes it; the daemon does not try to tell those
  apart.
- **Rationale:** noctalia's window picker takes keyboard focus while open, which niri reports as no
  focused window, so alt-tab always passes through that state. Focus-follows-mouse never does. The
  request spells this rule out, and it is the only signal available: niri does not report pointer
  position (niri-ipc 26.4.0 `Request` and `Event` enums have no pointer-position request or
  event), and the focus timestamp is debounced and unsuitable. Review showed that "no window
  focused" also occurs for the overview, the lock screen, niri's built-in alt-tab switcher, and
  empty workspaces. Requiring the landing window to differ from the departing one removes every
  "went away and came back" case without touching the alt-tab case, because alt-tabbing to the
  window you are already on is a no-op. niri's built-in switcher passes through the same state, so
  it unstashes too.
- **Evidence:** User request, paragraph beginning "Window should be unstashed if niri's focused
  window goes to None". niri-ipc 26.4.0 `Event::WindowFocusChanged { id: Option<u64> }` with
  `None` when no window is focused; `Window.is_focused` docs: "sometimes none is (for example, when
  a layer-shell surface has focus)". `Event::WindowFocusTimestampChanged` docs state the timestamp
  only updates after a debounce. niri v26.04 `src/niri.rs` `update_keyboard_focus` and
  `src/ipc/server.rs` (focus `None` for every non-layout keyboard focus: layer shell, `Mru`,
  `Overview`, `LockScreen`, `ScreenshotUi`, `ExitConfirmDialog`, empty layout). See T2.
- **Rejected alternatives:**
  - Unstash on any focus landing on a stashed window — rejected because focus-follows-mouse is on
    (`~/.config/niri/cfg/input.kdl`, `focus-follows-mouse max-scroll-amount="0%"`) and the pointer brushing a sliver would pop it out.
  - Unstash all stashed windows when one is alt-tabbed — rejected because the request says "unstash
    it", singular, and popping out the others would defeat tucking them away.
  - Unstash on every (none, stashed) pair regardless of the departing window (the draft rule) —
    rejected because cancelling the picker or unlocking the screen while a sliver had focus would
    pop it out.
- **Linked technical notes:** T2
- **Driven by findings:** F3, F4
- **Dependent decisions:** D8
- **Referenced in spec:** Alternate Flows and States; Edge Cases and Failure Modes; User Interactions

### D7: A manual move pops the window back; a manual resize forgets it

- **Question:** What happens when you drag or resize a sliver yourself?
- **Decision:** A manual change of position alone restores the window's original size at the new
  position and stops tracking it. A manual change of size forgets the stash and leaves the window
  exactly as you left it, with its recorded geometry discarded. The same applies when the
  application changes its own window after it has settled. Every stash and unstash has a short
  settling period during which
  nothing the window does counts as a manual change, including an application answering the resize
  with a size of its own; the geometry at the end of that period is the one later changes are
  compared against. An application answer that arrives after the settling period is a manual
  change.
- **Rationale:** The request says a manual change "marks window as unstashed" and does not say what
  size the window keeps. Review showed that leaving a dragged sliver tiny loses the original size for
  good once the next press records the strip as the original, and that a bar toggle does this to
  every sliver at once. Popping a moved sliver back to its original size keeps the request's rule
  (it is unstashed, and the daemon stops tracking it) while losing nothing. A hand resize is kept
  as the user's choice of size.
- **Evidence:** User request: "It also marks window as unstashed if user modifies its size/position
  manually". User input, escalation E1: "Pop back to original size". niri-ipc 26.4.0 `Event::WindowLayoutsChanged` reports every layout change with no
  cause. niri v26.04 `src/layout/floating.rs` `set_window_width`: the request is clamped to the
  window's min/max and sent once (`request_size_once`); the application answers asynchronously.
  See T3.
- **Rejected alternatives:**
  - Leave a dragged sliver tiny and discard the original (the draft rule, the request read
    literally) — rejected by the user after review showed the original is then unrecoverable.
  - Leave it tiny but keep the original on file for the window's lifetime — rejected by the user;
    more bookkeeping for the same protection.
  - Snap the sliver back to the edge, refusing manual moves — rejected because it contradicts the
    request.
  - Compare against the requested geometry instead of the settled one — rejected because terminals
    round to cell grids and minimum sizes substitute, so the first stash of a terminal would read as
    a manual change.
- **Linked technical notes:** T3
- **Driven by findings:** F1, F6
- **Dependent decisions:** D12, D13, D14, D16
- **Referenced in spec:** Primary Flow; Alternate Flows and States; Edge Cases and Failure Modes

### D8: The hotkey moves keyboard focus in both directions

- **Question:** Where is keyboard focus after a stash or an unstash?
- **Decision:** After a stash, if the window that had focus was stashed, focus moves to the tiled
  layout; if the workspace has no tiled window, focus stays on the sliver because niri has nowhere
  else to put it. After a hotkey unstash, focus moves to the one of the restored windows the daemon
  most recently saw focused, or to the first one restored if it has seen none of them focused.
  Alt-tab unstash leaves focus where alt-tab put it.
- **Rationale:** Without this, stashing a focused terminal leaves you typing into a 10-pixel sliver,
  and unstashing leaves you clicking before you can type. The request describes this as a scratch-workspace substitute, and i3 and sway scratchpads
  focus the window they show; that comparison is supporting context only (not checked against
  i3 or sway documentation in this run), and the decision rests on the user's interview answer.
- **Evidence:** User input, interview question 1: "Move focus both ways". niri provides a focus
  action by window id and an action to move focus to the tiling layout (`niri msg action` list:
  `focus-window`, `switch-focus-between-floating-and-tiling`, `focus-tiling`). niri v26.04
  `src/layout/workspace.rs` `switch_focus_floating_tiling`: with an empty scrolling layout the call
  returns without changing focus. Recency comes from the daemon's own observed focus events, not
  from the debounced `Window.focus_timestamp`.
- **Rejected alternatives:**
  - Fix only the stash side — rejected by the user.
  - Never touch focus — rejected by the user.
  - Pick "most recent" from niri's focus timestamp — rejected because that timestamp deliberately
    skips briefly focused windows; the daemon's own focus history is simpler and always populated
    for windows focused since it started.
- **Linked technical notes:** T2
- **Driven by findings:** F3, F12
- **Dependent decisions:** —
- **Referenced in spec:** Primary Flow; Alternate Flows and States; User Interactions

### D9: Every floating window is stashed, with no exemptions

- **Question:** Should picture-in-picture players, toasts, or other floating windows be skipped?
- **Decision:** No. Every floating window on the focused workspace is stashed. A per-application
  exemption list is deferred.
- **Rationale:** The niri config floats Zen picture-in-picture and Steam toasts in corners, which
  makes stashing them plausible friction, but there is no evidence yet that it is. An exemption list
  means a config file from day one.
- **Evidence:** User input, interview question 2: "No exemptions for now". `~/.config/niri/cfg/rules.kdl`
  rules for `app.zen_browser.zen` Picture-in-Picture and `steam` toasts.
- **Rejected alternatives:**
  - Ship a small built-in exemption list from the niri config — rejected by the user.
  - Skip windows that opened without focus — rejected by the user; relies on an opening-time signal.
- **Linked technical notes:** —
- **Driven by findings:** —
- **Dependent decisions:** —
- **Referenced in spec:** Deferred (YAGNI)

### D10: Stash state survives a daemon restart within the session

- **Question:** Does the daemon remember stashed windows after it restarts?
- **Decision:** Yes, within one login session. The saved state is a statement of desired state: for
  every tracked window, its original geometry and whether it should currently be stashed (at which
  edge) or restored. It is written before any command is sent to niri. At startup the daemon reads
  it, drops windows that no longer exist, and re-applies the desired state to every remaining
  window: stash again what should be stashed, restore and forget what should be restored. Nothing
  is adopted, inferred, or compared against a remembered settled geometry; whatever happened to a
  sliver while the daemon was down is undone. If the desired state cannot be written when a press
  begins, nothing moves and the hotkey client reports an error. Saved state from a different niri
  instance is discarded. If the saved state is missing or unreadable, the daemon starts empty and
  logs the problem.
- **Rationale:** You will restart the daemon often while developing it, and the service manager
  restarts it after a crash. Without memory, every restart strands the slivers and you resize each
  by hand. Window identities are only valid for one niri instance, so nothing survives a niri
  restart or a reboot, and the storage can live somewhere the session clears at logout. Recording
  desired state rather than observed state (user direction during the spec review pass) removes
  every special case for a crash mid-operation: re-applying a desired state is idempotent, so the
  restart needs no rule about what the crash interrupted.
- **Evidence:** User input, interview question 3: "Yes, survive daemon restarts". Existing user
  services in `~/.config/systemd/user/` use `Restart=always` / `Restart=on-failure`. niri-ipc
  26.4.0 `Window.id` docs: constant while the window is open, no guarantee across sessions. niri
  v26.04 `src/window/mapped.rs`, `MAPPED_ID_COUNTER` (a process-global counter used by
  `MappedId::next`): ids restart with each niri process. See T4.
- **Rejected alternatives:**
  - Memory only — rejected by the user.
  - Persist across reboots — rejected because window identities do not survive a niri restart, so
    there is nothing to restore.
  - Save only after the windows have moved (the draft order) — rejected because a crash between
    moving and saving would strand slivers with no record.
  - Record observed (settled) geometry and reconcile by comparing it at startup, with rules for each
    kind of interrupted operation — rejected by the user as over-complicated; desired state plus
    re-apply covers every case with one rule.
- **Linked technical notes:** T4
- **Driven by findings:** F2, F10; review F19, F20, F21
- **Dependent decisions:** D11
- **Referenced in spec:** Primary Flow; Alternate Flows and States; Edge Cases and Failure Modes

### D11: Daemon lifecycle under the session service manager

- **Question:** How does the daemon start, stop, and recover?
- **Decision:** It runs as a user service tied to the graphical session, starts with it, and is
  restarted after failure. If niri is not yet accepting connections when the daemon starts, the
  daemon keeps retrying rather than exiting, so the service manager never gives up on it. When
  niri's event stream closes (niri restarted or exited) the daemon exits so the service manager
  brings it back with the next session; it does not delete its saved state, because a stream close
  is not proof that niri is gone, and a fresh daemon ignores saved state tagged with a different
  niri instance anyway. Installing the user service and adding the
  niri keybind are deliverables of this feature.
- **Rationale:** The request says systemd-managed. The two existing user services on this machine
  follow the same pattern and one of them documents exiting non-zero to force a restart when its
  upstream goes away.
- **Evidence:** User request: "performed by a systemd-managed daemon process".
  `~/.config/systemd/user/asus-hotkey-notify.service` and `micmute-led-sync.service`
  (`PartOf=graphical-session.target`, `WantedBy=graphical-session.target`, `Restart=`).
  `~/.local/bin/micmute-led-sync` comment: "exit non-zero so systemd brings us back". niri v26.04
  `src/window/mapped.rs`, `MAPPED_ID_COUNTER`: window ids restart with each niri process, so state
  cannot carry across a niri restart (see T4). The start-limit risk in the third rejected
  alternative is systemd's documented `StartLimitIntervalSec=` / `StartLimitBurst=` behavior
  (systemd.unit(5)).
- **Rejected alternatives:**
  - Spawn the daemon from niri's `spawn-at-startup` — rejected because the request names systemd and
    systemd gives restart-on-failure for free.
  - Reconnect to niri in a loop instead of exiting — rejected because a fresh niri instance has fresh
    window identities, so there is no state to carry over and exit-and-restart is simpler.
  - Exit when niri is not yet reachable at startup and rely on the service manager to retry —
    rejected because repeated fast exits can trip the service manager's start limit and leave the
    daemon dead when the first hotkey is pressed.
- **Linked technical notes:** T4
- **Driven by findings:** F10, F15, F16
- **Dependent decisions:** D12
- **Referenced in spec:** Outcome; Actors and Triggers; Alternate Flows and States; Coordinations

### D12: One toggle command over a local socket

- **Question:** What can the hotkey ask the daemon to do, and what happens when the daemon is down?
- **Decision:** The hotkey client sends one of two requests over a local socket, toggle or unstash,
  and exits successfully when the daemon has applied it. Toggle follows D2. Unstash restores every
  stashed window on the focused workspace and never stashes; with nothing stashed it is a successful
  no-op. Each request has its own hotkey. A press acts on niri's state at the moment it
  arrives, not on an older picture. Presses are handled one at a time in arrival order, and a press
  is complete only when every window in it has settled. If one window closes, or leaves the floating
  layout, mid-press, that window is skipped and the rest of the press proceeds, with state saved
  for the windows that did move, and the client still exits successfully. niri does not report a
  request for a vanished window as failed, so the daemon detects the closure from niri's own
  window events and a fresh window list at the end of the press. When the daemon is
  unreachable the client exits with an error to the session log and nothing moves. Explicit stash,
  unstash, and status commands are deferred.
- **Rationale:** The request describes one hotkey with toggle semantics. Handling presses in order
  is what makes two quick presses behave as stash-then-unstash. The client never does the work
  itself because the daemon owns the state.
- **Evidence:** User request: "Hotkeys interact with daemon via a socket"; the toggle rule in the
  request. Existing keybinds in `~/.config/niri/cfg/keybinds.kdl` spawn short-lived commands
  (`noctalia msg ...`), the same shape.
- **Rejected alternatives:**
  - Client falls back to doing the stash itself when the daemon is down — rejected because the
    request puts all operations in the daemon, and split ownership of state would lose the restore
    geometry.
  - Explicit stash and status commands alongside toggle — rejected under the simpler-version test;
    deferred. (An explicit unstash command was deferred on the same grounds and then reinstated by
    the user at implementation-planning escalation E2; the user's direction is its justification.)
  - Fail the whole press when one window cannot be moved — rejected because the other windows would
    be left half done and the user gets no benefit from the failure.
- **Linked technical notes:** T3, T5
- **Driven by findings:** F5
- **Dependent decisions:** —
- **Referenced in spec:** Primary Flow; Edge Cases and Failure Modes; Deferred (YAGNI); Open Items

### D13: Stash state is tied to the window, not the workspace

- **Question:** What happens to a stashed window that moves workspace, closes, or becomes tiled?
- **Decision:** State follows the window. Moved to another workspace on the same monitor, it stays
  stashed and counts toward the toggle there; moved to a monitor with a different usable area, niri's
  shift counts as a manual move and the window pops back to its original size there. Closed, it is forgotten. Switched to tiled or fullscreen, it is
  no longer tracked as stashed, but the daemon keeps its original floating geometry, and when the
  window returns to floating the daemon restores that original straight away, because niri would
  otherwise bring it back at the sliver size.
- **Rationale:** niri window identities are stable for the window's lifetime, and the floating
  layout is per workspace, so keying on the window is the only way the state survives a workspace
  move. A tiled window has no floating geometry to restore.
- **Evidence:** niri-ipc 26.4.0 `Window.id` docs ("remains constant while the window is open"),
  `Window.is_floating`, `Event::WindowClosed`, `Event::WindowOpenedOrChanged`. niri v26.04
  `src/layout/tile.rs` (`floating_window_size`, `floating_pos`) and `src/layout/floating.rs`
  `add_tile_at`: re-floating restores the last floating size. `src/layout/workspace.rs`,
  `Workspace::set_fullscreen`: "Floating windows are never fullscreen"; a fullscreened floating
  window moves to the scrolling layout with `restore_to_floating` set.
- **Rejected alternatives:**
  - Unstash a window when it leaves the workspace — rejected because it would surprise you with a
    full-size window on the destination and adds a path nothing asked for.
  - Forget a window entirely when it is tiled (the draft rule) — rejected because niri remembers the
    sliver as the window's floating size, so re-floating would hand back a sliver with the original
    lost.
- **Linked technical notes:** T4
- **Driven by findings:** F7, F8, F11
- **Dependent decisions:** —
- **Referenced in spec:** Alternate Flows and States; Edge Cases and Failure Modes

### D14: A change to the usable area counts as a manual move

- **Question:** What happens to slivers when the bar is shown or hidden, or a monitor is added,
  removed, or rescaled, and niri shifts them?
- **Decision:** Every sliver niri moved is treated as a manual move under D7: it pops back to its
  original size where niri left it and is no longer tracked. Every stashed window reappears at
  once, and nothing is lost. Re-stashing to the new edge is deferred.
- **Rationale:** niri reports these shifts the same way it reports a drag, with no cause attached.
  Telling them apart would need the daemon to track the usable area itself. On a single laptop with
  a fixed bar this is rare, and with D7's pop-back rule the cost is a visible reappearance rather
  than a lost size.
- **Evidence:** niri v26.04 `src/layout/floating.rs`: `Data::update_config` recomputes and clamps
  positions on working-area change, which surfaces as a layout change event. Single output on this
  machine (`niri msg outputs`: eDP-1 only). See T1, T3.
- **Rejected alternatives:**
  - Track the usable area and re-apply the stash geometry after it changes — rejected under the
    evidence test; deferred with the trigger named in the spec.
- **Linked technical notes:** T1, T3
- **Driven by findings:** F6, F14
- **Dependent decisions:** —
- **Referenced in spec:** Edge Cases and Failure Modes; Deferred (YAGNI)

### D15: Edges belong to the monitor the focused workspace is on

- **Question:** With more than one monitor, which screen's edges count?
- **Decision:** The usable area and edges of the monitor that hosts the focused workspace. Windows on
  other monitors are untouched.
- **Rationale:** Each workspace lives on one monitor and has its own floating layout. The request
  says "edge of the monitor", singular.
- **Evidence:** niri-ipc 26.4.0 `Workspace.output`; niri wiki, Floating Windows: "each workspace
  and monitor has its own floating layout". Only one monitor is connected today, so this is a
  consequence of D1 and D3 rather than a tested behavior, and nothing is built or tested for a
  second monitor.
- **Rejected alternatives:** none worth noting.
- **Linked technical notes:** —
- **Driven by findings:** F11, F14
- **Dependent decisions:** —
- **Referenced in spec:** Edge Cases and Failure Modes

### D16: A window that cannot shrink is still stashed

- **Question:** What if the application refuses the 32-pixel size?
- **Decision:** The daemon shrinks the window as far as the application allows and pushes it to the
  edge anyway. niri then leaves more visible, up to 75 pixels. Unstash still restores the exact
  recorded geometry.
- **Rationale:** Many toolkits enforce a minimum window size and niri honors it. Skipping such
  windows would leave them in the way, which is the opposite of what the hotkey is for. The visible
  amount is a quarter of the resulting dimension, capped at 75 pixels, so the worst case is a
  75-pixel strip.
- **Evidence:** niri v26.04 `src/layout/floating.rs`, `Data::recompute_logical_pos`:
  `min_on_screen_hor/ver = clamp(size / 4, 10, 75)` computed from the window's current size; size
  requests are clamped to the window's min/max in `FloatingSpace::set_window_width` and
  `ensure_min_max_size`. See T1.
- **Rejected alternatives:**
  - Skip windows that cannot reach 32 pixels — rejected because they would stay in the way.
- **Linked technical notes:** T1, T3
- **Driven by findings:** F1
- **Dependent decisions:** —
- **Referenced in spec:** Edge Cases and Failure Modes

## Amendments after the build

### A1: The daemon only moves windows; it never resizes them

- **Date:** 2026-10-11, after the first build was in daily use.
- **Decision (user):** Remove the 32-pixel resize from the stash. A stash is a push toward the
  chosen edge that niri clamps; a restore is the nudge back to the original position. niri keeps a
  quarter of each dimension visible, capped at 75 pixels, so a pushed window shows a strip of up
  to 75 pixels. Any change the user makes to a stashed window (a move, a resize, a drag, tiling
  it) forgets it: the window stays where the user put it, at the size it has.
- **Rationale (user):** Slivers look bad in the alt-tab switcher and animate oddly when moved, and
  most windows already refuse to shrink, so the 75-pixel strip was the common case anyway. Not
  resizing also removes the pop-back, the tiled/re-float rule, the drag tracking, and the
  hand-move versus hand-resize judgement.
- **Supersedes:** D4 (sliver geometry) and D16 (windows that cannot shrink) entirely; the pop-back
  half of D7 and E1 (a hand move now forgets, like a hand resize); the re-float half of D13 (a
  stashed window that is tiled is forgotten, not remembered); the "rows added for" parts of F19,
  F21, F22, F25 that concerned resizes. D3 (edge choice), D5, D6, D8–D12, D14, D15 stand.
- **Spec text affected:** Outcome, Primary Flow steps 5–6, the alternate flows for hand moves,
  hand resizes, and tiled windows, and the Edge Cases rows about windows that cannot shrink,
  about overlapping slivers, and about pop-back. These sections describe the first build and are
  left as history; the behaviour that ships is this amendment.

### A2: No settling period; the daemon reads the result back

- **Date:** 2026-10-11, same day as A1.
- **Decision (user):** Remove the settling period. After sending a batch of moves the daemon
  reads the window list back once and records where each moved window now sits. A press is
  answered as soon as that read-back is done. Any later report that differs from the recorded
  position is a change the user made and forgets the window, exactly as before.
- **Rationale:** niri applies a move, including its clamp, before it answers the request, and
  refreshes the window list it serves over IPC before that answer is delivered (verified in niri
  26.04's IPC server: the action runs inside an idle callback, the IPC state refreshes in the
  post-dispatch callback, and the reply is written on a later loop iteration). So the read-back
  is already final. The timers existed to absorb application-driven resizes, which A1 removed.
- **Supersedes:** D9 (settling period) and D10 (the second-press wait: presses are now handled
  to completion one at a time, so there is nothing to wait for); the settling clauses of D7 and
  D12; T3 as a description of timers. The alt-tab rule (D6), focus (D8), and the state-file
  model (D11, D13–D15) stand.
- **Known cost:** if a future niri answered a move request before applying it, the daemon would
  read the old position back and later treat its own move as a hand change, forgetting the
  window. That fails safe: the window stays where it is and its original is in the journal.

### A3: Any focus counts when focus-follows-mouse is off

- **Date:** 2026-10-11.
- **Decision (user):** The alt-tab-only rule (D6) exists because focus-follows-mouse makes a
  direct focus change ambiguous. niri does not expose that setting over IPC, so the daemon takes a
  flag: started with `--no-focus-follows-mouse`, any focus change onto a stashed window restores
  it, not only one through the switcher. A return to the window focus came from (the overview
  closing) and focus changes the daemon itself requested still do not count.
- **Also:** the window a restore press focuses is now the one niri's own debounced
  `focus_timestamp` marks as most recently focused, so the daemon keeps no focus history.

### A4: A `stash` command alongside `toggle` and `unstash`

- **Date:** 2026-10-11, before publishing.
- **Decision (user):** `niri-stash stash` stashes every floating window on the focused workspace
  that is not stashed yet and never restores, the mirror of `unstash`. `toggle` is unchanged. The
  wire request is `stash`. Nothing is bound to it by default.

