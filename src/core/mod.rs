//! The pure core: what the daemon decides, with no I/O of its own.

pub(crate) mod command;
pub(crate) mod engine;
pub(crate) mod focus;
pub(crate) mod geometry;
pub(crate) mod input;
pub(crate) mod ports;

/// A niri window id, stable for the window's lifetime within one niri instance.
pub(crate) type WindowId = u64;
/// A niri workspace id, stable for the workspace's lifetime within one niri instance.
pub(crate) type WorkspaceId = u64;
