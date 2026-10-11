# Scope Boundary: Floating Window Stash for niri

## Work Item

No tracker item exists. The work item is the request the operator typed when invoking
`han-planning:plan-a-feature` on 2026-10-11. It is the only boundary this run has, and
the text below is that request, recorded word for word.

## Stated Scope

> I need to build a floating window manager for niri (it doesn't have scratch workspaces, so I'm inventing a workaround). The idea is to have a hotkey that puts all floating windows on the current workspace to the size where they barely stick out:
>
> - when stash command is activated, and there are unstashed windows on the current workspace, they are stashed;
> - when stash command is activated, and there are no unstashed windows on the current workspace, all windows are unstashed;
> - stashed window is moved beyond an edge of the monitor so that only its small part sticks out. Niri enforces that window can't be totally out of view, but if we make shrink width of a window to 32px, it will only stick out horiaontally a bit;
> - when a window is stashed, the exact edge of the monitor where the window belongs is the closest to the window's center;
> - when a window is unstashed, its original position and size is restored;
>
> All of these operations are performed by a systemd-managed daemon process. It reads niri's event stream to detect when a stashed window is alt-tabbed into so it can unstash it. It also marks window as unstashed if user modifies its size/position manually.
>
> Window should be unstashed if niri's focused window goes to None and then to a stashed window (alt-tabbing focuses noctalia's window, which is not reported by event stream). If niri's focused window goes from one window to another without jumping through None, it means that it got activated by `focus-follows-mouse`, in which case it shouldn't be unstashed.
>
> Hotkeys interact with daemon via a socket.
>
> Daemon should be implemented in rust, you can set up a project in ~/p.

## Stated Exclusions

None stated.

## Operator-Stated Scope

Same as the Stated Scope above: the operator's typed request is the work item. Two constraints in it
are stated as scope in their own right: "Daemon should be implemented in rust" and "you can set up a
project in ~/p".

## Direction of Travel

Answered 2026-10-11: "No, nothing is being replaced." niri's floating layout, noctalia, and the
niri keybind config stay as they are; the daemon sits beside them. Later the same day, at
implementation-planning escalation E2, the operator directed that the Mod+Shift+S region-screenshot
binding be replaced by the unstash binding; that one line is the only thing removed.

## Visual Material Received

None received

## Record Provenance

Established by `han-planning:plan-a-feature` on 2026-10-11, from the operator's typed request. No
prior record existed; nothing was inherited.
