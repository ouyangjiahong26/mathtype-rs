//! Loopback HTTP front end for the conversion bridge.
//!
//! Host add-ins run inside a browser-like context (WPS loads its add-ins as web pages) and cannot
//! launch processes, so the editor exposes the same conversions the command line does over HTTP:
//!
//! ```text
//! GET  /health                            -> "ok"
//! POST /convert?from=latex&to=omml   body -> converted text, or a message with status 400
//! POST /svg?latex=...&size=18             -> svg document
//! POST /export-html?script=mathjax body=.docx -> one self-contained HTML page
//! POST /batch?to=&mode=&dpi= body=.docx -> JSON report (+ base64 docx in replace mode)
//! ```
//!
//! Only the loopback interface is bound, and the server stops when its process does.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

/// Largest request body accepted, in bytes.
const MAX_BODY: usize = 4 * 1024 * 1024;

/// Longest request line or header line accepted, in bytes.
const MAX_LINE: usize = 8 * 1024;

/// Most header lines accepted, so a client cannot stream headers forever.
const MAX_HEADERS: usize = 100;

/// How long a single connection may stall on a read or a write before it is dropped.
const IO_TIMEOUT: Duration = Duration::from_secs(10);

/// Serve until the process is stopped.
pub fn serve(port: u16) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|error| format!("cannot listen on 127.0.0.1:{port}: {error}"))?;
    println!("mathtype conversion API listening on http://127.0.0.1:{port}");
    serve_listener(listener)
}

/// Accept connections until the listener fails or the process stops.
///
/// Every connection is handled on its own thread, so a client that connects and then stalls
/// cannot delay the requests behind it.
fn serve_listener(listener: TcpListener) -> Result<(), String> {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                std::thread::spawn(move || {
                    if let Err(error) = handle(stream) {
                        eprintln!("request failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("accept failed: {error}"),
        }
    }
    Ok(())
}

struct Request {
    method: String,
    target: String,
    body: Vec<u8>,
}

fn handle(mut stream: TcpStream) -> Result<(), String> {
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|error| error.to_string())?);
    let mut request_line = String::new();
    if read_line_limited(&mut reader, &mut request_line, MAX_LINE).is_err() {
        respond(
            &mut stream,
            400,
            "text/plain",
            "malformed or oversized request line",
        )?;
        return Ok(());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();

    let mut content_length = 0usize;
    let mut headers = 0usize;
    loop {
        let mut header = String::new();
        if read_line_limited(&mut reader, &mut header, MAX_LINE).is_err() {
            respond(
                &mut stream,
                400,
                "text/plain",
                "malformed or oversized header",
            )?;
            return Ok(());
        }
        let trimmed = header.trim_end();
        if trimmed.is_empty() {
            break;
        }
        headers += 1;
        if headers > MAX_HEADERS {
            respond(&mut stream, 400, "text/plain", "too many headers")?;
            return Ok(());
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
        reader
            .read_exact(&mut body)
            .map_err(|error| error.to_string())?;
    }
    let request = Request {
        method,
        target,
        body,
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
        ("POST", "/export-html") => export_html(&mut stream, &query, &request.body),
        ("POST", "/batch") => batch(&mut stream, &query, &request.body),
        ("GET", _) | ("POST", _) => respond(&mut stream, 404, "text/plain", "unknown path"),
        _ => respond(&mut stream, 405, "text/plain", "method not allowed"),
    }?;
    // Host add-ins are verified by watching this log while the host application loads them.
    println!("{} {} -> {}", request.method, request.target, status);
    Ok(())
}

/// Read one line into `line`, refusing to buffer more than `limit` bytes.
///
/// `read_line` on its own would grow without bound, so a client could stream an endless request
/// line. The caller turns the error into a 400.
fn read_line_limited(
    reader: &mut impl BufRead,
    line: &mut String,
    limit: usize,
) -> std::io::Result<()> {
    line.clear();
    let read = reader.by_ref().take(limit as u64 + 1).read_line(line)?;
    if read > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "line too long",
        ));
    }
    Ok(())
}

fn convert(stream: &mut TcpStream, query: &str, body: &[u8]) -> Result<u16, String> {
    let from = parameter(query, "from").and_then(|name| crate::Format::parse(&name));
    let to = parameter(query, "to").and_then(|name| crate::Format::parse(&name));
    let Some(to) = to else {
        return respond(stream, 400, "text/plain", "missing or unknown ?to= format");
    };
    let from = from.unwrap_or_else(|| crate::detect_format(&String::from_utf8_lossy(body)));
    let math = match crate::read_math(body, from) {
        Ok(math) => math,
        Err(message) => return respond(stream, 400, "text/plain", &message),
    };
    let size = match size_parameter(query) {
        Ok(size) => size,
        Err(message) => return respond(stream, 400, "text/plain", &message),
    };
    let scale = match scale_parameter(query) {
        Ok(scale) => scale,
        Err(message) => return respond(stream, 400, "text/plain", &message),
    };
    match crate::write_math(&math, to, size, scale) {
        Ok(bytes) if to.is_binary() => {
            respond_bytes(stream, 200, "application/octet-stream", &bytes)
        }
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            respond(stream, 200, "text/plain", &text)
        }
        Err(message) => respond(stream, 400, "text/plain", &message),
    }
}

fn svg(stream: &mut TcpStream, query: &str, body: &[u8]) -> Result<u16, String> {
    let latex = match parameter(query, "latex") {
        Some(latex) => latex.into_bytes(),
        None if !body.is_empty() => body.to_owned(),
        None => return respond(stream, 400, "text/plain", "missing ?latex= or a body"),
    };
    let math = match crate::read_math(&latex, crate::Format::Latex) {
        Ok(math) => math,
        Err(message) => return respond(stream, 400, "text/plain", &message),
    };
    let size = match size_parameter(query) {
        Ok(size) => size,
        Err(message) => return respond(stream, 400, "text/plain", &message),
    };
    match crate::write_math(&math, crate::Format::Svg, size, 1.0) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            respond(stream, 200, "image/svg+xml", &text)
        }
        Err(message) => respond(stream, 400, "text/plain", &message),
    }
}

/// POST /export-html?script=mathjax|katex|none, body a .docx container: a single self-contained
/// HTML page, every picture inlined as a data URI — the only shape a host that can write one
/// file (the WPS add-in) can save directly.
fn export_html(stream: &mut TcpStream, query: &str, body: &[u8]) -> Result<u16, String> {
    let mut options = mt_html::Options {
        embed: true,
        ..Default::default()
    };
    if let Some(value) = parameter(query, "script") {
        options.script = mt_html::Script::parse(&value)
            .ok_or_else(|| format!("unknown ?script= {value}: none, mathjax, or katex"))?;
    }
    if let Some(value) = parameter(query, "math") {
        options.math = mt_html::MathMode::parse(&value)
            .ok_or_else(|| format!("unknown ?math= {value}: mathml or image"))?;
    }
    let result =
        mt_docx::Document::open(body).and_then(|document| Ok(mt_html::export(&document, &options)));
    match result {
        Ok(output) => respond(stream, 200, "text/html; charset=utf-8", &output.html),
        Err(error) => respond(
            stream,
            400,
            "text/plain",
            &format!("cannot export: {error}"),
        ),
    }
}

/// POST /batch?to=latex|mathml|omath|png&mode=report|replace|export&dpi=N&size=pt[&out=dir],
/// body a .docx container: the document-level batch conversion (#13), same options as
/// `mathtype batch-convert`.
///
/// The answer is JSON: `{"report":{...},"docx":"<base64>"}` in replace mode — the rewritten
/// container, base64 so a browser host can write it out byte-for-byte — and `{"report":{...}}`
/// otherwise, the report's `entries` listing every converted formula (report mode) or the
/// product file names written under `out` (export mode, default `$TMPDIR/mathtype-batch`).
fn batch(stream: &mut TcpStream, query: &str, body: &[u8]) -> Result<u16, String> {
    // Parse errors are answered inline: propagating them out of the handler would drop the
    // connection instead of returning a status line.
    fn bad(stream: &mut TcpStream, message: String) -> Result<u16, String> {
        respond(stream, 400, "text/plain", &message)
    }
    let format = match parameter(query, "to").as_deref() {
        Some(name) => match mt_batch::TargetFormat::parse(name) {
            Some(format) => format,
            None => {
                return bad(
                    stream,
                    format!("unknown ?to= {name}: latex, mathml, omath, or png"),
                );
            }
        },
        None => mt_batch::TargetFormat::Latex,
    };
    let mode = match parameter(query, "mode").as_deref() {
        Some(name) => match mt_batch::Mode::parse(name) {
            Some(mode) => mode,
            None => {
                return bad(
                    stream,
                    format!("unknown ?mode= {name}: report, replace, or export"),
                );
            }
        },
        None => mt_batch::Mode::Report,
    };
    let dpi = match parameter(query, "dpi") {
        Some(value) => match value.parse() {
            Ok(dpi) => dpi,
            Err(_) => return bad(stream, "?dpi= must be a number".to_owned()),
        },
        None => mt_batch::DEFAULT_DPI,
    };
    let size = match parameter(query, "size") {
        Some(value) => match value.parse() {
            Ok(size) => size,
            Err(_) => return bad(stream, "?size= must be a number of points".to_owned()),
        },
        None => mt_batch::DEFAULT_SIZE,
    };
    let export_dir = parameter(query, "out")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("mathtype-batch"));
    if mode != mt_batch::Mode::Replace {
        if let Err(message) = mt_batch::ensure_export_dir(&export_dir) {
            return bad(stream, message);
        }
    }
    let options = mt_batch::Options {
        format,
        mode,
        dpi,
        size,
        export_dir,
        scope: None,
    };
    let outcome = mt_batch::run(body, &options, &mut |progress| {
        // The bridge's own log is where a host watches the run go by.
        println!(
            "batch {}/{} {}",
            progress.index, progress.total, progress.location
        );
        true
    });
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(message) => return bad(stream, message),
    };
    let docx = outcome
        .docx
        .map(|bytes| format!(",\"docx\":\"{}\"", base64(&bytes)))
        .unwrap_or_default();
    let json = format!("{{\"report\":{}}}", outcome.report.to_json());
    // The rewritten container was base64'd above; splice it in before the closing brace.
    let json = if docx.is_empty() {
        json
    } else {
        format!("{}{}}}", &json[..json.len() - 1], docx)
    };
    respond(stream, 200, "application/json", &json)
}

/// Standard base64 of the bytes, for the docx the bridge hands back.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let padded = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
            0,
        ];
        let triple = u32::from_be_bytes(padded) >> 8;
        out.push(ALPHABET[(triple >> 18) as usize & 63] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// `?size=`, defaulting to 18 points and refusing values outside the accepted range.
fn size_parameter(query: &str) -> Result<f64, String> {
    match parameter(query, "size") {
        None => Ok(18.0),
        Some(value) => {
            let size = value
                .parse()
                .map_err(|_| "?size= must be a number of points".to_owned())?;
            crate::validate_size(size).map_err(|message| format!("?size= {message}"))
        }
    }
}

/// `?scale=`, defaulting to 3 device pixels per point and refusing values outside the range.
fn scale_parameter(query: &str) -> Result<f64, String> {
    match parameter(query, "scale") {
        None => Ok(3.0),
        Some(value) => {
            let scale = value
                .parse()
                .map_err(|_| "?scale= must be a number".to_owned())?;
            crate::validate_scale(scale).map_err(|message| format!("?scale= {message}"))
        }
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

fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> Result<u16, String> {
    respond_bytes(stream, status, content_type, body.as_bytes())
}

fn respond_bytes(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<u16, String> {
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
    stream
        .write_all(head.as_bytes())
        .map_err(|error| error.to_string())?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::Shutdown;
    use std::sync::LazyLock;

    /// One background server for every test, on an ephemeral loopback port.
    static TEST_PORT: LazyLock<u16> = LazyLock::new(|| {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind test server");
        let port = listener.local_addr().expect("test address").port();
        std::thread::spawn(move || {
            let _ = serve_listener(listener);
        });
        port
    });

    fn test_port() -> u16 {
        *TEST_PORT
    }

    /// Send a raw request and return the whole response.
    fn request(raw: &[u8]) -> Vec<u8> {
        let mut stream =
            TcpStream::connect(("127.0.0.1", test_port())).expect("connect test server");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(raw).expect("write request");
        stream.shutdown(Shutdown::Write).expect("half close");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read response");
        response
    }

    fn status(response: &[u8]) -> String {
        String::from_utf8_lossy(response)
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned()
    }

    fn body(response: &[u8]) -> String {
        String::from_utf8_lossy(response)
            .split_once("\r\n\r\n")
            .map(|(_, body)| body.to_owned())
            .unwrap_or_default()
    }

    #[test]
    fn percent_decode_handles_escapes_and_plus() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("1%2B1"), "1+1");
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("100%25"), "100%");
        // A stray percent cannot open an escape and is left as-is.
        assert_eq!(percent_decode("50%"), "50%");
    }

    #[test]
    fn parameter_picks_the_named_value() {
        assert_eq!(
            parameter("from=latex&to=omml", "to").as_deref(),
            Some("omml")
        );
        assert_eq!(
            parameter("latex=%5Cfrac", "latex").as_deref(),
            Some("\\frac")
        );
        assert_eq!(parameter("from=latex", "to"), None);
    }

    #[test]
    fn oversized_request_body_is_rejected() {
        let response = request(
            b"POST /convert?to=latex HTTP/1.1\r\nHost: test\r\nContent-Length: 9999999999\r\n\r\n",
        );
        assert!(status(&response).contains("413"), "{}", status(&response));
    }

    #[test]
    fn unknown_path_is_rejected() {
        let response = request(b"POST /nope HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\n\r\n");
        assert!(status(&response).contains("404"), "{}", status(&response));
    }

    #[test]
    fn out_of_range_size_is_rejected() {
        // The same expression at the default size succeeds, so the 400 is about `size` alone.
        let ok = request(b"POST /svg?latex=x HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\n\r\n");
        assert!(status(&ok).contains("200"), "{}", status(&ok));
        let response = request(
            b"POST /svg?latex=x&size=9999999 HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\n\r\n",
        );
        assert!(status(&response).contains("400"), "{}", status(&response));
    }

    #[test]
    fn out_of_range_scale_is_rejected() {
        let response = request(
            b"POST /convert?from=latex&to=png&scale=1000000 HTTP/1.1\r\nHost: test\r\nContent-Length: 1\r\n\r\nx",
        );
        assert!(status(&response).contains("400"), "{}", status(&response));
    }

    /// A one-paragraph .docx holding one Office Math formula, zipped like a real container.
    fn tiny_docx() -> Vec<u8> {
        use std::io::Write;
        let math = mt_tex::parse("x^2").expect("the test equation parses");
        let document = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
             <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
             xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"><w:body>\
             <w:p><w:r><w:t>a</w:t></w:r>{omath}</w:p></w:body></w:document>",
            omath = mt_omml::write(&math)
        );
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let parts: [(&str, String); 4] = [
            (
                "[Content_Types].xml",
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
                 <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
                 <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
                 <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
                 <Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>"
                    .to_owned(),
            ),
            (
                "_rels/.rels",
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
                 <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
                 <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>"
                    .to_owned(),
            ),
            ("word/document.xml", document),
            (
                "word/_rels/document.xml.rels",
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
                 <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>"
                    .to_owned(),
            ),
        ];
        for (name, data) in parts {
            writer.start_file(name, options).expect("start part");
            writer.write_all(data.as_bytes()).expect("write part");
        }
        writer.finish().expect("close container").into_inner()
    }

    #[test]
    fn export_html_returns_a_self_contained_page() {
        let bytes = tiny_docx();
        let mut raw = format!(
            "POST /export-html HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
            bytes.len()
        )
        .into_bytes();
        raw.extend_from_slice(&bytes);
        let response = request(&raw);
        assert_eq!(status(&response), "HTTP/1.1 200 OK");
        let page = body(&response);
        assert!(page.contains("<!DOCTYPE html>"), "{page}");
        assert!(page.contains("<math"), "{page}");
        assert!(page.contains("<p>a"), "{page}");
    }

    #[test]
    fn batch_report_returns_the_json_report() {
        let bytes = tiny_docx();
        let mut raw = format!(
            "POST /batch?to=latex&mode=report HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
            bytes.len()
        )
        .into_bytes();
        raw.extend_from_slice(&bytes);
        let response = request(&raw);
        assert_eq!(status(&response), "HTTP/1.1 200 OK");
        let json = body(&response);
        assert!(json.contains("\"format\":\"latex\""), "{json}");
        assert!(json.contains("\"succeeded\":1"), "{json}");
        assert!(json.contains("\"output\":\"x^2\""), "{json}");
        assert!(
            !json.contains("docx"),
            "report mode rewrites nothing: {json}"
        );
    }

    #[test]
    fn batch_replace_returns_the_report_and_the_converted_docx() {
        let bytes = tiny_docx();
        let mut raw = format!(
            "POST /batch?to=latex&mode=replace HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
            bytes.len()
        )
        .into_bytes();
        raw.extend_from_slice(&bytes);
        let response = request(&raw);
        assert_eq!(status(&response), "HTTP/1.1 200 OK");
        let json = body(&response);
        assert!(json.contains("\"docx\":\""), "{json}");
        // The base64 container decodes back into a document whose formula is gone.
        let encoded = json
            .rsplit_once("\"docx\":\"")
            .unwrap()
            .1
            .trim_end_matches('\"');
        let decoded = base64_decode(encoded);
        let document = mt_docx::Document::open(&decoded).expect("the returned container opens");
        assert!(
            document.equations().is_empty(),
            "the one formula became text"
        );
    }

    #[test]
    fn batch_rejects_an_unknown_format() {
        let response =
            request(b"POST /batch?to=nope HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\n\r\n");
        assert_eq!(status(&response), "HTTP/1.1 400 Bad Request");
    }

    /// Standard base64 decode, the test side of [`base64`].
    fn base64_decode(text: &str) -> Vec<u8> {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        let mut buffer = 0u32;
        let mut bits = 0u32;
        for character in text.chars() {
            if character == '=' {
                break;
            }
            let value = ALPHABET
                .iter()
                .position(|byte| *byte as char == character)
                .expect("base64") as u32;
            buffer = (buffer << 6) | value;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buffer >> bits) as u8);
            }
        }
        out
    }

    #[test]
    fn export_html_rejects_a_broken_container() {
        let response =
            request(b"POST /export-html HTTP/1.1\r\nHost: test\r\nContent-Length: 3\r\n\r\nabc");
        assert_eq!(status(&response), "HTTP/1.1 400 Bad Request");
    }

    #[test]
    fn binary_request_body_is_preserved() {
        let math = mt_tex::parse("\\frac{a}{b}").expect("parse latex");
        let mtef = mt_mtef::write(&math);
        assert!(
            std::str::from_utf8(&mtef).is_err(),
            "the equation stream must be binary"
        );

        let mut raw = format!(
            "POST /convert?from=mtef&to=latex HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
            mtef.len()
        )
        .into_bytes();
        raw.extend_from_slice(&mtef);
        let response = request(&raw);

        assert!(
            status(&response).contains("200"),
            "{} — {}",
            status(&response),
            body(&response)
        );
        assert_eq!(body(&response), mt_tex::to_latex(&math));
    }

    #[test]
    fn stalled_connection_does_not_block_others() {
        // A client that connects and then says nothing: a serial server would sit on it until
        // the read timeout, delaying every request behind it.
        let _stalled =
            TcpStream::connect(("127.0.0.1", test_port())).expect("connect stalled client");
        let start = std::time::Instant::now();
        let response = request(b"GET /health HTTP/1.1\r\nHost: test\r\n\r\n");
        assert!(status(&response).contains("200"), "{}", status(&response));
        assert_eq!(body(&response), "ok");
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "health request waited behind the stalled client"
        );
    }
}
