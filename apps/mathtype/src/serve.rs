//! Loopback HTTP front end for the conversion bridge.
//!
//! Host add-ins run inside a browser-like context (WPS loads its add-ins as web pages) and cannot
//! launch processes, so the editor exposes the same conversions the command line does over HTTP:
//!
//! ```text
//! GET  /health                            -> "ok"
//! POST /convert?from=latex&to=omml   body -> converted text, or a message with status 400
//! POST /svg?latex=...&size=18             -> svg document
//! ```
//!
//! Only the loopback interface is bound, and the server stops when its process does.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

/// Largest request body accepted, in bytes.
const MAX_BODY: usize = 4 * 1024 * 1024;

/// Serve until the process is stopped.
pub fn serve(port: u16) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|error| format!("cannot listen on 127.0.0.1:{port}: {error}"))?;
    println!("mathtype conversion API listening on http://127.0.0.1:{port}");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle(stream) {
                    eprintln!("request failed: {error}");
                }
            }
            Err(error) => eprintln!("accept failed: {error}"),
        }
    }
    Ok(())
}

struct Request {
    method: String,
    target: String,
    body: String,
}

fn handle(mut stream: TcpStream) -> Result<(), String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|error| error.to_string())?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).map_err(|error| error.to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();

    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).map_err(|error| error.to_string())?;
        let trimmed = header.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    if content_length > MAX_BODY {
        respond(&mut stream, 413, "text/plain", "request body too large")?;
        return Ok(());
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body).map_err(|error| error.to_string())?;
    }
    let request = Request {
        method,
        target,
        body: String::from_utf8_lossy(&body).into_owned(),
    };

    if request.method == "OPTIONS" {
        respond_with(&mut stream, 204, "text/plain", "", true)?;
        println!("OPTIONS {} -> 204", request.target);
        return Ok(());
    }

    let (path, query) = match request.target.split_once('?') {
        Some((path, query)) => (path.to_owned(), query.to_owned()),
        None => (request.target.clone(), String::new()),
    };

    let status = match (request.method.as_str(), path.as_str()) {
        ("GET", "/health") => respond(&mut stream, 200, "text/plain", "ok"),
        ("POST", "/convert") => convert(&mut stream, &query, &request.body),
        ("POST", "/svg") => svg(&mut stream, &query, &request.body),
        ("GET", _) | ("POST", _) => respond(&mut stream, 404, "text/plain", "unknown path"),
        _ => respond(&mut stream, 405, "text/plain", "method not allowed"),
    }?;
    // Host add-ins are verified by watching this log while the host application loads them.
    println!("{} {} -> {}", request.method, request.target, status);
    Ok(())
}

fn convert(stream: &mut TcpStream, query: &str, body: &str) -> Result<u16, String> {
    let from = parameter(query, "from").and_then(|name| crate::Format::parse(&name));
    let to = parameter(query, "to").and_then(|name| crate::Format::parse(&name));
    let Some(to) = to else {
        return respond(stream, 400, "text/plain", "missing or unknown ?to= format");
    };
    let from = from.unwrap_or_else(|| crate::detect_format(body));
    let math = match crate::read_math(body.as_bytes(), from) {
        Ok(math) => math,
        Err(message) => return respond(stream, 400, "text/plain", &message),
    };
    let size = parameter(query, "size").and_then(|value| value.parse().ok()).unwrap_or(18.0);
    let scale = parameter(query, "scale").and_then(|value| value.parse().ok()).unwrap_or(3.0);
    match crate::write_math(&math, to, size, scale) {
        Ok(bytes) if to.is_binary() => respond_bytes(stream, 200, "application/octet-stream", &bytes),
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            respond(stream, 200, "text/plain", &text)
        }
        Err(message) => respond(stream, 400, "text/plain", &message),
    }
}

fn svg(stream: &mut TcpStream, query: &str, body: &str) -> Result<u16, String> {
    let latex = match parameter(query, "latex") {
        Some(latex) => latex,
        None if !body.is_empty() => body.to_owned(),
        None => return respond(stream, 400, "text/plain", "missing ?latex= or a body"),
    };
    let math = match crate::read_math(latex.as_bytes(), crate::Format::Latex) {
        Ok(math) => math,
        Err(message) => return respond(stream, 400, "text/plain", &message),
    };
    let size = parameter(query, "size").and_then(|value| value.parse().ok()).unwrap_or(18.0);
    match crate::write_math(&math, crate::Format::Svg, size, 1.0) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            respond(stream, 200, "image/svg+xml", &text)
        }
        Err(message) => respond(stream, 400, "text/plain", &message),
    }
}

/// Value of `name` in a `a=1&b=2` query string, percent-decoded.
fn parameter(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then(|| percent_decode(value))
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) -> Result<u16, String> {
    respond_bytes(stream, status, content_type, body.as_bytes())
}

fn respond_bytes(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) -> Result<u16, String> {
    respond_with(stream, status, content_type, body, false)?;
    Ok(status)
}

fn respond_with(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: impl AsRef<[u8]>,
    empty: bool,
) -> Result<(), String> {
    let body = if empty { &[][..] } else { body.as_ref() };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {length}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Headers: content-type\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Connection: close\r\n\
         \r\n",
        reason = reason(status),
        length = body.len(),
    );
    stream.write_all(head.as_bytes()).map_err(|error| error.to_string())?;
    stream.write_all(body).map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        _ => "Error",
    }
}
