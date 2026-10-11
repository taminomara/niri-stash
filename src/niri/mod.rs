//! The thin niri IPC client: one connection per request with timeouts, and the event-stream
//! reader. Translation to and from the core's types lives in `translate`.

pub(crate) mod translate;

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use niri_ipc::{Event, Reply, Request};

/// Read and write timeout for every request connection.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(1);

/// What one read from the event stream produced.
#[derive(Debug)]
pub(crate) enum StreamItem {
    Event(Event),
    /// A line niri sent that this build cannot parse (a newer event type); carries the reason.
    Unparseable(String),
    /// niri closed the stream (0 bytes read).
    EndOfStream,
}

/// Reads events one line at a time from a stream niri is sending events on.
pub(crate) struct EventStream {
    reader: BufReader<UnixStream>,
    line: String,
}

impl EventStream {
    #[cfg(test)]
    pub(crate) fn from_stream(stream: UnixStream) -> Self {
        Self::from_reader(BufReader::new(stream))
    }

    fn from_reader(reader: BufReader<UnixStream>) -> Self {
        Self {
            reader,
            line: String::new(),
        }
    }

    /// The underlying socket, for waiting on it.
    pub(crate) fn socket(&self) -> &UnixStream {
        self.reader.get_ref()
    }

    /// True when a read would be served from memory, so waiting on the socket would be wrong.
    pub(crate) fn has_buffered(&self) -> bool {
        !self.reader.buffer().is_empty()
    }

    /// The next event, a skipped line, or end of stream. An I/O error is an error.
    pub(crate) fn next_item(&mut self) -> io::Result<StreamItem> {
        self.line.clear();
        if self.reader.read_line(&mut self.line)? == 0 {
            return Ok(StreamItem::EndOfStream);
        }
        match serde_json::from_str(&self.line) {
            Ok(event) => Ok(StreamItem::Event(event)),
            // The serde message names the unknown variant; the raw line is left out because it
            // may carry a window title (plan D-18).
            Err(err) => Ok(StreamItem::Unparseable(err.to_string())),
        }
    }
}

/// `send_request_with_timeout` with the default `REQUEST_TIMEOUT`.
pub(crate) fn send_request(path: &Path, req: &Request) -> io::Result<Reply> {
    send_request_with_timeout(path, req, REQUEST_TIMEOUT)
}

/// Open the event stream: connect, send `Request::EventStream`, check the `Handled` reply, and
/// hand back a reader positioned at the first event. The stream itself has no read timeout.
pub(crate) fn open_event_stream(path: &Path) -> io::Result<EventStream> {
    let mut reader = open_request(path, &Request::EventStream, None)?;
    match read_reply(&mut reader)? {
        Ok(_) => Ok(EventStream::from_reader(reader)),
        Err(message) => Err(io::Error::other(format!(
            "niri refused the event stream: {message}"
        ))),
    }
}

/// Connect to the niri socket at `path`, send one request, and return niri's reply. The connection
/// carries `timeout` for reads and writes and is dropped afterwards.
fn send_request_with_timeout(path: &Path, req: &Request, timeout: Duration) -> io::Result<Reply> {
    let mut reader = open_request(path, req, Some(timeout))?;
    read_reply(&mut reader)
}

/// Connect, send one request line, and close the write half. niri answers one request per
/// connection. `read_timeout` is `None` for the event stream, which must block indefinitely.
fn open_request(
    path: &Path,
    req: &Request,
    read_timeout: Option<Duration>,
) -> io::Result<BufReader<UnixStream>> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(read_timeout)?;
    stream.set_write_timeout(Some(REQUEST_TIMEOUT))?;
    let mut line = serde_json::to_string(req).map_err(io::Error::other)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    stream.shutdown(std::net::Shutdown::Write)?;
    Ok(BufReader::new(stream))
}

fn read_reply(reader: &mut BufReader<UnixStream>) -> io::Result<Reply> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "niri closed the connection before replying",
        ));
    }
    serde_json::from_str(&line).map_err(io::Error::other)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_one_event_per_line_from_the_stream() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let mut niri = theirs;
        niri.write_all(b"{\"WindowFocusChanged\":{\"id\":52}}\n{\"WindowClosed\":{\"id\":71}}\n")
            .unwrap();
        let mut reader = EventStream::from_stream(ours);
        let first = reader.next_item().unwrap();
        let second = reader.next_item().unwrap();
        assert!(
            matches!(
                first,
                StreamItem::Event(Event::WindowFocusChanged { id: Some(52) })
            ),
            "{first:?}"
        );
        assert!(
            matches!(second, StreamItem::Event(Event::WindowClosed { id: 71 })),
            "{second:?}"
        );
    }

    #[test]
    fn reports_end_of_stream_as_eof_not_an_error() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        drop(theirs);
        let mut reader = EventStream::from_stream(ours);
        let got = reader.next_item().unwrap();
        assert!(matches!(got, StreamItem::EndOfStream), "{got:?}");
    }

    #[test]
    fn skips_a_line_it_cannot_parse_and_keeps_reading() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let mut niri = theirs;
        niri.write_all(b"{\"SomeFutureEvent\":{\"x\":1}}\n{\"WindowClosed\":{\"id\":71}}\n")
            .unwrap();
        let mut reader = EventStream::from_stream(ours);
        let first = reader.next_item().unwrap();
        let second = reader.next_item().unwrap();
        assert!(
            matches!(&first, StreamItem::Unparseable(reason) if reason.contains("SomeFutureEvent")),
            "{first:?}"
        );
        assert!(
            matches!(second, StreamItem::Event(Event::WindowClosed { id: 71 })),
            "{second:?}"
        );
    }

    /// A stand-in for niri's socket: accepts one connection, reads one line, answers with `reply`
    /// (or never answers when `reply` is `None`), and hands back what it read.
    fn fake_niri(
        reply: Option<&'static str>,
    ) -> (std::path::PathBuf, std::thread::JoinHandle<String>) {
        use std::os::unix::net::UnixListener;
        let dir = std::env::temp_dir().join(format!(
            "niri-stash-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("niri.sock");
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
                std::thread::sleep(Duration::from_secs(3));
            }
            line
        });
        (path, handle)
    }

    #[test]
    fn sends_one_request_and_returns_niris_reply() {
        let (path, server) = fake_niri(Some("{\"Ok\":\"Handled\"}\n"));
        let reply = send_request(&path, &Request::FocusedWindow).unwrap();
        assert!(
            matches!(reply, Ok(niri_ipc::Response::Handled)),
            "{reply:?}"
        );
        assert_eq!(server.join().unwrap().trim_end(), "\"FocusedWindow\"");
    }

    #[test]
    fn a_request_with_no_reply_times_out_within_the_deadline() {
        let (path, _server) = fake_niri(None);
        let started = std::time::Instant::now();
        let err =
            send_request_with_timeout(&path, &Request::FocusedWindow, Duration::from_millis(200))
                .unwrap_err();
        assert!(
            matches!(
                err.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ),
            "{err:?}"
        );
        assert!(
            started.elapsed() < Duration::from_millis(1500),
            "took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn subscribing_consumes_the_handshake_and_yields_the_events_after_it() {
        let (path, server) = fake_niri(Some(
            "{\"Ok\":\"Handled\"}\n{\"WindowClosed\":{\"id\":9}}\n",
        ));
        let mut reader = open_event_stream(&path).unwrap();
        let first = reader.next_item().unwrap();
        assert!(
            matches!(first, StreamItem::Event(Event::WindowClosed { id: 9 })),
            "{first:?}"
        );
        assert_eq!(server.join().unwrap().trim_end(), "\"EventStream\"");
    }
}
