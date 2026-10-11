//! What the core asks the compositor to do, in the core's own terms. The niri adapter translates
//! these into niri actions.

use crate::core::geometry::Nudge;
use crate::core::WindowId;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Command {
    /// Move the floating window by a relative amount.
    Nudge { id: WindowId, by: Nudge },
    /// Give keyboard focus to the window.
    FocusWindow(WindowId),
    /// Move keyboard focus to the tiled layout.
    FocusTiling,
}
