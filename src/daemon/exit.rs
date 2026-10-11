//! Daemon exit codes (plan D-15); the systemd unit keys its restart policy on them.

/// niri unreachable within the retry window, or no initial snapshot.
pub(super) const NIRI_UNREACHABLE: i32 = 69;
/// niri's event stream ended (EOF or read error), or the wait on it failed.
pub(super) const STREAM_ENDED: i32 = 75;
/// `XDG_RUNTIME_DIR` or `NIRI_SOCKET` unset, or another daemon owns the socket.
pub(super) const CONFIG: i32 = 78;
