//! Request reading for the stub HTTP servers in the CLI integration tests.
//!
//! A client can send a request in more than one write, for example the head
//! first and the body later. A stub that reads once can miss the body. It then
//! closes the socket with unread data, and Windows aborts the connection
//! (`os error 10053`). Every stub reads the request with [`read_request`].

use std::io::Read;
use std::net::TcpStream;
use std::time::Duration;

/// The longest time a stub waits for the next part of a request.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Read one full HTTP request from `stream`: the head, then the body that the
/// `content-length` header names.
pub fn read_request(stream: &mut TcpStream) -> String {
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
    use std::time::Duration;

    use super::{read_message, read_request};

    const HEAD: &str = "POST /v1/review HTTP/1.1\r\nHost: localhost\r\nContent-Length: 11\r\n\r\n";
    const BODY: &str = "{\"ok\":true}";

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

    #[test]
    fn reads_a_body_sent_in_a_second_write_over_tcp() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub");
        let addr = listener.local_addr().expect("stub addr");
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).expect("connect");
            stream.set_nodelay(true).expect("no delay");
            stream.write_all(HEAD.as_bytes()).expect("write head");
            stream.flush().expect("flush head");
            thread::sleep(Duration::from_millis(100));
            stream.write_all(BODY.as_bytes()).expect("write body");
            stream
        });
        let (mut stream, _) = listener.accept().expect("accept");
        let request = read_request(&mut stream);
        drop(client.join().expect("client thread"));
        assert_eq!(request, format!("{HEAD}{BODY}"));
    }
}
