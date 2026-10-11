//! The hotkey client: `niri-stash stash` / `toggle` / `unstash`.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use crate::hotkey::protocol::{self, HotkeyReply, HotkeyRequest};

/// Long enough for a whole press: every niri request inside one has a 1 s timeout.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// Exit codes of the client.
const EXIT_OK: i32 = 0;
const EXIT_UNREACHABLE: i32 = 1;
const EXIT_ERR_REPLY: i32 = 2;
const EXIT_NO_REPLY: i32 = 3;
pub const EXIT_USAGE: i32 = 64;

/// Send one request to the daemon's socket under `$XDG_RUNTIME_DIR` and return the exit code.
pub(crate) fn run(req: HotkeyRequest) -> i32 {
    let Some(dir) = crate::daemon::runtime_dir() else {
        let _ = writeln!(io::stderr(), "niri-stash: XDG_RUNTIME_DIR is not set");
        return EXIT_UNREACHABLE;
    };
    send_request(&dir.join("sock"), req, REPLY_TIMEOUT, &mut io::stderr())
}

/// Send `req` to the daemon at `socket` and return the process exit code, writing any reason to
/// `stderr`.
fn send_request(
    socket: &Path,
    req: HotkeyRequest,
    timeout: Duration,
    stderr: &mut dyn Write,
) -> i32 {
    let stream = match UnixStream::connect(socket) {
        Ok(stream) => stream,
        Err(err) => {
            let _ = writeln!(
                stderr,
                "niri-stash: cannot reach the daemon at {}: {err}",
                socket.display()
            );
            return EXIT_UNREACHABLE;
        }
    };
    let line = match exchange(stream, req, timeout) {
        Ok(line) => line,
        Err(err) => {
            let _ = writeln!(stderr, "niri-stash: no reply from the daemon: {err}");
            return EXIT_NO_REPLY;
        }
    };
    match protocol::parse_reply_line(&line) {
        Some(HotkeyReply::Ok) => EXIT_OK,
        Some(reply @ HotkeyReply::Err { .. }) => {
            let _ = write!(
                stderr,
                "niri-stash: {}",
                protocol::format_reply_line(&reply)
            );
            EXIT_ERR_REPLY
        }
        None => {
            let _ = writeln!(
                stderr,
                "niri-stash: unreadable reply from the daemon: {:?}",
                line.trim_end()
            );
            EXIT_NO_REPLY
        }
    }
}

/// Write the request line, close the write half, and read the one reply line.
fn exchange(mut stream: UnixStream, req: HotkeyRequest, timeout: Duration) -> io::Result<String> {
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(protocol::format_request_line(req).as_bytes())?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// A stand-in daemon: accepts one connection, reads the request, writes `reply` (or nothing).
    fn fake_daemon(
        reply: Option<&'static str>,
    ) -> (std::path::PathBuf, std::thread::JoinHandle<String>) {
        let dir = std::env::temp_dir().join(format!(
            "niri-stash-client-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sock");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if let Some(reply) = reply {
                reader.get_mut().write_all(reply.as_bytes()).unwrap();
            } else {
                std::thread::sleep(Duration::from_millis(600));
            }
            line
        });
        (path, handle)
    }

    #[test]
    fn exits_zero_on_an_ok_reply_after_sending_the_request_line() {
        let (path, daemon) = fake_daemon(Some("ok\n"));
        let mut err = Vec::new();
        let code = send_request(&path, HotkeyRequest::Toggle, REPLY_TIMEOUT, &mut err);
        assert_eq!(code, EXIT_OK);
        assert_eq!(daemon.join().unwrap(), "toggle\n");
        assert!(err.is_empty());
    }

    #[test]
    fn exits_one_when_the_daemon_socket_does_not_exist() {
        let mut err = Vec::new();
        let code = send_request(
            Path::new("/nonexistent/niri-stash/sock"),
            HotkeyRequest::Toggle,
            REPLY_TIMEOUT,
            &mut err,
        );
        assert_eq!(code, EXIT_UNREACHABLE);
        assert!(String::from_utf8(err)
            .unwrap()
            .contains("cannot reach the daemon"));
    }

    #[test]
    fn exits_two_and_prints_the_reason_on_an_err_reply() {
        let (path, _daemon) = fake_daemon(Some(
            "err state-unwritable rename state.json: Read-only file system\n",
        ));
        let mut err = Vec::new();
        let code = send_request(&path, HotkeyRequest::Unstash, REPLY_TIMEOUT, &mut err);
        assert_eq!(code, EXIT_ERR_REPLY);
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "niri-stash: err state-unwritable rename state.json: Read-only file system\n"
        );
    }

    #[test]
    fn exits_three_when_no_reply_arrives_within_the_timeout() {
        let (path, _daemon) = fake_daemon(None);
        let mut err = Vec::new();
        let code = send_request(
            &path,
            HotkeyRequest::Toggle,
            Duration::from_millis(200),
            &mut err,
        );
        assert_eq!(code, EXIT_NO_REPLY);
    }
}
