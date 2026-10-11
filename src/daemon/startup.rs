//! Startup: reaching niri with bounded retries, and claiming the hotkey socket (plan D-14).

use std::io;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Total time the daemon keeps trying to reach niri at startup.
pub(crate) const NIRI_CONNECT_WINDOW: Duration = Duration::from_secs(60);

/// `$XDG_RUNTIME_DIR/niri-stash/`.
pub(crate) fn runtime_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join("niri-stash"))
}

/// Call `attempt` until it succeeds or `total` has elapsed on `now`, sleeping `backoff_delays()`
/// between tries through `sleep`. Logs the first failure at info and then about every 10 s.
pub(crate) fn retry_for<T>(
    total: Duration,
    mut attempt: impl FnMut() -> io::Result<T>,
    mut sleep: impl FnMut(Duration),
    mut now: impl FnMut() -> Instant,
) -> Option<T> {
    let start = now();
    let mut last_warn = start;
    let mut tries = 0u32;
    for delay in backoff_delays() {
        match attempt() {
            Ok(value) => return Some(value),
            Err(err) => {
                tries += 1;
                let elapsed = now() - start;
                if tries == 1 {
                    log::info!(
                        "niri not reachable yet: {err}; retrying for up to {} s",
                        total.as_secs()
                    );
                } else if now() - last_warn >= Duration::from_secs(10) {
                    log::warn!(
                        "still waiting for niri after {} tries, {} s: {err}",
                        tries,
                        elapsed.as_secs()
                    );
                    last_warn = now();
                }
                if elapsed >= total {
                    return None;
                }
                // Sleep no further than the end of the window; the next try is the last.
                sleep(delay.min(total - elapsed));
            }
        }
    }
    None
}

/// The reconnect delays: 100 ms, doubling, capped at 2 s.
fn backoff_delays() -> impl Iterator<Item = Duration> {
    std::iter::successors(Some(Duration::from_millis(100)), |d| {
        Some((*d * 2).min(Duration::from_secs(2)))
    })
}

/// Why the hotkey socket could not be bound.
#[derive(Debug)]
pub(crate) enum HotkeySocketError {
    /// A live daemon answered on the socket.
    OwnedByAnotherDaemon,
    Io(io::Error),
}

/// Bind the hotkey socket at `path`, mode 0600. An existing file is probed first: a refused
/// connection means a stale file, which is removed; an accepted one means another daemon.
pub(crate) fn bind_hotkey_socket(path: &Path) -> Result<UnixListener, HotkeySocketError> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(HotkeySocketError::Io)?;
    }
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err(HotkeySocketError::OwnedByAnotherDaemon);
        }
        log::info!("removing stale hotkey socket {}", path.display());
        std::fs::remove_file(path).map_err(HotkeySocketError::Io)?;
    }
    let listener = UnixListener::bind(path).map_err(HotkeySocketError::Io)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(HotkeySocketError::Io)?;
    Ok(listener)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_starts_at_100ms_doubles_and_caps_at_2s() {
        let delays: Vec<u64> = backoff_delays()
            .take(7)
            .map(|d| d.as_millis() as u64)
            .collect();
        assert_eq!(delays, vec![100, 200, 400, 800, 1600, 2000, 2000]);
    }

    #[test]
    fn retry_gives_up_after_the_window_and_returns_the_first_success_before_it() {
        // Clock and sleep are simulated: each sleep advances the clock.
        let start = Instant::now();
        let clock = std::cell::Cell::new(start);
        let mut attempts = 0;
        let got: Option<()> = retry_for(
            Duration::from_secs(60),
            || {
                attempts += 1;
                Err(io::Error::new(io::ErrorKind::NotFound, "no socket"))
            },
            |d| clock.set(clock.get() + d),
            || clock.get(),
        );
        assert_eq!(got, None);
        assert!(
            clock.get() - start >= Duration::from_secs(60),
            "kept trying for the whole window"
        );
        assert!(attempts > 30, "attempts: {attempts}");

        let clock = std::cell::Cell::new(start);
        let mut attempts = 0;
        let got = retry_for(
            Duration::from_secs(60),
            || {
                attempts += 1;
                if attempts == 3 {
                    Ok(42)
                } else {
                    Err(io::Error::new(io::ErrorKind::ConnectionRefused, "not yet"))
                }
            },
            |d| clock.set(clock.get() + d),
            || clock.get(),
        );
        assert_eq!(got, Some(42));
        assert_eq!(
            clock.get() - start,
            Duration::from_millis(300),
            "slept 100 + 200 before the third try"
        );
    }

    #[test]
    fn retry_makes_a_last_attempt_at_the_end_of_the_window_instead_of_sleeping_past_it() {
        let start = Instant::now();
        let clock = std::cell::Cell::new(start);
        let mut attempts = 0;
        let got: Option<()> = retry_for(
            Duration::from_secs(1),
            || {
                attempts += 1;
                Err(io::Error::new(io::ErrorKind::NotFound, "no socket"))
            },
            |d| clock.set(clock.get() + d),
            || clock.get(),
        );
        assert_eq!(got, None);
        // Sleeps of 100, 200, 400 ms, then the 300 ms left, each followed by a try.
        assert_eq!(
            attempts, 5,
            "the remainder of the window is used, not slept away"
        );
        assert_eq!(clock.get() - start, Duration::from_secs(1));
    }

    fn temp_sock(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("niri-stash-daemon-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sock");
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn binding_over_a_stale_socket_file_succeeds_and_over_a_live_daemon_is_refused() {
        let stale = temp_sock("stale");
        drop(UnixListener::bind(&stale).unwrap()); // the file stays behind, nobody listens
        assert!(bind_hotkey_socket(&stale).is_ok());

        let live = temp_sock("live");
        let _owner = UnixListener::bind(&live).unwrap();
        assert!(matches!(
            bind_hotkey_socket(&live),
            Err(HotkeySocketError::OwnedByAnotherDaemon)
        ));
    }
}
