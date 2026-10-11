//! The running daemon: one thread that waits on niri's event stream and the hotkey socket with
//! `poll` and handles whatever is ready. Verified by hand on a live niri; the behavior it drives is
//! covered by the engine tests.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use niri_ipc::{Request as NiriRequest, Response, Window};
use rustix::event::{poll, PollFd, PollFlags, Timespec};

use super::exit;
use super::startup::{
    bind_hotkey_socket, retry_for, runtime_dir, HotkeySocketError, NIRI_CONNECT_WINDOW,
};
use crate::core::command::Command;
use crate::core::engine::StashEngine;
use crate::core::focus::FocusPolicy;
use crate::core::input::{CompositorSnapshot, WindowInfo};
use crate::core::ports::{Compositor, PortPair, StateStore};
use crate::hotkey::protocol::{self, HotkeyReply, ReplyErrorCode};
use crate::niri::{self, EventStream, StreamItem};
use crate::state_file::{self, StateFile};

/// A client whose request line is not complete this long after it became readable is dropped.
/// A backstop only: the client writes its whole line in one call, and a probe closes at once.
const CLIENT_TIMEOUT: Duration = Duration::from_millis(500);

/// The state store: the desired-state file.
struct StateFileStore {
    path: PathBuf,
}

impl StateStore for StateFileStore {
    fn save(&mut self, state: &StateFile) -> io::Result<()> {
        state_file::save(&self.path, state)
    }
}

/// The compositor port over the real niri socket, one connection per request.
struct NiriCompositor {
    socket: PathBuf,
}

impl NiriCompositor {
    fn request(&self, req: NiriRequest) -> io::Result<Response> {
        match niri::send_request(&self.socket, &req)? {
            Ok(response) => Ok(response),
            Err(message) => Err(io::Error::other(format!(
                "niri declined {req:?}: {message}"
            ))),
        }
    }

    fn niri_windows(&self) -> io::Result<Vec<Window>> {
        match self.request(NiriRequest::Windows)? {
            Response::Windows(windows) => Ok(windows),
            other => Err(io::Error::other(format!(
                "unexpected reply to Windows: {other:?}"
            ))),
        }
    }
}

impl Compositor for NiriCompositor {
    fn execute(&mut self, commands: &[Command]) -> io::Result<()> {
        for action in commands
            .iter()
            .flat_map(|c| niri::translate::niri_actions_for(*c))
        {
            self.request(NiriRequest::Action(action))?;
        }
        Ok(())
    }

    fn list_windows(&mut self) -> io::Result<Vec<WindowInfo>> {
        Ok(self
            .niri_windows()?
            .iter()
            .map(niri::translate::window_info_from_niri)
            .collect())
    }
}

type DaemonPorts = PortPair<StateFileStore, NiriCompositor>;

/// Fetch windows, workspaces and outputs in three requests.
fn fetch_snapshot(io: &mut DaemonPorts) -> io::Result<CompositorSnapshot> {
    let windows = io.compositor.niri_windows()?;
    let workspaces = match io.compositor.request(NiriRequest::Workspaces)? {
        Response::Workspaces(w) => w,
        other => {
            return Err(io::Error::other(format!(
                "unexpected reply to Workspaces: {other:?}"
            )))
        }
    };
    let outputs = match io.compositor.request(NiriRequest::Outputs)? {
        Response::Outputs(o) => o,
        other => {
            return Err(io::Error::other(format!(
                "unexpected reply to Outputs: {other:?}"
            )))
        }
    };
    Ok(niri::translate::snapshot_from_niri(
        &windows,
        &workspaces,
        &outputs,
    ))
}

/// Run the daemon to completion and return its exit code.
pub(crate) fn run(focus_policy: FocusPolicy) -> i32 {
    let Some(dir) = runtime_dir() else {
        log::error!("XDG_RUNTIME_DIR is not set");
        return exit::CONFIG;
    };
    let Some(niri_socket) = std::env::var_os("NIRI_SOCKET").map(PathBuf::from) else {
        log::error!("NIRI_SOCKET is not set; is niri running in this session?");
        return exit::CONFIG;
    };
    let instance = niri_socket.to_string_lossy().into_owned();
    let mut io = PortPair {
        store: StateFileStore {
            path: dir.join("state.json"),
        },
        compositor: NiriCompositor {
            socket: niri_socket.clone(),
        },
    };

    // 1. Reach niri.
    let Some(events) = retry_for(
        NIRI_CONNECT_WINDOW,
        || niri::open_event_stream(&niri_socket),
        std::thread::sleep,
        Instant::now,
    ) else {
        log::error!(
            "niri not reachable within {} s; exiting {}",
            NIRI_CONNECT_WINDOW.as_secs(),
            exit::NIRI_UNREACHABLE
        );
        return exit::NIRI_UNREACHABLE;
    };
    let snap = match fetch_snapshot(&mut io) {
        Ok(snap) => snap,
        Err(err) => {
            log::error!("could not fetch the initial snapshot: {err}");
            return exit::NIRI_UNREACHABLE;
        }
    };

    // 2. Re-apply the saved desired state before anyone can press a key.
    let mut engine = StashEngine::new(instance.clone(), focus_policy);
    let loaded = state_file::load(&io.store.path, &instance);
    log::info!(
        "instance {instance}; {} windows in the snapshot; focus policy {focus_policy:?}",
        snap.windows.len()
    );
    log::debug!("debug logging is enabled");
    engine.start_from_saved_state(loaded, &snap, &mut io);

    // 3. Open the hotkey socket.
    let sock = dir.join("sock");
    let listener = match bind_hotkey_socket(&sock) {
        Ok(l) => l,
        Err(HotkeySocketError::OwnedByAnotherDaemon) => {
            log::error!("another niri-stash daemon already owns {}", sock.display());
            return exit::CONFIG;
        }
        Err(HotkeySocketError::Io(err)) => {
            log::error!("cannot bind {}: {err}", sock.display());
            return exit::CONFIG;
        }
    };

    // 4. Serve until niri goes away.
    run_loop(events, listener, engine, io)
}

/// The loop: wait until niri, the listener, or a client that has not spoken yet is readable, then
/// handle exactly that. Nothing here blocks on anything that has not announced data,
/// so a press is never delayed by an idle client and a client never waits on niri's stream.
fn run_loop(
    mut events: EventStream,
    listener: UnixListener,
    mut engine: StashEngine,
    mut io: DaemonPorts,
) -> i32 {
    let mut clients: Vec<UnixStream> = Vec::new();
    loop {
        let mut fds = Vec::with_capacity(2 + clients.len());
        fds.push(PollFd::new(events.socket(), PollFlags::IN));
        fds.push(PollFd::new(&listener, PollFlags::IN));
        fds.extend(clients.iter().map(|c| PollFd::new(c, PollFlags::IN)));
        // Events already buffered in memory would never wake the socket: do not wait for them.
        let no_wait = Timespec::default();
        let timeout = events.has_buffered().then_some(&no_wait);
        match poll(&mut fds, timeout) {
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(err) => {
                log::error!("poll failed: {err}; exiting {}", exit::STREAM_ENDED);
                return exit::STREAM_ENDED;
            }
        }
        // Any flag, including hang-up or error, means a read will tell us what happened.
        let niri_ready = events.has_buffered() || !fds[0].revents().is_empty();
        let listener_ready = !fds[1].revents().is_empty();
        let ready_clients: Vec<bool> = fds[2..].iter().map(|f| !f.revents().is_empty()).collect();

        if niri_ready {
            match events.next_item() {
                Ok(StreamItem::Event(event)) => {
                    engine.on_compositor_events(niri::translate::events_from_niri(&event), &mut io)
                }
                Ok(StreamItem::Unparseable(reason)) => {
                    log::warn!("skipped an event this build cannot parse: {reason}")
                }
                Ok(StreamItem::EndOfStream) => return stream_ended("niri closed the event stream"),
                Err(err) => return stream_ended(&format!("event stream read error: {err}")),
            }
        }
        // Serve before accepting: `ready_clients` describes the clients as they were polled.
        for (index, ready) in ready_clients.iter().enumerate().rev() {
            if *ready {
                let stream = clients.swap_remove(index);
                serve_client(stream, &mut engine, &mut io);
            }
        }
        if listener_ready {
            match listener.accept() {
                Ok((stream, _)) => clients.push(stream),
                Err(err) => log::warn!("accept failed: {err}"),
            }
        }
    }
}

fn stream_ended(reason: &str) -> i32 {
    log::error!(
        "{reason}; exiting {} so systemd restarts the daemon",
        exit::STREAM_ENDED
    );
    exit::STREAM_ENDED
}

/// A client has something to say: read its one request line, carry out the press, and write the
/// one reply line. A client that connected and closed without a word is another
/// daemon probing whether this socket is live; it gets nothing.
fn serve_client(stream: UnixStream, engine: &mut StashEngine, io: &mut DaemonPorts) {
    let _ = stream.set_read_timeout(Some(CLIENT_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CLIENT_TIMEOUT));
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => return,
        Ok(_) => {}
        Err(err) => {
            log::info!("hotkey connection ended early: {err}");
            return;
        }
    }
    let reply = match protocol::parse_request_line(&line) {
        None => HotkeyReply::Err {
            code: ReplyErrorCode::UnknownCommand,
            message: format!("got {:?}", line.trim_end()),
        },
        Some(req) => {
            log::info!("press {req:?}");
            match fetch_snapshot(io) {
                Ok(snap) => engine.on_press(req, &snap, io),
                Err(err) => {
                    log::warn!("snapshot failed: {err}");
                    HotkeyReply::Err {
                        code: ReplyErrorCode::NiriUnavailable,
                        message: err.to_string(),
                    }
                }
            }
        }
    };
    let line = protocol::format_reply_line(&reply);
    if let Err(err) = reader.get_mut().write_all(line.as_bytes()) {
        log::info!("could not answer a hotkey client: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives the loop with no niri behind it: probes, a line that is not a request, and the
    /// stream closing. A press is not sent because it would need a live niri for its snapshot.
    #[test]
    fn probes_are_dropped_an_unknown_line_is_answered_and_the_loop_exits_when_niri_closes() {
        let dir = std::env::temp_dir().join(format!("niri-stash-loop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("sock");
        let _ = std::fs::remove_file(&sock);
        let listener = UnixListener::bind(&sock).unwrap();
        let (ours, niri_end) = UnixStream::pair().unwrap();
        let events = EventStream::from_stream(ours);
        let io = PortPair {
            store: StateFileStore {
                path: dir.join("state.json"),
            },
            compositor: NiriCompositor {
                socket: dir.join("no-niri"),
            },
        };
        let engine = StashEngine::new("test".into(), FocusPolicy::AltTabOnly);

        let client_sock = sock.clone();
        let driver = std::thread::spawn(move || {
            let pause = || std::thread::sleep(Duration::from_millis(30));
            // Two probes: connect and close without a word.
            drop(UnixStream::connect(&client_sock).unwrap());
            pause();
            drop(UnixStream::connect(&client_sock).unwrap());
            pause();
            // An idle client that stays connected and silent must not block the next one.
            let idle = UnixStream::connect(&client_sock).unwrap();
            pause();
            let mut talker = UnixStream::connect(&client_sock).unwrap();
            talker.write_all(b"status\n").unwrap();
            let mut reply = String::new();
            BufReader::new(&talker).read_line(&mut reply).unwrap();
            drop(idle);
            pause();
            // niri closes its stream.
            drop(niri_end);
            reply
        });
        let code = run_loop(events, listener, engine, io);
        let reply = driver.join().unwrap();
        assert_eq!(reply, "err unknown-command got \"status\"\n");
        assert_eq!(code, exit::STREAM_ENDED);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Debug-level logs of where windows land must be available at runtime (plan D-18).
    #[test]
    fn debug_logging_is_compiled_in() {
        assert!(
            log::STATIC_MAX_LEVEL >= log::LevelFilter::Debug,
            "static max level is {:?}",
            log::STATIC_MAX_LEVEL
        );
    }
}
