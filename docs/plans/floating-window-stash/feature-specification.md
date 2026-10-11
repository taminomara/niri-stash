# Feature Specification: Floating Window Stash for niri

> **Amended after the build.** The daemon no longer resizes windows: a stash pushes a window off
> its edge until niri stops it, leaving a strip of up to 75 pixels, and any change you make to a
> stashed window by hand forgets it (amendment A1). There is no settling period either: the daemon
> reads the result of each move back from niri and answers the hotkey at once (amendment A2). Both
> are in [artifacts/decision-log.md](artifacts/decision-log.md#amendments-after-the-build).
> Sentences below about a 32-pixel sliver, pop-back, re-floated windows, settling, and a second
> press waiting describe the first build.

One hotkey tucks every floating window on the current workspace out of the way as a thin sliver at
the nearest screen edge. The same hotkey brings them all back where they were. A background daemon
does the work and also brings a window back on its own when you alt-tab into it. This stands in for
the scratch workspaces that niri, a scrolling-tiling Wayland compositor, does not have.

## Outcome

You press the stash hotkey and the floating windows on your current workspace shrink to slivers
pressed against the screen edge closest to each of them. Your tiled windows are left unobstructed.
Press it again and each window comes back at exactly the size and position it had. Alt-tab into a
stashed window and it comes back by itself. The daemon that does this survives its own restarts
within a login session without losing track of what it stashed.

Done looks like this, checked by hand on your laptop:

- A floating terminal and a floating calculator on one workspace both become slivers on one press
  and both come back on the next, at their old size and place.
- Alt-tabbing into the stashed terminal through niri's alt-tab switcher brings only the terminal
  back.
- Hovering the pointer over a sliver does not bring it back.
- Dragging a sliver somewhere else makes it pop back to its original size where you dropped it, and
  the next press stashes it again from there.
- Restarting the daemon while windows are stashed, then pressing the hotkey, still brings them back.
- A second hotkey brings everything on the workspace back without ever stashing, for when you do
  not want to think about which way the toggle will go.
- The daemon is installed as a user service that starts with the session, and both hotkeys are
  bound in your niri configuration. All of these are deliverables of this feature
  ([D11](artifacts/decision-log.md#d11-daemon-lifecycle-under-the-session-service-manager)).

## Actors and Triggers

- **Actors**
  - **You**, pressing a hotkey, alt-tabbing through niri's switcher or noctalia's window picker, or
    moving and resizing windows by hand.
  - **The stash daemon**, a long-running background process that owns all stash state and performs
    every move and resize.
  - **niri**, the compositor, which reports window, focus, and layout changes to the daemon and
    carries out the daemon's move and resize requests.
  - **The hotkey client**, a short-lived command niri runs when you press the hotkey. It passes the
    request to the daemon and exits.
- **Triggers**
  - The stash hotkey (toggle).
  - The unstash hotkey (always restores, never stashes)
    ([D12](artifacts/decision-log.md#d12-one-toggle-command-over-a-local-socket)).
  - Keyboard focus landing on a stashed window after no window was focused.
  - A stashed window being moved or resized by something other than the daemon.
  - A stashed window closing, leaving the floating layout, or niri restarting.
- **Preconditions**
  - niri is running and the daemon is connected to its event stream.
  - The daemon is running under the session's service manager (systemd) and reachable on its local
    socket ([D11](artifacts/decision-log.md#d11-daemon-lifecycle-under-the-session-service-manager)).

## Primary Flow

The happy path is one press of the hotkey with at least one floating window open on the focused
workspace.

1. You press the stash hotkey. niri runs the hotkey client, which sends a single toggle request to
   the daemon ([D12](artifacts/decision-log.md#d12-one-toggle-command-over-a-local-socket)).
2. The daemon asks niri for the current state of windows and workspaces at that moment, so the
   press acts on what is on screen now rather than on an older picture
   ([D12](artifacts/decision-log.md#d12-one-toggle-command-over-a-local-socket),
   [T5](artifacts/feature-technical-notes.md#t5-a-press-acts-on-a-fresh-snapshot)). It looks at the
   floating windows on the focused workspace only. Tiled windows and other workspaces are never
   touched ([D1](artifacts/decision-log.md#d1-a-stash-acts-on-the-focused-workspace-only)).
3. At least one of those floating windows is not stashed, so this press stashes. Windows already
   stashed stay stashed ([D2](artifacts/decision-log.md#d2-one-hotkey-toggles-between-stash-and-unstash)).
4. For each unstashed floating window, the daemon records the window's current size and position
   and the edge it is about to go to. It writes that desired state down before it moves anything,
   so a crash in the middle cannot lose the original ([D10](artifacts/decision-log.md#d10-stash-state-survives-a-daemon-restart-within-the-session),
   [T4](artifacts/feature-technical-notes.md#t4-what-survives-a-restart)). It then picks the edge of
   the usable screen area closest to the window's center
   ([D3](artifacts/decision-log.md#d3-each-window-goes-to-the-nearest-edge-of-the-usable-area)).
   The usable area is the screen minus the bar, so a window stashed to the top sits against the bar
   rather than behind it ([T1](artifacts/feature-technical-notes.md#t1-niris-off-screen-limit-and-coordinate-frame)).
5. The daemon shrinks the window to 32 pixels in the direction that points at that edge.
   The other dimension and the position along the edge are kept. It then pushes the window over the
   edge. niri stops the push when about 10 pixels remain visible, and that 10-pixel sliver is what
   you see ([D4](artifacts/decision-log.md#d4-stashed-geometry-is-a-32-pixel-sliver-pushed-to-the-edge),
   [T1](artifacts/feature-technical-notes.md#t1-niris-off-screen-limit-and-coordinate-frame)).
6. The daemon waits briefly for the window to settle at its new size and place. Anything the window
   does during that short settling period is part of the stash and never counts as a manual change.
   That includes an application answering with a size of its own choosing. The geometry the window
   settles at is what the daemon remembers as "stashed"
   ([D7](artifacts/decision-log.md#d7-a-manual-move-pops-the-window-back-a-manual-resize-forgets-it),
   [T3](artifacts/feature-technical-notes.md#t3-telling-the-daemons-own-moves-from-yours)).
7. If the window that had keyboard focus was among those stashed, focus moves to the tiled layout,
   so your next keystrokes do not go into a sliver. If the workspace has no tiled window, focus
   stays on the sliver, because niri has nowhere else to put it
   ([D8](artifacts/decision-log.md#d8-the-hotkey-moves-keyboard-focus-in-both-directions)).
8. The daemon saves the settled state so a restart does not lose it
   ([D10](artifacts/decision-log.md#d10-stash-state-survives-a-daemon-restart-within-the-session)).
9. The hotkey client exits successfully. There is no other on-screen feedback.

## Alternate Flows and States

### Unstash everything with the hotkey

- **Entry condition:** You press the hotkey and every floating window on the focused workspace is
  already stashed (or the only floating windows there are stashed ones).
- **Sequence:** The daemon restores each stashed window on that workspace to the size and position it
  recorded when stashing it ([D5](artifacts/decision-log.md#d5-unstash-restores-the-recorded-size-and-position)).
  Each window gets the same short settling period as on stash, so the restore itself is never read
  as a manual change ([T3](artifacts/feature-technical-notes.md#t3-telling-the-daemons-own-moves-from-yours)).
  Keyboard focus moves to whichever of the restored windows you had focused most recently; if the
  daemon has not seen any of them focused, focus goes to the first one restored
  ([D8](artifacts/decision-log.md#d8-the-hotkey-moves-keyboard-focus-in-both-directions)).
- **Exit:** No window on that workspace is stashed. Windows stashed on other workspaces are
  untouched.

### Unstash everything with the unstash hotkey

- **Entry condition:** You press the unstash hotkey, whatever the state of the focused workspace.
- **Sequence:** Identical to "Unstash everything with the hotkey" above: every stashed window on the
  focused workspace is restored and focus moves to the most recently focused of them. If nothing on
  the workspace is stashed, nothing moves and the hotkey client exits successfully
  ([D12](artifacts/decision-log.md#d12-one-toggle-command-over-a-local-socket)).
- **Exit:** No window on that workspace is stashed.

### Alt-tab into a stashed window

- **Entry condition:** niri reports that no window has focus, and the next focus change lands on a
  stashed window that is **not** the window that had focus before. Noctalia's window picker,
  niri's own built-in alt-tab switcher, and the overview all take focus away from every window while
  open, so this covers all three
  ([D6](artifacts/decision-log.md#d6-a-focus-change-through-no-window-unstashes-a-direct-one-does-not),
  [T2](artifacts/feature-technical-notes.md#t2-how-alt-tab-is-recognised)).
- **Sequence:** The daemon restores that one window to its recorded size and position. Other stashed
  windows on the workspace stay stashed. Focus is already on the window.
- **Exit:** That window is unstashed. The next hotkey press on this workspace stashes again, because
  an unstashed floating window now exists ([D2](artifacts/decision-log.md#d2-one-hotkey-toggles-between-stash-and-unstash)).

### Focus returns to the window it came from

- **Entry condition:** niri reports that no window has focus, and focus then lands on the same window
  that had it before. This is what happens when you open the picker and cancel it, lock and unlock
  the screen, open and close the overview without choosing anything, or switch to an empty workspace
  and back.
- **Sequence:** The daemon does nothing, even if that window is a stashed sliver. Returning to where
  you were is not alt-tabbing into something
  ([D6](artifacts/decision-log.md#d6-a-focus-change-through-no-window-unstashes-a-direct-one-does-not)).
- **Exit:** Nothing changes.

### Focus moves to a sliver without passing through "no window"

- **Entry condition:** Focus goes directly from one window to a stashed window. This is what happens
  when the pointer drifts over a sliver with focus-follows-mouse on, and also when a keyboard action
  focuses a window directly.
- **Sequence:** The daemon does nothing. The sliver has focus and keeps its stashed geometry
  ([D6](artifacts/decision-log.md#d6-a-focus-change-through-no-window-unstashes-a-direct-one-does-not)).
- **Exit:** The window stays stashed. Press the hotkey or alt-tab into it to bring it back.

### You move a stashed window yourself

- **Entry condition:** A stashed window's position changes after its settling period, its size does
  not, and the daemon did not request the change
  ([T3](artifacts/feature-technical-notes.md#t3-telling-the-daemons-own-moves-from-yours)).
- **Sequence:** The daemon restores the window's original size at the position where you left it,
  then stops tracking it. Dragging a sliver out is how you say "I want this window here"
  ([D7](artifacts/decision-log.md#d7-a-manual-move-pops-the-window-back-a-manual-resize-forgets-it)).
- **Exit:** The window counts as unstashed, at its original size, where you dropped it. The next
  hotkey press stashes it again from there.

### You resize a stashed window yourself

- **Entry condition:** A stashed window's size changes after its settling period, and the daemon did
  not request the change
  ([T3](artifacts/feature-technical-notes.md#t3-telling-the-daemons-own-moves-from-yours)).
- **Sequence:** The daemon forgets that the window was stashed and discards the recorded size and
  position. The window stays exactly as you left it; nothing is restored. A hand resize means you
  chose that size ([D7](artifacts/decision-log.md#d7-a-manual-move-pops-the-window-back-a-manual-resize-forgets-it)).
- **Exit:** The window counts as unstashed. The next hotkey press stashes it again, recording its
  current geometry as the one to restore.

### A stashed window is tiled and later floated again

- **Entry condition:** You switch a stashed window from floating to tiled, or it goes fullscreen,
  which niri treats the same way. Later it returns to the floating layout, and niri puts it back at
  the sliver size and position it last had as a floating window.
- **Sequence:** While the window is tiled the daemon does not track it, but it keeps the original
  floating geometry on file. When the window returns to floating, the daemon restores that original
  geometry straight away, so you get your window back rather than a sliver
  ([D13](artifacts/decision-log.md#d13-stash-state-is-tied-to-the-window-not-the-workspace)).
- **Exit:** The window is floating, unstashed, at its original size and position.

### The daemon restarts

- **Entry condition:** The daemon exits for any reason within a login session (crash, rebuild,
  manual restart) and the service manager starts it again.
- **Sequence:** The saved state records, for every tracked window, its original geometry and what
  it should currently be: stashed at a named edge, or restored. The daemon reads it and drops
  windows that no longer exist. It then brings every remaining window to its desired state. A
  window that should be stashed is stashed again, with no visible change if it already is. A window
  that should be restored is restored and then forgotten. Because the desired state is written before any command
  is sent, a crash at any point leaves nothing to guess about. Anything done to a sliver while the
  daemon was not running is undone by this step
  ([D10](artifacts/decision-log.md#d10-stash-state-survives-a-daemon-restart-within-the-session),
  [T4](artifacts/feature-technical-notes.md#t4-what-survives-a-restart)).
- **Exit:** Slivers from before the restart still come back with the hotkey or alt-tab.

### niri restarts

- **Entry condition:** niri's event stream closes.
- **Sequence:** Every window is gone and window identities start over, so the daemon exits. The
  service manager starts it again when the graphical session is back, and the fresh daemon ignores
  any saved state that belongs to a different niri instance, so nothing from before is reused
  ([D11](artifacts/decision-log.md#d11-daemon-lifecycle-under-the-session-service-manager),
  [T4](artifacts/feature-technical-notes.md#t4-what-survives-a-restart)).
- **Exit:** A fresh daemon with no stashed windows.

### The daemon starts before niri is ready

- **Entry condition:** The service manager starts the daemon at login before niri accepts
  connections.
- **Sequence:** The daemon keeps trying to connect rather than failing, so the service manager does
  not give up on it, and it becomes reachable as soon as niri is
  ([D11](artifacts/decision-log.md#d11-daemon-lifecycle-under-the-session-service-manager)).
- **Exit:** Connected, with no stashed windows.

## Edge Cases and Failure Modes

| Condition | Required Behavior |
| --------- | ----------------- |
| The hotkey is pressed and the focused workspace has no floating windows. | Nothing moves. The hotkey client exits successfully. There is no on-screen message. |
| A window refuses to shrink to 32 pixels (its application enforces a larger minimum size, or rounds to whole character cells like a terminal). | The daemon shrinks it as far as the application allows and pushes it to the edge. niri then leaves more visible, up to 75 pixels instead of 10. The size the window settles at is the stashed size. Unstash still restores the exact recorded original ([D16](artifacts/decision-log.md#d16-a-window-that-cannot-shrink-is-still-stashed), [T1](artifacts/feature-technical-notes.md#t1-niris-off-screen-limit-and-coordinate-frame), [T3](artifacts/feature-technical-notes.md#t3-telling-the-daemons-own-moves-from-yours)). |
| An application answers the daemon's resize late, after the settling period has ended. | Treated as a manual resize: the daemon forgets the stash. The settling period is sized so that this is rare ([D7](artifacts/decision-log.md#d7-a-manual-move-pops-the-window-back-a-manual-resize-forgets-it), [T3](artifacts/feature-technical-notes.md#t3-telling-the-daemons-own-moves-from-yours)). |
| A window's center is equally close to two edges. | Edges are tried in the order left, right, top, bottom and the first of the tied edges wins ([D3](artifacts/decision-log.md#d3-each-window-goes-to-the-nearest-edge-of-the-usable-area)). |
| Two windows that overlapped along an edge are both stashed to that edge. | Their slivers overlap, and the one underneath cannot be reached with the pointer. It still comes back with the hotkey or alt-tab ([D4](artifacts/decision-log.md#d4-stashed-geometry-is-a-32-pixel-sliver-pushed-to-the-edge)). |
| A stashed window is closed. | The daemon forgets it ([D13](artifacts/decision-log.md#d13-stash-state-is-tied-to-the-window-not-the-workspace)). |
| A stashed window is moved to another workspace on the same monitor. | It stays stashed, and it now counts toward the toggle on its new workspace ([D13](artifacts/decision-log.md#d13-stash-state-is-tied-to-the-window-not-the-workspace)). |
| A stashed window is moved to a workspace on a monitor with a different usable area. | niri shifts it, and the shift counts as a manual move: the window pops back to its original size there and is no longer tracked. Not tested; only one monitor exists ([D13](artifacts/decision-log.md#d13-stash-state-is-tied-to-the-window-not-the-workspace), [D15](artifacts/decision-log.md#d15-edges-belong-to-the-monitor-the-focused-workspace-is-on)). |
| A new floating window opens while others on the workspace are stashed. | It is unstashed. The next hotkey press stashes it, and the already-stashed windows stay stashed ([D2](artifacts/decision-log.md#d2-one-hotkey-toggles-between-stash-and-unstash)). |
| A window closes while a press is being carried out. | That window is skipped. niri never reports a request for a vanished window as failed, so the daemon notices the closure itself. The other windows in the same press are still stashed or restored, their state is saved, and the hotkey client still exits successfully ([D12](artifacts/decision-log.md#d12-one-toggle-command-over-a-local-socket), [T5](artifacts/feature-technical-notes.md#t5-a-press-acts-on-a-fresh-snapshot)). |
| A window is switched to tiled, or goes fullscreen, while a press is being carried out. | It leaves the press. Its original floating geometry is kept, and when it returns to floating it is restored to that original, the same as a stashed window that is tiled later ([D13](artifacts/decision-log.md#d13-stash-state-is-tied-to-the-window-not-the-workspace)). |
| The daemon cannot write its saved state when a press begins. | Nothing moves. The hotkey client exits with an error that goes to the session log. The desired state is always written down before any command is sent, so a press that cannot record it does not start ([D10](artifacts/decision-log.md#d10-stash-state-survives-a-daemon-restart-within-the-session)). |
| An application resizes its own stashed window after it has settled (for example a dialog reflows). | Treated as a manual resize: the daemon forgets the stash ([D7](artifacts/decision-log.md#d7-a-manual-move-pops-the-window-back-a-manual-resize-forgets-it)). |
| The usable area changes while windows are stashed (for example the bar is shown or hidden) and niri shifts the slivers. | Treated as a manual move for every sliver niri moved: each pops back to its original size where niri left it and is no longer tracked. Nothing is lost, but every stashed window reappears at once; press the hotkey to stash them again ([D14](artifacts/decision-log.md#d14-a-change-to-the-usable-area-counts-as-a-manual-move)). |
| A floating window is maximized to the usable area when the hotkey is pressed. | It is an ordinary floating window at a large size and is stashed like any other. Its full-size geometry is what unstash restores. |
| The hotkey is pressed while the overview is open. | The press acts on the focused workspace as usual. Focus is away from every window while the overview is open, so closing it onto a different stashed window unstashes that window, and closing it back onto the same window does not ([D6](artifacts/decision-log.md#d6-a-focus-change-through-no-window-unstashes-a-direct-one-does-not)). |
| Unstash restores a geometry that no longer fits the current usable area. | The daemon requests the recorded geometry and niri clamps it. The window lands wherever niri allows ([D5](artifacts/decision-log.md#d5-unstash-restores-the-recorded-size-and-position)). |
| The hotkey is pressed while the daemon is not running or not reachable. | Nothing moves. The hotkey client exits with an error that goes to the session log ([D12](artifacts/decision-log.md#d12-one-toggle-command-over-a-local-socket)). |
| The hotkey is pressed twice in quick succession. | The second press waits until every window from the first press has settled, then acts on the result: the first stashes, the second unstashes ([D12](artifacts/decision-log.md#d12-one-toggle-command-over-a-local-socket), [T3](artifacts/feature-technical-notes.md#t3-telling-the-daemons-own-moves-from-yours)). |
| The hotkey is pressed right after a workspace switch or a window opening. | The press sees the new workspace and the new window, because it works from a fresh snapshot taken when the press arrives ([T5](artifacts/feature-technical-notes.md#t5-a-press-acts-on-a-fresh-snapshot)). |
| Focus lands on a different stashed window right after "no window focused" for a reason other than alt-tab (for example the pointer rests on a sliver while the picker is open, or the focused window closes while the picker is open). | The window is unstashed. The daemon cannot tell this apart from alt-tab and does not try ([D6](artifacts/decision-log.md#d6-a-focus-change-through-no-window-unstashes-a-direct-one-does-not)). |
| You alt-tab into the very sliver that already had focus. | Nothing happens, because focus returned to the window it came from. Press the hotkey to bring it back ([D6](artifacts/decision-log.md#d6-a-focus-change-through-no-window-unstashes-a-direct-one-does-not)). |
| The focused workspace is on a second monitor. | The edges and usable area are those of that monitor. This follows from the rules above and is not tested; only one monitor exists ([D15](artifacts/decision-log.md#d15-edges-belong-to-the-monitor-the-focused-workspace-is-on)). |
| The daemon's saved state is missing, unreadable, or from a different niri instance at startup. | The daemon starts with no stashed windows and logs the problem. Any slivers left over are ordinary windows ([D10](artifacts/decision-log.md#d10-stash-state-survives-a-daemon-restart-within-the-session), [T4](artifacts/feature-technical-notes.md#t4-what-survives-a-restart)). |

## User Interactions

- **Affordances:** Two hotkeys, bound in your niri configuration to the hotkey client: one toggles,
  one always unstashes. The unstash hotkey takes over the binding that previously took a region
  screenshot; the screenshot stays on its other key. The daemon has no window, tray icon, or menu.
- **Feedback:** The windows themselves are the feedback. They shrink to the edge, or come back. There
  is no notification or sound.
- **Keyboard focus:** A sliver can hold keyboard focus. That happens when the pointer rests on it with
  focus-follows-mouse, and when you stash the focused window on a workspace that has no tiled
  windows. Keystrokes then go into a window you can barely see. The hotkey or alt-tab brings it back
  ([D6](artifacts/decision-log.md#d6-a-focus-change-through-no-window-unstashes-a-direct-one-does-not),
  [D8](artifacts/decision-log.md#d8-the-hotkey-moves-keyboard-focus-in-both-directions)).
- **Error states:** When the daemon is unreachable, nothing happens on screen and the error goes to
  the session log. When the daemon's saved state cannot be read, it logs the problem and starts
  empty.

## Coordinations

| Coordinating System | Direction | Interaction | Ordering / Consistency Requirement |
| ------------------- | --------- | ----------- | ---------------------------------- |
| niri event stream | inbound | Reports window open, close, focus, layout, and workspace changes; also reports when no window has focus. | Changes are applied in the order niri reports them. The alt-tab signal is focus leaving every window and then landing on a stashed window other than the one it left ([T2](artifacts/feature-technical-notes.md#t2-how-alt-tab-is-recognised)). Changes the daemon caused are recognised as its own before the manual-change rule applies ([T3](artifacts/feature-technical-notes.md#t3-telling-the-daemons-own-moves-from-yours)). |
| niri state queries | outbound/inbound | Fetches the current windows and workspaces when a press arrives. | A press acts on niri's state at the moment of the press, not on an older picture ([T5](artifacts/feature-technical-notes.md#t5-a-press-acts-on-a-fresh-snapshot)). |
| niri actions | outbound | Resize a window, move a floating window, focus a window, move focus to the tiled layout. | All windows of one press have settled before the next press is handled. |
| Hotkey client | inbound | Delivers one toggle request per hotkey press over a local socket and reports success or failure. | One press, one toggle; presses are handled in arrival order. |
| Session service manager (systemd) | lifecycle | Starts the daemon with the graphical session, restarts it after failure or after niri restarts. | The daemon must be reachable by the time a hotkey can be pressed, including when it started before niri was ready ([D11](artifacts/decision-log.md#d11-daemon-lifecycle-under-the-session-service-manager)). |
| The daemon's saved stash state | outbound/inbound | Keeps what each tracked window should be (original geometry plus stashed-at-edge or restored) across daemon restarts within one login session; nothing is kept past logout. | The desired state is written before any command goes to niri, so a restart can re-apply it without guessing ([T4](artifacts/feature-technical-notes.md#t4-what-survives-a-restart)). |
| noctalia window picker, niri's built-in alt-tab switcher, overview, lock screen | indirect | Each takes keyboard focus away from every window while open, which niri reports as "no window focused". | None. The daemon only observes the resulting focus events. |

## Out of Scope

- Tiled windows. Only floating windows are ever stashed.
- Windows on workspaces other than the focused one, and windows on other monitors.
- Any visible indicator of stashed windows (bar module, badge, overlay). The slivers are the
  indicator.
- Launch-or-summon behavior ("show my scratch terminal, starting it if needed"). This feature manages
  windows that already exist.
- Changes to niri or noctalia themselves. Both are used as they are. Your niri configuration gains
  the toggle keybind, and the unstash keybind takes over the key that previously took a region
  screenshot (your direction; the screenshot stays on Print).
- Multi-monitor behavior beyond what follows from the rules above. Only one monitor exists, so
  nothing is built or tested for a second one.

## Deferred (YAGNI)

This is work no evidence supports yet, not work the work item excludes. Every entry carries the
trigger that would justify revisiting it.

### Per-application exemptions from stashing

- **Why deferred:** Evidence test. Your niri config floats picture-in-picture players and toasts in a
  corner, so stashing them is plausible friction, but nothing shows it happens yet. Every floating
  window is stashed ([D9](artifacts/decision-log.md#d9-every-floating-window-is-stashed-with-no-exemptions)).
- **Reopen when:** A window you never want stashed keeps getting stashed in daily use.
- **Source:** Interview, question 2.

### Explicit stash and status commands

- **Why deferred:** Simpler-version test. The request describes one hotkey with toggle semantics.
  The explicit **unstash** command was reinstated by your direction during implementation planning
  and is now a second hotkey ([D12](artifacts/decision-log.md#d12-one-toggle-command-over-a-local-socket));
  an explicit stash-only command and a status listing remain deferred.
- **Reopen when:** You want a binding that always stashes, or a way to list what is stashed from a
  script.
- **Source:** Interview, resolved from the request; unstash reinstated at implementation-planning
  escalation E2.

### Re-stashing after the usable area changes

- **Why deferred:** Evidence test. A bar toggle or monitor change moves the slivers and they all pop
  back to full size ([D14](artifacts/decision-log.md#d14-a-change-to-the-usable-area-counts-as-a-manual-move)).
  On a single laptop with a fixed bar this is rare, and nothing is lost when it happens.
- **Reopen when:** Every stashed window reappearing after a bar or monitor change annoys you more
  than once.
- **Source:** Interview, edge-case analysis.

### Spreading overlapping slivers apart along an edge

- **Why deferred:** Evidence test. Slivers overlap only when the windows already overlapped along that
  edge, and the hidden one is still reachable by hotkey or alt-tab.
- **Reopen when:** You lose track of a sliver hidden under another one.
- **Source:** Review finding F9.

### On-screen feedback when the hotkey has nothing to do

- **Why deferred:** Evidence test. A silent no-op is the simplest behavior and the request names no
  feedback.
- **Reopen when:** You find yourself pressing the hotkey repeatedly, unsure whether it worked.
- **Source:** Interview, edge-case analysis.

## Open Items

- **OI-1:** The contract between the hotkey client, the niri keybind that invokes it, and the daemon:
  the command name niri runs, the socket location, the toggle request shape, and how success and
  failure are signalled ([D12](artifacts/decision-log.md#d12-one-toggle-command-over-a-local-socket)).
  Delegated to implementation planning.
  - **Resolves when:** Implementation planning pins the command name, socket path, request shape, and
    exit behavior in one place that both the daemon and the niri keybind follow.
  - **Blocks implementation:** No. The behavior in this specification holds for any choice.

## Summary

- **Outcome delivered:** One hotkey hides every floating window on the current workspace as a sliver
  at its nearest edge and brings them all back; alt-tab brings one back on its own.
- **Primary actors:** You, the stash daemon, niri, the hotkey client.
- **Decisions settled by evidence:** 12 — see [artifacts/decision-log.md](artifacts/decision-log.md)
- **Decisions settled by user input:** 4 — see [artifacts/decision-log.md](artifacts/decision-log.md)
- **Sub-agents consulted:** junior-developer, edge-case-explorer — see [artifacts/team-findings.md](artifacts/team-findings.md)
- **Key adjustments from review:** Settling period for the daemon's own changes, originals saved before
  any move, "return to the same window" never unstashes, a hand-moved sliver pops back to its
  original size, re-floated windows get their original geometry back, saved state tied to the niri
  instance — see
  [artifacts/team-findings.md](artifacts/team-findings.md)
- **Remaining open items:** 1
- **Technical notes:** 5 — see [artifacts/feature-technical-notes.md](artifacts/feature-technical-notes.md)

## Review History

- **Review mode:** lightweight
- **Spec-aware mode:** engaged
- **Iterations completed:** 1 — see [artifacts/review-iteration-history.md](artifacts/review-iteration-history.md)
- **Findings raised:** 8 (F18–F25, continuing the numbering of [artifacts/team-findings.md](artifacts/team-findings.md)), all major; 3 resolved by evidence (F18, F22, F23), 5 by user input (F19, F20, F21: desired-state model; F24: second hotkey that always unstashes; F25: hand-move decision waits for the drop) — see [artifacts/review-findings.md](artifacts/review-findings.md)
- **Assumptions challenged across all passes:** niri reports failed actions (refuted); a restart can infer what a crash interrupted from observed geometry (replaced by recording desired state); the state file is always writable (treated as a failure mode); D13 covers mid-press tiling (refuted for that case)
- **Consolidations made:** none
- **Ambiguities resolved, and how:** how a restart recovers from a crash mid-operation, settled by the user: the file records desired state, written before any command, and a restart re-applies it
- **Technical notes added/edited:** 5 edited (T1, T2, T3, T4, T5) — see [artifacts/feature-technical-notes.md](artifacts/feature-technical-notes.md)
- **Open items remaining:** 1 — OI-1 (hotkey client ⇄ daemon contract), delegated to implementation planning; does not block
