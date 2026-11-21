//! Connection accepting and request reading for the stub HTTP servers in the
//! CLI integration tests.
//!
//! A stub accepts each connection with a time limit ([`accept_expected`]), so
//! a client that sends one request less than the script fails the test and
//! does not make it hang.
//!
//! A client can send a request in more than one write, for example the head
//! first and the body later. A stub that reads once can miss the body. It then
//! closes the socket with unread data, and Windows aborts the connection
//! (`os error 10053`). Every stub reads the request with [`read_request`].

use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

/// The longest time a stub waits for the next part of a request.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// The longest time a stub waits for a client to connect.
const ACCEPT_TIMEOUT: Duration = Duration::from_secs(30);
/// The pause between two accept attempts.
const ACCEPT_POLL: Duration = Duration::from_millis(10);

/// Accept one connection, or return `None` after `timeout`.
///
/// The listener becomes non-blocking. The accepted stream is blocking.
pub fn accept_within(listener: &TcpListener, timeout: Duration) -> Option<TcpStream> {
    let deadline = Instant::now() + timeout;
    listener
        .set_nonblocking(true)
        .expect("non-blocking listener");
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).expect("blocking stream");
                return Some(stream);
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(ACCEPT_POLL);
            }
            Err(err) => panic!("accept request: {err}"),
        }
    }
}

/// Accept one connection within [`ACCEPT_TIMEOUT`], or return `None`.
///
/// A client that sends one request less than the script then gives a failed
/// assertion on the captured requests, not a test that hangs.
pub fn accept_before_deadline(listener: &TcpListener) -> Option<TcpStream> {
    accept_within(listener, ACCEPT_TIMEOUT)
}

/// Accept the connection for scripted response number `position` (from 1).
///
/// Panics when no client connects within [`ACCEPT_TIMEOUT`], and names the
/// missing request.
pub fn accept_expected(listener: &TcpListener, position: usize) -> TcpStream {
    accept_before_deadline(listener).unwrap_or_else(|| {
        panic!(
            "no client connected for scripted response {position} within {} seconds",
            ACCEPT_TIMEOUT.as_secs()
        )
    })
}

/// Read one full HTTP request from `stream`: the head, then the body that the
/// `content-length` header names.
pub fn read_request(stream: &mut TcpStream) -> String {
    // Accepted sockets can inherit a polling listener's nonblocking mode.
    stream.set_nonblocking(false).expect("set blocking reads");
    stream
        .set_read_timeout(Some(READ_TIMEOUT))
        .expect("set read timeout");
    read_message(stream)
}

/// Read one HTTP request from `reader` and return it as text.
///
/// Panics when the client closes the connection before the request ends, so
/// a truncated request fails the test instead of a weaker body assertion.
pub fn read_message(reader: &mut impl Read) -> String {
    const HEAD_END: &[u8] = b"\r\n\r\n";
    let mut data = Vec::new();
    let mut chunk = [0_u8; 4096];
    let head_len = loop {
        if let Some(pos) = data.windows(HEAD_END.len()).position(|w| w == HEAD_END) {
            break pos + HEAD_END.len();
        }
        read_chunk(reader, &mut chunk, &mut data, "head");
    };
    let total_len = head_len + content_length(&data[..head_len]);
    while data.len() < total_len {
        read_chunk(reader, &mut chunk, &mut data, "body");
    }
    data.truncate(total_len);
    String::from_utf8_lossy(&data).into_owned()
}

fn read_chunk(reader: &mut impl Read, chunk: &mut [u8], data: &mut Vec<u8>, part: &str) {
    let read = reader
        .read(chunk)
        .unwrap_or_else(|err| panic!("read request {part}: {err}"));
    assert_ne!(
        read, 0,
        "the client closed the connection before the request {part} ended"
    );
    data.extend_from_slice(&chunk[..read]);
}

/// The value of the `content-length` header in `head`, or 0 when it is absent.
fn content_length(head: &[u8]) -> usize {
    String::from_utf8_lossy(head)
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().expect("numeric content-length"))
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{ACCEPT_POLL, READ_TIMEOUT, accept_within, read_message, read_request};

    const HEAD: &str = "POST /v1/review HTTP/1.1\r\nHost: localhost\r\nContent-Length: 11\r\n\r\n";
    const BODY: &str = "{\"ok\":true}";
    const FRAGMENT_DELAY: Duration = Duration::from_millis(100);

    /// A reader that gives back one chunk for each `read` call.
    struct Chunks(VecDeque<Vec<u8>>);

    impl Read for Chunks {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let Some(chunk) = self.0.pop_front() else {
                return Ok(0);
            };
            buf[..chunk.len()].copy_from_slice(&chunk);
            Ok(chunk.len())
        }
    }

    fn chunks(parts: &[&str]) -> Chunks {
        Chunks(parts.iter().map(|part| part.as_bytes().to_vec()).collect())
    }

    #[test]
    fn reads_a_body_that_arrives_after_the_head() {
        let request = read_message(&mut chunks(&[HEAD, BODY]));
        assert_eq!(request, format!("{HEAD}{BODY}"));
    }

    #[test]
    fn reads_a_request_split_at_any_byte() {
        let full = format!("{HEAD}{BODY}");
        for split in 1..full.len() {
            let parts: [&str; 2] = full.split_at(split).into();
            let request = read_message(&mut chunks(&parts));
            assert_eq!(request, full, "split at byte {split}");
        }
    }

    #[test]
    fn stops_after_the_head_when_there_is_no_body() {
        let head = "GET /v1/status HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let mut reader = chunks(&[head, "not part of this request"]);
        assert_eq!(read_message(&mut reader), head);
    }

    #[test]
    fn matches_the_content_length_header_in_any_case() {
        let head = "POST / HTTP/1.1\r\ncontent-length: 2\r\n\r\n";
        assert_eq!(
            read_message(&mut chunks(&[head, "{}"])),
            format!("{head}{{}}")
        );
    }

    fn delayed_request_after_acceptance(explicit_nonblocking: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let mut client =
            TcpStream::connect(listener.local_addr().expect("stub addr")).expect("connect");
        client.set_nodelay(true).expect("no delay");
        let deadline = Instant::now() + READ_TIMEOUT;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "client was not accepted");
                    thread::sleep(ACCEPT_POLL);
                }
                Err(error) => panic!("accept connected client: {error}"),
            }
        };
        if explicit_nonblocking {
            stream.set_nonblocking(true).expect("nonblocking stream");
        }
        thread::scope(|scope| {
            scope.spawn(move || {
                // The header is withheld until acceptance, then arrives after the read starts.
                thread::sleep(FRAGMENT_DELAY);
                client.write_all(HEAD.as_bytes()).expect("write head");
                thread::sleep(FRAGMENT_DELAY);
                client.write_all(BODY.as_bytes()).expect("write body");
            });
            read_request(&mut stream)
        })
    }

    #[test]
    fn reads_a_delayed_request_from_a_nonblocking_listener() {
        assert_eq!(
            delayed_request_after_acceptance(false),
            format!("{HEAD}{BODY}")
        );
    }

    #[test]
    fn reads_a_delayed_request_from_a_nonblocking_stream() {
        assert_eq!(
            delayed_request_after_acceptance(true),
            format!("{HEAD}{BODY}")
        );
    }

    #[test]
    fn reads_a_body_sent_in_a_second_write_over_tcp() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub");
        let addr = listener.local_addr().expect("stub addr");
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).expect("connect");
            stream.set_nodelay(true).expect("no delay");
            stream.write_all(HEAD.as_bytes()).expect("write head");
            stream.flush().expect("flush head");
            thread::sleep(FRAGMENT_DELAY);
            stream.write_all(BODY.as_bytes()).expect("write body");
            stream
        });
        let (mut stream, _) = listener.accept().expect("accept");
        let request = read_request(&mut stream);
        drop(client.join().expect("client thread"));
        assert_eq!(request, format!("{HEAD}{BODY}"));
    }

    #[test]
    fn accept_within_gives_up_when_no_client_connects() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub");
        assert!(accept_within(&listener, Duration::from_millis(50)).is_none());
    }

    #[test]
    fn accept_within_returns_a_blocking_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub");
        let mut client =
            TcpStream::connect(listener.local_addr().expect("stub addr")).expect("connect");
        let mut stream = accept_within(&listener, READ_TIMEOUT).expect("client accepted");
        client
            .write_all(format!("{HEAD}{BODY}").as_bytes())
            .expect("write request");
        assert_eq!(read_request(&mut stream), format!("{HEAD}{BODY}"));
    }
}
