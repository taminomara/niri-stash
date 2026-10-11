//! Translation between the core's types and niri's IPC types.

use std::collections::HashMap;

use niri_ipc::{Action, Event, Output, PositionChange, Window, WindowLayout, Workspace};

use crate::core::command::Command;
use crate::core::geometry::Geometry;
use crate::core::input::{
    CompositorEvent, CompositorSnapshot, FocusTimestamp, WindowInfo, WorkspaceInfo,
};

/// Geometry from a niri layout report, or `None` when the window has no position (it is being
/// dragged, or it is tiled).
fn geometry_from_layout(layout: &WindowLayout) -> Option<Geometry> {
    let (x, y) = layout.tile_pos_in_workspace_view?;
    let (w, h) = layout.window_size;
    Some(Geometry {
        x,
        y,
        w: f64::from(w),
        h: f64::from(h),
    })
}

/// A niri window as the core sees it.
pub(crate) fn window_info_from_niri(w: &Window) -> WindowInfo {
    WindowInfo {
        id: w.id,
        workspace: w.workspace_id,
        floating: w.is_floating,
        focused: w.is_focused,
        geometry: geometry_from_layout(&w.layout),
        focus_timestamp: w.focus_timestamp.map(|t| FocusTimestamp {
            secs: t.secs,
            nanos: t.nanos,
        }),
    }
}

/// The core inputs one niri event carries (none for events the core ignores).
pub(crate) fn events_from_niri(event: &Event) -> Vec<CompositorEvent> {
    match event {
        Event::WindowLayoutsChanged { changes } => changes
            .iter()
            .filter_map(|(id, layout)| {
                geometry_from_layout(layout)
                    .map(|geometry| CompositorEvent::GeometryChanged { id: *id, geometry })
            })
            .collect(),
        Event::WindowFocusChanged { id } => vec![CompositorEvent::FocusChanged(*id)],
        Event::WindowOpenedOrChanged { window } => vec![CompositorEvent::WindowChanged(
            window_info_from_niri(window),
        )],
        Event::WindowClosed { id } => vec![CompositorEvent::WindowClosed(*id)],
        _ => Vec::new(),
    }
}

/// A snapshot from niri's three list replies.
pub(crate) fn snapshot_from_niri(
    windows: &[Window],
    workspaces: &[Workspace],
    outputs: &HashMap<String, Output>,
) -> CompositorSnapshot {
    CompositorSnapshot {
        windows: windows.iter().map(window_info_from_niri).collect(),
        workspaces: workspaces
            .iter()
            .map(|w| WorkspaceInfo {
                id: w.id,
                focused: w.is_focused,
                output: w.output.clone(),
            })
            .collect(),
        outputs: outputs
            .iter()
            .filter_map(|(name, o)| {
                o.logical
                    .as_ref()
                    .map(|l| (name.clone(), (f64::from(l.width), f64::from(l.height))))
            })
            .collect(),
    }
}

/// The niri actions that carry out one core command.
pub(crate) fn niri_actions_for(cmd: Command) -> Vec<Action> {
    match cmd {
        Command::Nudge { id, by } => vec![Action::MoveFloatingWindow {
            id: Some(id),
            x: PositionChange::AdjustFixed(by.dx),
            y: PositionChange::AdjustFixed(by.dy),
        }],
        Command::FocusWindow(id) => vec![Action::FocusWindow { id }],
        Command::FocusTiling => vec![Action::FocusTiling {}],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::geometry::Nudge;

    fn layout(pos: Option<(f64, f64)>) -> WindowLayout {
        WindowLayout {
            pos_in_scrolling_layout: None,
            tile_size: (812.0, 612.0),
            window_size: (800, 600),
            tile_pos_in_workspace_view: pos,
            window_offset_in_tile: (6.0, 6.0),
        }
    }

    fn geo(x: f64, y: f64, w: f64, h: f64) -> Geometry {
        Geometry { x, y, w, h }
    }

    /// `Action` has no `PartialEq`; compare through its JSON form.
    fn json(actions: &[Action]) -> serde_json::Value {
        serde_json::to_value(actions).unwrap()
    }

    #[test]
    fn maps_a_nudge_to_a_relative_floating_move() {
        let expected = [Action::MoveFloatingWindow {
            id: Some(7),
            x: PositionChange::AdjustFixed(-10000.0),
            y: PositionChange::AdjustFixed(0.0),
        }];
        assert_eq!(
            json(&niri_actions_for(Command::Nudge {
                id: 7,
                by: Nudge {
                    dx: -10000.0,
                    dy: 0.0
                }
            })),
            json(&expected)
        );
    }

    #[test]
    fn maps_focus_commands_to_focus_actions() {
        assert_eq!(
            json(&niri_actions_for(Command::FocusWindow(9))),
            json(&[Action::FocusWindow { id: 9 }])
        );
        assert_eq!(
            json(&niri_actions_for(Command::FocusTiling)),
            json(&[Action::FocusTiling {}])
        );
    }

    #[test]
    fn layout_with_position_yields_geo_from_tile_pos_and_window_size_and_without_yields_none() {
        assert_eq!(
            geometry_from_layout(&layout(Some((100.0, 228.0)))),
            Some(geo(100.0, 228.0, 800.0, 600.0))
        );
        assert_eq!(geometry_from_layout(&layout(None)), None);
    }
}
