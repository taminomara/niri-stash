//! What the core hears from the compositor, in the core's own terms. The niri adapter translates
//! niri's events and queries into these.

use std::collections::HashMap;

use crate::core::geometry::Geometry;
use crate::core::{WindowId, WorkspaceId};

/// A window as the compositor reports it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WindowInfo {
    pub(crate) id: WindowId,
    /// `None` while the window is being dragged or moved between workspaces.
    pub(crate) workspace: Option<WorkspaceId>,
    pub(crate) floating: bool,
    pub(crate) focused: bool,
    /// `None` when the window has no position: tiled, fullscreen, or in the user's hand.
    pub(crate) geometry: Option<Geometry>,
    /// When the window last held focus, by the compositor's own debounced record; `None` if it
    /// never has.
    pub(crate) focus_timestamp: Option<FocusTimestamp>,
}

/// A moment on the compositor's clock. Orders by time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FocusTimestamp {
    pub(crate) secs: u64,
    pub(crate) nanos: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WorkspaceInfo {
    pub(crate) id: WorkspaceId,
    pub(crate) focused: bool,
    pub(crate) output: Option<String>,
}

/// Everything a press needs to know, fetched fresh when the press arrives.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CompositorSnapshot {
    pub(crate) windows: Vec<WindowInfo>,
    pub(crate) workspaces: Vec<WorkspaceInfo>,
    /// Logical size of each output, by name.
    pub(crate) outputs: HashMap<String, (f64, f64)>,
}

impl CompositorSnapshot {
    /// The focused workspace, if any.
    pub(crate) fn focused_workspace(&self) -> Option<&WorkspaceInfo> {
        self.workspaces.iter().find(|w| w.focused)
    }

    /// Logical size of the output the focused workspace is on, or `(0, 0)` when unknown.
    pub(crate) fn focused_output_size(&self) -> (f64, f64) {
        self.focused_workspace()
            .and_then(|ws| ws.output.as_ref())
            .and_then(|name| self.outputs.get(name))
            .copied()
            .unwrap_or((0.0, 0.0))
    }
}

/// One thing the compositor reported.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CompositorEvent {
    /// A window's geometry changed.
    GeometryChanged { id: WindowId, geometry: Geometry },
    /// Keyboard focus moved to this window, or to no window.
    FocusChanged(Option<WindowId>),
    /// A window opened or changed (floating state, workspace, focus, geometry).
    WindowChanged(WindowInfo),
    /// A window closed.
    WindowClosed(WindowId),
}
