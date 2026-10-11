//! niri-stash: stash floating windows on niri into slivers at the nearest screen edge.
//!
//! `core` decides; `niri`, `hotkey`, `state_file`, and `daemon` move bytes and time. The public
//! surface is the two entry points `main` dispatches to.

pub(crate) mod core;
pub(crate) mod daemon;
pub(crate) mod hotkey;
pub(crate) mod niri;
pub(crate) mod state_file;

pub use core::focus::FocusPolicy;
pub use hotkey::client::EXIT_USAGE;
pub use hotkey::protocol::HotkeyRequest;

/// Run the daemon until it exits; returns its exit code (see `daemon::exit`).
pub fn run_daemon(focus_policy: FocusPolicy) -> i32 {
    daemon::run(focus_policy)
}

/// Send one hotkey request to the running daemon; returns the client's exit code.
pub fn run_client(req: HotkeyRequest) -> i32 {
    hotkey::client::run(req)
}
