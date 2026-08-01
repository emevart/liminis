//! A very small HTTP/1.1 server, on `std::net` and nothing else.
//!
//! # Why not a crate
//!
//! The viewer needs four routes and no middleware, and open question C-5 asks
//! for "downloaded it, one command, a picture in a minute". Every dependency
//! added here is a dependency in the way of that, and an HTTP server is the
//! canonical place where a project acquires forty transitive crates and an async
//! runtime in exchange for features none of these four routes use.
//!
//! What is given up is real and worth naming: no TLS, no HTTP/2, no keep-alive,
//! no compression, no concurrency beyond a thread per connection. The viewer
//! polls a handful of times a second over the loopback interface, so none of
//! those are load-bearing. If any of them ever becomes load-bearing, that is the
//! moment to take the dependency, not before.
//!
//! # What it deliberately cannot do
//!
//! There is no path that reaches the filesystem. The viewer is compiled into the
//! binary with `include_str!`, and every other route answers from memory. A
//! simulator that also serves files from disk is a simulator with a directory
//! traversal bug in it, and there is no reason for this one to have the
//! capability at all.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

/// The two methods the viewer uses. Everything else is answered 405 rather than
/// guessed at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Other,
}

/// A parsed request: enough of one to route on, and nothing more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    /// Path with the query string removed and percent escapes decoded.
    pub path: String,
    /// Raw query string, undecoded. No route needs it yet; it is kept so that
    /// adding one does not mean changing the parser.
    pub query: String,
    pub body: Vec<u8>,
}

/// A response, built in memory and written in one go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    pub fn html(body: &'static str) -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: body.as_bytes().to_vec(),
        }
    }

    /// The shape `/api/state` and `/api/profile/<field>` answer in.
    // `expect` rather than `allow`, and the difference is the point: an
    // unfulfilled expectation is itself a warning, so the day a route starts
    // returning JSON the compiler asks for this attribute back. `allow` would
    // sit here forever.
    //
    // Alive under `cfg(test)` — the tests below build both shapes — so the
    // expectation has to be scoped to the build where the lint actually fires,
    // or it is itself an unfulfilled expectation and therefore a warning.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the data routes answer 503 until the tick loop exists (wave 6 of the S0 plan)"
        )
    )]
    pub fn json(body: String) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            body: body.into_bytes(),
        }
    }

    /// The shape `/api/volume/<field>` answers in: a self-describing header and
    /// one byte per voxel.
    // Alive under `cfg(test)` — the tests below build both shapes — so the
    // expectation has to be scoped to the build where the lint actually fires,
    // or it is itself an unfulfilled expectation and therefore a warning.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the data routes answer 503 until the tick loop exists (wave 6 of the S0 plan)"
        )
    )]
    pub fn bytes(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type: "application/octet-stream",
            body,
        }
    }

    /// A refusal that says what is missing.
    ///
    /// Plain text rather than JSON because the reader is a person looking at a
    /// failed request in a browser's network tab, and the message is the whole
    /// payload.
    pub fn error(status: u16, message: &str) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8",
            body: message.as_bytes().to_vec(),
        }
    }
}

/// The reason phrase. Only the codes this server actually returns.
fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

/// The largest request this server will read.
///
/// A bound rather than a limit anyone should hit: the only body in the protocol
/// is a control message of a few dozen bytes. Without it a malformed
/// `Content-Length` allocates whatever it says.
const MAX_BODY: usize = 64 * 1024;

/// The longest request line and header block.
const MAX_HEAD: usize = 8 * 1024;

/// Decode `%XX` escapes and `+`.
///
/// Invalid escapes are left as written rather than rejected: a path that does
/// not decode also does not match a route, so it lands on the same 404 by a
/// shorter path.
fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    None => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Read one request off a stream.
///
/// Returns `Ok(None)` when the peer closed without sending anything, which is
/// what a browser doing connection housekeeping looks like and is not an error.
///
/// # Errors
///
/// Returns an error on a malformed request line, on headers longer than
/// [`MAX_HEAD`], on a body longer than [`MAX_BODY`], or on an I/O failure.
pub fn read_request(stream: impl Read) -> std::io::Result<Option<Request>> {
    let mut reader = BufReader::new(stream);

    let mut line = String::new();
    if reader.by_ref().take(MAX_HEAD as u64).read_line(&mut line)? == 0 {
        return Ok(None);
    }

    let mut parts = line.split_whitespace();
    let method = match parts.next() {
        Some("GET") => Method::Get,
        Some("POST") => Method::Post,
        Some(_) => Method::Other,
        None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "empty request line",
            ));
        }
    };
    let target = parts.next().unwrap_or("/");
    let (raw_path, query) = match target.split_once('?') {
        Some((p, q)) => (p, q.to_string()),
        None => (target, String::new()),
    };
    let path = percent_decode(raw_path);

    // Headers. Only `Content-Length` is acted on; the rest are read to get past
    // them, because the body starts after the blank line whether or not anyone
    // looked at what came before it.
    let mut content_length = 0usize;
    let mut consumed = line.len();
    loop {
        let mut header = String::new();
        let n = reader
            .by_ref()
            .take(MAX_HEAD as u64)
            .read_line(&mut header)?;
        if n == 0 {
            break;
        }
        consumed += n;
        if consumed > MAX_HEAD {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "request head too large",
            ));
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }

    if content_length > MAX_BODY {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("body of {content_length} bytes is over the {MAX_BODY} byte bound"),
        ));
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    Ok(Some(Request {
        method,
        path,
        query,
        body,
    }))
}

/// Write a response and close.
///
/// `Connection: close` on every response, which is what makes a thread per
/// connection sound: the thread ends when the response does, so a client that
/// wanders off cannot hold one open.
///
/// # Errors
///
/// Returns an error if the write fails, which normally means the client went
/// away mid-response — the caller logs it and moves on rather than treating it
/// as a fault.
pub fn write_response(mut out: impl Write, response: &Response) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        response.status,
        reason(response.status),
        response.content_type,
        response.body.len(),
    );
    out.write_all(head.as_bytes())?;
    out.write_all(&response.body)?;
    out.flush()
}

/// Listen, and answer every request with `handler`.
///
/// One thread per connection, no pool. The handler is shared, so whatever it
/// closes over carries its own synchronisation — for the viewer that is the
/// simulation, which lives behind a mutex and is read for a frame at a time.
///
/// Blocks forever.
///
/// # Errors
///
/// Returns an error if the address cannot be bound. A failure on an individual
/// connection is not an error of the server: it is reported and the loop
/// continues.
pub fn serve<H>(listener: TcpListener, handler: H) -> std::io::Result<()>
where
    H: Fn(&Request) -> Response + Send + Sync + 'static,
{
    let handler = std::sync::Arc::new(handler);
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(err) => {
                eprintln!("liminis serve: dropped a connection: {err}");
                continue;
            }
        };
        let handler = std::sync::Arc::clone(&handler);
        std::thread::spawn(move || answer(stream, handler.as_ref()));
    }
    Ok(())
}

/// One connection, start to finish.
fn answer(stream: TcpStream, handler: &(impl Fn(&Request) -> Response + ?Sized)) {
    let peer = stream.peer_addr().ok();
    let response = match read_request(&stream) {
        Ok(Some(request)) => handler(&request),
        // A connection that closed without a request. Nothing to answer.
        Ok(None) => return,
        Err(err) => Response::error(400, &format!("could not read the request: {err}")),
    };
    if let Err(err) = write_response(&stream, &response) {
        // Normal when a browser cancels an in-flight fetch, which the viewer
        // does every time a field is toggled mid-frame.
        eprintln!(
            "liminis serve: could not answer {}: {err}",
            peer.map(|p| p.to_string()).unwrap_or_default()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> Option<Request> {
        read_request(raw.as_bytes()).unwrap()
    }

    #[test]
    fn a_plain_get_parses() {
        let request = parse("GET /api/state HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.path, "/api/state");
        assert_eq!(request.query, "");
        assert!(request.body.is_empty());
    }

    #[test]
    fn the_query_string_is_split_off_the_path() {
        // Routing matches on the path, so a query has to leave it. Without the
        // split, `/api/volume/O2?lod=1` would miss every route and 404 — which
        // looks exactly like a wrong field name.
        let request = parse("GET /api/volume/O2?lod=1&x=2 HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(request.path, "/api/volume/O2");
        assert_eq!(request.query, "lod=1&x=2");
    }

    #[test]
    fn percent_escapes_are_decoded() {
        let request = parse("GET /api/volume/N%5FMIN HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(request.path, "/api/volume/N_MIN");

        // A malformed escape is left alone rather than rejected: it fails to
        // match a route anyway, and the 404 is the clearer answer.
        let request = parse("GET /api/volume/%ZZ HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(request.path, "/api/volume/%ZZ");
        let request = parse("GET /api/volume/%A HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(request.path, "/api/volume/%A");
    }

    #[test]
    fn a_post_body_arrives_whole() {
        let request = parse(
            "POST /api/control HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 18\r\n\r\n{\"action\":\"pause\"}",
        )
        .unwrap();
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.path, "/api/control");
        assert_eq!(request.body, b"{\"action\":\"pause\"}");
    }

    #[test]
    fn the_content_length_header_is_matched_without_regard_to_case() {
        // Not pedantry: HTTP header names are case-insensitive, and a client
        // that sends `content-length` would otherwise have its body silently
        // dropped and its control command silently ignored.
        let request = parse("POST /api/control HTTP/1.1\r\ncontent-length: 4\r\n\r\nstep").unwrap();
        assert_eq!(request.body, b"step");
    }

    #[test]
    fn an_empty_connection_is_not_an_error() {
        assert_eq!(parse(""), None);
    }

    #[test]
    fn an_oversized_body_is_refused_rather_than_allocated() {
        // The declared length is what gets allocated, so an absurd one has to
        // be refused before the allocation and not after it.
        let err = read_request(
            format!(
                "POST /api/control HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
                u32::MAX
            )
            .as_bytes(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("bound"), "unhelpful: {err}");
    }

    #[test]
    fn an_endless_header_block_is_refused() {
        let mut raw = String::from("GET / HTTP/1.1\r\n");
        for i in 0..2000 {
            raw.push_str(&format!("X-Filler-{i}: 0123456789abcdef\r\n"));
        }
        raw.push_str("\r\n");
        let err = read_request(raw.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("too large"), "unhelpful: {err}");
    }

    #[test]
    fn an_unknown_method_is_reported_rather_than_assumed() {
        // Answered 405 by the router. Guessing GET would make a stray DELETE
        // look like a page load.
        let request = parse("DELETE /api/state HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(request.method, Method::Other);
    }

    #[test]
    fn a_response_says_its_length_and_closes() {
        let mut out = Vec::new();
        write_response(&mut out, &Response::json("{\"tick\":7}".into())).unwrap();
        let text = String::from_utf8(out).unwrap();

        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Type: application/json\r\n"));
        assert!(text.contains("Content-Length: 10\r\n"));
        assert!(text.contains("Connection: close\r\n"));
        // Without this the browser answers a paused simulation from cache and
        // the tick counter freezes while the world keeps running.
        assert!(text.contains("Cache-Control: no-store\r\n"));
        assert!(text.ends_with("\r\n\r\n{\"tick\":7}"));
    }

    #[test]
    fn a_binary_response_survives_bytes_that_are_not_text() {
        // Volume payloads are u8 per voxel and every byte value is legal,
        // including the ones that would end a line or a string.
        let body: Vec<u8> = (0..=255u8).collect();
        let mut out = Vec::new();
        write_response(&mut out, &Response::bytes(body.clone())).unwrap();
        assert!(out.ends_with(&body));

        // The declared length has to be the byte count, not the character
        // count: a volume is full of bytes that are not valid UTF-8, and a
        // length computed on text would truncate the frame at the first one.
        let head = String::from_utf8_lossy(&out[..out.len() - body.len()]).into_owned();
        assert!(head.contains("Content-Length: 256\r\n"), "head was {head}");
    }

    #[test]
    fn every_status_this_server_returns_has_a_reason() {
        for status in [200u16, 400, 404, 405, 413, 503] {
            assert_ne!(reason(status), "Unknown", "status {status} has no reason");
        }
    }
}
