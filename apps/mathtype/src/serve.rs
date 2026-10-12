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
//! POST /number?style=&side=&at= body=.docx -> JSON {"docx": base64, "count": n}
//! POST /crossref?to=N|latest body=.docx -> JSON {"docx": base64, "bookmark": name}
//! POST /compose?seed= (body ignored) -> JSON {"latex": "..."} from one editor session
//! POST /objectize?token=&latex=[&progid=][&number=right|left][&style=] [path=|body] -> the
//!     marker text becomes a fresh equation object (MathType's by default) with a rendered
//!     preview, optionally with its numbering field; JSON {"ok":true,"path":...} or {"docx":...}
//! POST /activate?index=N[&path=<docx>[&out=<docx>]] -> JSON {"docx": base64} (body mode)
//!     or {"ok":true,"path":...} (file mode), after the object's
//!     equation went through the desktop editor (`--edit`, or $MATHTYPE_RS_EDITOR) and came
//!     back with a refreshed preview — the add-in's "double-click to edit" on hosts without
//!     OLE. The request runs for as long as the editing session does.
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
        ("POST", "/number") => number(&mut stream, &query, &request.body),
        ("POST", "/crossref") => crossref(&mut stream, &query, &request.body),
        ("POST", "/activate") => activate(&mut stream, &query, &request.body),
        ("POST", "/compose") => compose(&mut stream, &query, &request.body),
        ("POST", "/objectize") => objectize(&mut stream, &query, &request.body),
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

/// POST /number?style=plain|chapter|section|paragraph&side=right|left[&at=N], body a .docx
/// container: equation numbering (#11), the same work `mathtype number` does.
///
/// The answer is JSON like /batch in replace mode: `{"count": n, "docx": "<base64>"}` — the
/// rewritten container, base64 so a browser host can write it out byte-for-byte.
fn number(stream: &mut TcpStream, query: &str, body: &[u8]) -> Result<u16, String> {
    fn bad(stream: &mut TcpStream, message: String) -> Result<u16, String> {
        respond(stream, 400, "text/plain", &message)
    }
    let style = match parameter(query, "style") {
        None => mt_numbering::NumberStyle::Plain,
        Some(value) => match mt_numbering::NumberStyle::parse(&value) {
            Some(style) => style,
            None => {
                return bad(
                    stream,
                    format!("unknown ?style= {value}: plain, chapter, section, or paragraph"),
                );
            }
        },
    };
    let side = match parameter(query, "side") {
        None => mt_numbering::Side::Right,
        Some(value) => match mt_numbering::Side::parse(&value) {
            Some(side) => side,
            None => return bad(stream, format!("unknown ?side= {value}: right or left")),
        },
    };
    let at = match parameter(query, "at") {
        Some(value) => match value.parse() {
            Ok(at) => Some(at),
            Err(_) => return bad(stream, "?at= must be a 1-based equation number".to_owned()),
        },
        None => None,
    };
    let opened = mt_docx::Document::open(body);
    let mut document = match opened {
        Ok(document) => document,
        Err(error) => return bad(stream, format!("cannot open the document: {error}")),
    };
    let outcome = mt_numbering::number_existing(&mut document, style, side, at)
        .and_then(|count| mt_numbering::write_settings(&mut document, style, side).map(|()| count));
    let count = match outcome {
        Ok(count) => count,
        Err(error) => return bad(stream, format!("numbering failed: {error}")),
    };
    let bytes = match document.into_bytes() {
        Ok(bytes) => bytes,
        Err(error) => return bad(stream, format!("writing the document failed: {error}")),
    };
    // The bridge's own log is where a host watches the work go by.
    println!("number {count} equations, {} style, {} side", style.as_str(), side.as_str());
    respond(
        stream,
        200,
        "application/json",
        &format!("{{\"count\":{count},\"docx\":\"{}\"}}", base64(&bytes)),
    )
}

/// POST /crossref?to=N|latest, body a .docx container: a cross-reference to a numbered
/// equation (#11), appended at the end of the body — the same work `mathtype crossref` does.
///
/// The answer is JSON: `{"bookmark": name, "docx": "<base64>"}`.
fn crossref(stream: &mut TcpStream, query: &str, body: &[u8]) -> Result<u16, String> {
    fn bad(stream: &mut TcpStream, message: String) -> Result<u16, String> {
        respond(stream, 400, "text/plain", &message)
    }
    let target = match parameter(query, "to") {
        None => mt_numbering::ReferenceTarget::Latest,
        Some(value) if value.eq_ignore_ascii_case("latest") => {
            mt_numbering::ReferenceTarget::Latest
        }
        Some(value) => match value.parse() {
            Ok(number) => mt_numbering::ReferenceTarget::Number(number),
            Err(_) => return bad(stream, "?to= must be an equation number or latest".to_owned()),
        },
    };
    let mut document = match mt_docx::Document::open(body) {
        Ok(document) => document,
        Err(error) => return bad(stream, format!("cannot open the document: {error}")),
    };
    let bookmark = match mt_numbering::insert_reference(
        &mut document,
        mt_docx::Position::BodyEnd,
        target,
    ) {
        Ok(bookmark) => bookmark,
        Err(error) => return bad(stream, format!("inserting the cross-reference failed: {error}")),
    };
    let bytes = match document.into_bytes() {
        Ok(bytes) => bytes,
        Err(error) => return bad(stream, format!("writing the document failed: {error}")),
    };
    println!("crossref to {bookmark}");
    respond(
        stream,
        200,
        "application/json",
        &format!("{{\"bookmark\":\"{bookmark}\",\"docx\":\"{}\"}}", base64(&bytes)),
    )
}

/// POST /activate?index=N&progid=Prefix body=.docx — the "double-click" edit round trip.
///
/// The `index`-th embedded equation object whose ProgID starts with `progid` (default
/// `MathTypeRs`) is handed to the desktop editor as LaTeX — the same hand-off the object server
/// uses — and this request blocks until the editor exits. Its LaTeX comes back, becomes a tree,
/// and is written into the object's stream with the stream's own preferences, with the preview
/// picture re-rendered from the edited equation. The whole document is returned as base64.
fn activate(stream: &mut TcpStream, query: &str, body: &[u8]) -> Result<u16, String> {
    fn bad(stream: &mut TcpStream, message: String) -> Result<u16, String> {
        respond(stream, 400, "text/plain", &message)
    }
    let index = match parameter(query, "index").map(|value| value.parse::<usize>()) {
        Some(Ok(n)) if n >= 1 => n,
        _ => return bad(stream, "?index= must give the 1-based number of the object".to_owned()),
    };
    // Which object kinds count as editable equations: ours, and MathType's own
    // (`Equation.DSMT4`), whose streams this bridge reads and writes natively.
    let filters: Vec<String> = parameter(query, "progid")
        .unwrap_or_else(|| "MathTypeRs,Equation.DSMT4".to_owned())
        .split(',')
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect();

    // The document comes from `?path=` (the add-in's way: no size limit, no big transfer)
    // or from the request body.
    let path_parameter = parameter(query, "path");
    let document_bytes: Vec<u8> = match &path_parameter {
        Some(path) => match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => return bad(stream, format!("cannot read {path}: {error}")),
        },
        None => body.to_vec(),
    };
    let mut document = match mt_docx::Document::open(&document_bytes) {
        Ok(document) => document,
        Err(error) => return bad(stream, format!("the document does not open: {error}")),
    };
    let target = document
        .equations()
        .into_iter()
        .filter(|equation| match &equation.kind {
            mt_docx::EquationKind::Ole { prog_id, .. } => prog_id
                .as_deref()
                .is_some_and(|id| filters.iter().any(|filter| id.starts_with(filter.as_str()))),
            _ => false,
        })
        .nth(index - 1);
    let Some(target) = target else {
        return bad(stream, format!("no equation object {index} with a {} ProgID", filters.join(" or ")));
    };
    let part = match &target.kind {
        mt_docx::EquationKind::Ole { part: Some(part), .. } => part.clone(),
        _ => return bad(stream, "the object has no data part".to_owned()),
    };
    let object_bytes = match document.part(&part) {
        Some(Ok(bytes)) => bytes,
        _ => return bad(stream, format!("the object part {part} cannot be read")),
    };
    // A real embedded object is a compound file around the equation stream; the bridge also
    // accepts the bare stream, and hands back whichever shape it was given.
    let (equation_bytes, compound) = match mt_mtef::ole::read_from_ole(&object_bytes) {
        Ok(Some(stream)) => (stream, true),
        _ => (object_bytes.clone(), false),
    };
    let parsed = match mt_mtef::read(&equation_bytes) {
        Ok(parsed) => parsed,
        Err(error) => return bad(stream, format!("the object's equation stream is unreadable: {error}")),
    };

    let edited = match edit_equation_session("activate", index, &mt_tex::to_latex(&parsed.math)) {
        Ok(edited) => edited,
        Err((status, message)) => return respond(stream, status, "text/plain", &message),
    };
    let math = match mt_tex::parse(&edited) {
        Ok(math) => math,
        Err(error) => {
            return bad(stream, format!("the edited equation does not parse at byte {}: {}", error.offset, error.message))
        }
    };

    let equation = mt_mtef::write_with_preferences(&math, &parsed.header.preferences);
    let rewritten = if compound {
        match mt_mtef::ole::write_into_ole(&object_bytes, &equation) {
            Ok(ole) => ole,
            Err(error) => {
                return respond(stream, 500, "text/plain", &format!("writing the object stream failed: {error}"));
            }
        }
    } else {
        equation
    };
    document.set_part(&part, rewritten);
    let preview = preview_part(&document, target.block, target.item);
    if let Some(preview) = preview {
        let style = mt_layout::LayoutStyle::display(18.0);
        let picture = if preview.ends_with(".emf") {
            crate::write_emf(&math, &style)
        } else {
            crate::write_math_with_style(&math, crate::Format::Png, &style, 3.0).unwrap_or_default()
        };
        if !picture.is_empty() {
            document.set_part(&preview, picture);
        }
    }
    let bytes = match document.into_bytes() {
        Ok(bytes) => bytes,
        Err(error) => return respond(stream, 500, "text/plain", &format!("writing the document failed: {error}")),
    };
    println!("activate object {index} -> {} bytes after editing", bytes.len());
    let out_parameter = parameter(query, "out");
    if let Some(path) = &path_parameter {
        // File mode: write the edited document where the caller wants it (in place by default)
        // and answer with a small receipt instead of the bytes.
        let destination = out_parameter.as_deref().unwrap_or(path.as_str());
        if let Err(error) = std::fs::write(destination, &bytes) {
            return respond(stream, 500, "text/plain", &format!("cannot write {destination}: {error}"));
        }
        return respond(
            stream,
            200,
            "application/json",
            &format!("{{\"ok\":true,\"path\":\"{}\"}}", json_escape(destination)),
        );
    }
    respond(
        stream,
        200,
        "application/json",
        &format!("{{\"docx\":\"{}\"}}", base64(&bytes)),
    )
}

/// POST /objectize — the add-in's insert: the marker text left by the add-in becomes a real
/// equation object, MathType's own format by default, with a freshly rendered preview picture.
/// `number=right|left` adds the numbering field on that side; `style=plain|chapter|section|paragraph`
/// picks the label.
fn objectize(stream: &mut TcpStream, query: &str, body: &[u8]) -> Result<u16, String> {
    fn bad(stream: &mut TcpStream, message: String) -> Result<u16, String> {
        respond(stream, 400, "text/plain", &message)
    }
    let Some(token) = parameter(query, "token") else {
        return bad(stream, "?token= must name the marker text to replace".to_owned());
    };
    let Some(latex) = parameter(query, "latex") else {
        return bad(stream, "?latex= must carry the equation".to_owned());
    };
    let prog_id = parameter(query, "progid").unwrap_or_else(|| "Equation.DSMT4".to_owned());
    let math = match mt_tex::parse(&latex) {
        Ok(math) => math,
        Err(error) => {
            return bad(stream, format!("the equation does not parse at byte {}: {}", error.offset, error.message))
        }
    };

    let path_parameter = parameter(query, "path");
    let document_bytes: Vec<u8> = match &path_parameter {
        Some(path) => match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => return bad(stream, format!("cannot read {path}: {error}")),
        },
        None => body.to_vec(),
    };
    let mut document = match mt_docx::Document::open(&document_bytes) {
        Ok(document) => document,
        Err(error) => return bad(stream, format!("the document does not open: {error}")),
    };

    // The marker paragraph item the add-in left behind.
    let mut found = None;
    for (block_index, block) in document.blocks().iter().enumerate() {
        let mt_docx::Block::Paragraph(paragraph) = block else { continue };
        for (item_index, item) in paragraph.items.iter().enumerate() {
            if let mt_docx::Item::Text { text, .. } = item {
                if text.contains(&token) {
                    found = Some((block_index, item_index));
                    break;
                }
            }
        }
        if found.is_some() {
            break;
        }
    }
    let Some((block, item)) = found else {
        return bad(stream, format!("the marker {token} is not in the document"));
    };

    // A freshly rendered preview, sized in points at the render scale (3 px per point).
    let style = mt_layout::LayoutStyle::display(18.0);
    let picture = match crate::write_math_with_style(&math, crate::Format::Png, &style, 3.0) {
        Ok(bytes) => bytes,
        Err(error) => return respond(stream, 500, "text/plain", &format!("rendering the preview failed: {error}")),
    };
    let (pixel_width, pixel_height) = match png_size(&picture) {
        Some(size) => size,
        None => return respond(stream, 500, "text/plain", "the preview is not a PNG"),
    };
    let to_emu = |pixels: u32| ((pixels as f64 / 3.0) * 12700.0).round() as u32;
    let preview = mt_docx::ImageData {
        bytes: picture,
        extension: "png".to_owned(),
        content_type: "image/png".to_owned(),
        name: "equation.png".to_owned(),
        extent_emu: (to_emu(pixel_width), to_emu(pixel_height)),
    };

    if let Err(error) = document.replace(
        block,
        item,
        mt_docx::Replacement::Ole {
            math: math.clone(),
            prog_id: Some(prog_id.clone()),
            preview: Some(preview),
        },
    ) {
        return respond(stream, 500, "text/plain", &format!("placing the object failed: {error}"));
    }
    println!("objectize -> {prog_id} at block {block}, item {item}");

    if let Some(side_name) = parameter(query, "number") {
        let Some(side) = mt_numbering::Side::parse(&side_name) else {
            return bad(stream, format!("unknown ?number= {side_name}: left or right"));
        };
        let number_style = parameter(query, "style")
            .and_then(|name| mt_numbering::NumberStyle::parse(&name))
            .unwrap_or(mt_numbering::NumberStyle::Plain);
        // The tab stop that puts the number at the margin.
        let properties = match &document.blocks()[block] {
            mt_docx::Block::Paragraph(paragraph) if paragraph.properties_open_end.is_some() => {
                mt_numbering::tabs_runs(side)
            }
            _ => mt_numbering::tabs_paragraph_properties(side),
        };
        if let Err(error) = document.insert_fragment(mt_docx::Position::ParagraphPropertiesStart { block }, &properties) {
            return respond(stream, 500, "text/plain", &format!("adding the tab stop failed: {error}"));
        }
        let tab_run = "<w:r xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:tab/></w:r>";
        let (position, runs) = match side {
            mt_numbering::Side::Right => (
                mt_docx::Position::AfterItem { block, item },
                format!("{tab_run}{}", mt_numbering::numbering_field_runs(number_style)),
            ),
            mt_numbering::Side::Left => (
                mt_docx::Position::BeforeItem { block, item },
                format!("{}{tab_run}", mt_numbering::numbering_field_runs(number_style)),
            ),
        };
        if let Err(error) = document.insert_fragment(position, &runs) {
            return respond(stream, 500, "text/plain", &format!("adding the number field failed: {error}"));
        }
    }

    let bytes = match document.into_bytes() {
        Ok(bytes) => bytes,
        Err(error) => return respond(stream, 500, "text/plain", &format!("writing the document failed: {error}")),
    };
    if let Some(_) = &path_parameter {
        let out = parameter(query, "out").or(path_parameter.clone()).expect("path present");
        if let Err(error) = std::fs::write(&out, &bytes) {
            return respond(stream, 500, "text/plain", &format!("cannot write {out}: {error}"));
        }
        return respond(stream, 200, "application/json", &format!("{{\"ok\":true,\"path\":\"{}\"}}", json_escape(&out)));
    }
    respond(stream, 200, "application/json", &format!("{{\"docx\":\"{}\"}}", base64(&bytes)))
}

/// Width and height of a PNG, from its IHDR chunk.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((width, height))
}

/// POST /compose?seed=<latex> — open the editor on a fresh equation and return its LaTeX.
///
/// This is what the add-in's insert commands (inline, display, numbered) are built on: the
/// request blocks for the whole editing session and answers `{"latex": "..."}`.
fn compose(stream: &mut TcpStream, query: &str, _body: &[u8]) -> Result<u16, String> {
    let seed = parameter(query, "seed").unwrap_or_default();
    let edited = match edit_equation_session("compose", 0, &seed) {
        Ok(edited) => edited,
        Err((status, message)) => return respond(stream, status, "text/plain", &message),
    };
    if edited.trim().is_empty() {
        return respond(stream, 400, "text/plain", "编辑器没有产出公式");
    }
    let latex = match mt_tex::parse(&edited) {
        Ok(math) => mt_tex::to_latex(&math),
        Err(error) => {
            return respond(
                stream,
                400,
                "text/plain",
                &format!("the composed equation does not parse at byte {}: {}", error.offset, error.message),
            )
        }
    };
    println!("compose -> {} bytes of LaTeX", latex.len());
    respond(stream, 200, "application/json", &format!("{{\"latex\":\"{}\"}}", json_escape(&latex)))
}

/// Escape the characters a JSON string must not carry raw.
fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// The part behind the object's preview picture, `word/media/image1.png` for instance.
fn preview_part(document: &mt_docx::Document, block: usize, item: usize) -> Option<String> {
    let paragraph = match &document.blocks()[block] {
        mt_docx::Block::Paragraph(paragraph) => paragraph,
        mt_docx::Block::Table(_) => return None,
    };
    let rel = match &paragraph.items[item] {
        mt_docx::Item::Object { preview_rel_id: Some(rel), .. } => rel,
        _ => return None,
    };
    document.relationship_part(rel)
}

/// Run one editing session on a staged equation file and return what the editor wrote back.
///
/// The file is seeded with `seed`, the editor runs to completion, and its result is read and
/// removed. Failures come back as the status and message to answer with.
fn edit_equation_session(kind: &str, serial: usize, seed: &str) -> Result<String, (u16, String)> {
    let equation_file = std::env::temp_dir().join(format!(
        "mathtype-{kind}-{}-{serial}.tex",
        std::process::id()
    ));
    if let Err(error) = std::fs::write(&equation_file, seed) {
        return Err((500, format!("cannot stage the equation: {error}")));
    }
    let editor = match std::env::var_os("MATHTYPE_RS_EDITOR") {
        Some(editor) => run_configured_editor(&editor, &equation_file),
        None => run_builtin_editor(&equation_file),
    };
    let finished = match editor.and_then(|mut child| child.wait().map_err(|error| error.to_string()))
    {
        Ok(status) => status,
        Err(error) => {
            let _ = std::fs::remove_file(&equation_file);
            return Err((500, format!("the editor could not be started: {error}")));
        }
    };
    let edited = match std::fs::read_to_string(&equation_file) {
        Ok(text) => text,
        Err(error) => {
            let _ = std::fs::remove_file(&equation_file);
            return Err((500, format!("the edited equation is unreadable: {error}")));
        }
    };
    let _ = std::fs::remove_file(&equation_file);
    if !finished.success() {
        return Err((500, format!("the editor exited with {finished}; nothing was changed")));
    }
    Ok(edited)
}

/// Start `$MATHTYPE_RS_EDITOR`, which takes the equation file as its only argument.
fn run_configured_editor(editor: &std::ffi::OsStr, equation: &std::path::PathBuf)
    -> Result<std::process::Child, String> {
    std::process::Command::new(editor)
        .arg(equation)
        .spawn()
        .map_err(|error| error.to_string())
}

/// Start this same binary in editor mode on the equation file.
fn run_builtin_editor(equation: &std::path::PathBuf) -> Result<std::process::Child, String> {
    let program = std::env::current_exe().unwrap_or_else(|_| "mathtype".into());
    std::process::Command::new(program)
        .arg("--edit")
        .arg(equation)
        .spawn()
        .map_err(|error| error.to_string())
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
    fn number_returns_a_numbered_docx() {
        let bytes = tiny_docx();
        let mut raw = format!(
            "POST /number?style=chapter&side=right HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
            bytes.len()
        )
        .into_bytes();
        raw.extend_from_slice(&bytes);
        let response = request(&raw);
        assert_eq!(status(&response), "HTTP/1.1 200 OK");
        let json = body(&response);
        assert!(json.contains("\"count\":1"), "{json}");
        let encoded = json.rsplit_once("\"docx\":\"").unwrap().1.split('"').next().unwrap();
        let document = mt_docx::Document::open(&base64_decode(encoded)).expect("the container opens");
        assert_eq!(mt_numbering::enumerate(&document).len(), 1);
        assert_eq!(
            mt_numbering::read_settings(&document),
            (mt_numbering::NumberStyle::Chapter, mt_numbering::Side::Right)
        );
    }

    #[test]
    fn number_rejects_an_unknown_style() {
        let response =
            request(b"POST /number?style=roman HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\n\r\n");
        assert_eq!(status(&response), "HTTP/1.1 400 Bad Request");
    }

    #[test]
    fn crossref_returns_a_referencing_docx() {
        // Number first, so the cross-reference has a target.
        let bytes = tiny_docx();
        let mut raw = format!(
            "POST /number HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
            bytes.len()
        )
        .into_bytes();
        raw.extend_from_slice(&bytes);
        let json = body(&request(&raw));
        let numbered = base64_decode(json.rsplit_once("\"docx\":\"").unwrap().1.split('"').next().unwrap());

        let mut raw = format!(
            "POST /crossref?to=1 HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
            numbered.len()
        )
        .into_bytes();
        raw.extend_from_slice(&numbered);
        let response = request(&raw);
        assert_eq!(status(&response), "HTTP/1.1 200 OK");
        let json = body(&response);
        assert!(json.contains("\"bookmark\":\"_RefMTEq1\""), "{json}");
        let encoded = json.rsplit_once("\"docx\":\"").unwrap().1.split('"').next().unwrap();
        let document = mt_docx::Document::open(&base64_decode(encoded)).expect("the container opens");
        assert!(document.document_xml().contains(" REF _RefMTEq1 \\h "));
    }

    #[test]
    fn crossref_without_a_target_is_refused() {
        let response =
            request(b"POST /crossref HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\n\r\n");
        assert_eq!(status(&response), "HTTP/1.1 400 Bad Request");
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

    /// A document whose one paragraph embeds a MathTypeRs equation object with a PNG preview.
    fn ole_docx() -> Vec<u8> {
        use std::io::Write;
        let math = mt_tex::parse("\\frac{a+b}{c}").expect("the object equation parses");
        let stream = mt_mtef::write(&math);
        let bin = mt_mtef::ole::create_ole(&stream).expect("the compound file is written");
        let document = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
             <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
             xmlns:v=\"urn:schemas-microsoft-com:vml\" \
             xmlns:o=\"urn:schemas-microsoft-com:office:office\" \
             xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body>\
             <w:p><w:r><w:object w:dxaOrig=\"3621\" w:dyaOrig=\"1686\">\
             <v:shape id=\"eqobj1\" style=\"width:271.6pt;height:126.5pt\">\
             <v:imagedata r:id=\"rIdImg1\" o:title=\"\"/></v:shape>\
             <o:OLEObject Type=\"Embed\" ProgID=\"MathTypeRs.Equation.1\" ShapeID=\"eqobj1\" \
             DrawAspect=\"Content\" ObjectID=\"_1700000001\" r:id=\"rIdObj1\"/>\
             </w:object></w:r></w:p></w:body></w:document>"
        );
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        let text_parts: [(&str, String); 4] = [
            (
                "[Content_Types].xml",
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
                 <Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
                 <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
                 <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
                 <Default Extension=\"bin\" ContentType=\"application/vnd.openxmlformats-officedocument.oleObject\"/>\
                 <Default Extension=\"png\" ContentType=\"image/png\"/>\
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
                 <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
                 <Relationship Id=\"rIdObj1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject\" Target=\"embeddings/oleObject1.bin\"/>\
                 <Relationship Id=\"rIdImg1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image1.png\"/></Relationships>"
                    .to_owned(),
            ),
        ];
        for (name, data) in text_parts {
            writer.start_file(name, options).expect("start part");
            writer.write_all(data.as_bytes()).expect("write part");
        }
        writer.start_file("word/embeddings/oleObject1.bin", options).expect("start bin");
        writer.write_all(&bin).expect("write bin");
        writer.start_file("word/media/image1.png", options).expect("start preview");
        writer.write_all(b"not a picture yet").expect("write preview");
        writer.finish().expect("close container").into_inner()
    }

    /// Tests that point `MATHTYPE_RS_EDITOR` at a script must not race each other.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A shell script standing in for the desktop editor: `fake_editor("a+b")` overwrites the
    /// equation file with that text; `fake_editor("")` makes the editor fail.
    fn fake_editor(replacement: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("mathtype-fake-editor-{}", std::process::id()));
        let script = if replacement.is_empty() {
            "#!/bin/sh\nexit 3\n".to_owned()
        } else {
            format!("#!/bin/sh\nprintf '%s' '{replacement}' > \"$1\"\n")
        };
        std::fs::write(&path, script).expect("write the fake editor");
        make_executable(&path);
        path
    }

    #[cfg(unix)]
    fn make_executable(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).expect("stat the editor").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod the editor");
    }

    #[cfg(not(unix))]
    fn make_executable(_path: &std::path::Path) {}

    /// An editor that leaves the equation file alone and exits successfully.
    fn fake_blank_editor() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("mathtype-blank-editor-{}", std::process::id()));
        std::fs::write(&path, "#!/bin/sh
exit 0
").expect("write the blank editor");
        make_executable(&path);
        path
    }

    #[cfg(unix)]
    #[test]
    fn activate_round_trips_and_reports_editor_failures() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        unsafe { std::env::set_var("MATHTYPE_RS_EDITOR", fake_editor("a+b")) };
        let bytes = ole_docx();
        let mut raw = format!(
            "POST /activate?index=1 HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
            bytes.len()
        )
        .into_bytes();
        raw.extend_from_slice(&bytes);
        let response = request(&raw);
        assert!(
            status(&response).contains("200"),
            "{} — {}",
            status(&response),
            body(&response)
        );
        let json = body(&response);
        let encoded = json.rsplit_once("\"docx\":\"").unwrap().1.split('"').next().unwrap();
        let decoded = base64_decode(encoded);
        let document = mt_docx::Document::open(&decoded).expect("the returned container opens");
        let equations = document.equations();
        assert_eq!(equations.len(), 1, "the object survived editing");
        assert_eq!(mt_tex::to_latex(equations[0].math.as_ref().expect("the object parses")), "a+b");
        // The stream is still inside its compound file, and the preview became a real picture.
        let part = match &equations[0].kind {
            mt_docx::EquationKind::Ole { part: Some(part), .. } => part.clone(),
            kind => panic!("expected an object, got {kind:?}"),
        };
        let bin = document.part(&part).expect("part present").expect("part reads");
        let stream = mt_mtef::ole::read_from_ole(&bin)
            .expect("the compound file reads")
            .expect("the object stayed a compound file");
        assert_eq!(mt_tex::to_latex(&mt_mtef::read(&stream).expect("stream reads").math), "a+b");
        let preview = document.part("word/media/image1.png").expect("preview present").expect("preview reads");
        assert_eq!(&preview[..4], b"\x89PNG", "the preview was re-rendered");

        // An editor that fails leaves the document alone and says so.
        unsafe { std::env::set_var("MATHTYPE_RS_EDITOR", fake_editor("")) };
        let response = request(&raw);
        assert!(
            status(&response).contains("500"),
            "{} — {}",
            status(&response),
            body(&response)
        );
        assert!(body(&response).contains("exited"), "{}", body(&response));
        unsafe { std::env::remove_var("MATHTYPE_RS_EDITOR") };
    }

    #[cfg(unix)]
    #[test]
    fn compose_returns_the_edited_latex() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        unsafe { std::env::set_var("MATHTYPE_RS_EDITOR", fake_editor("a+b")) };
        let response = request(b"POST /compose HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\n\r\n");
        assert!(
            status(&response).contains("200"),
            "{} — {}",
            status(&response),
            body(&response)
        );
        assert!(body(&response).contains("\"latex\":\"a+b\""), "{}", body(&response));

        // An editor session that produces nothing is a user-level error.
        unsafe { std::env::set_var("MATHTYPE_RS_EDITOR", fake_blank_editor()) };
        let response = request(b"POST /compose HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\n\r\n");
        assert!(status(&response).contains("400"), "{}", status(&response));

        // A broken editor is a server-side failure.
        unsafe { std::env::set_var("MATHTYPE_RS_EDITOR", fake_editor("")) };
        let response = request(b"POST /compose HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\n\r\n");
        assert!(status(&response).contains("500"), "{}", status(&response));
        unsafe { std::env::remove_var("MATHTYPE_RS_EDITOR") };
    }

    #[cfg(unix)]
    #[test]
    fn activate_path_mode_rewrites_the_file_and_accepts_mathetype_objects() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        unsafe { std::env::set_var("MATHTYPE_RS_EDITOR", fake_editor("u+v")) };
        // A MathType (`Equation.DSMT4`) object is editable too, not only our own ProgID.
        let bytes = ole_docx();
        let path = std::env::temp_dir().join(format!("mathtype-path-mode-{}.docx", std::process::id()));
        std::fs::write(&path, &bytes).expect("stage the document");
        let response = request(
            format!(
                "POST /activate?index=1&path={} HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\n\r\n",
                path.display()
            )
            .as_bytes(),
        );
        assert!(
            status(&response).contains("200"),
            "{} — {}",
            status(&response),
            body(&response)
        );
        assert!(body(&response).contains("\"ok\":true"), "{}", body(&response));
        let rewritten = std::fs::read(&path).expect("the file was rewritten");
        let document = mt_docx::Document::open(&rewritten).expect("rewritten container opens");
        let equations = document.equations();
        assert_eq!(
            mt_tex::to_latex(equations[0].math.as_ref().expect("parses")),
            "u+v"
        );
        let _ = std::fs::remove_file(&path);
        unsafe { std::env::remove_var("MATHTYPE_RS_EDITOR") };
    }

    /// A document whose one paragraph holds the add-in's marker text.
    fn marker_docx(token: &str) -> Vec<u8> {
        use std::io::Write;
        let document = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
             <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>\
             <w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr><w:r><w:t>{token}</w:t></w:r></w:p>\
             </w:body></w:document>"
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
    fn objectize_turns_the_marker_into_a_mathetype_object_with_a_preview() {
        let bytes = marker_docx("MTRSINSERT1");
        let mut raw = format!(
            "POST /objectize?token=MTRSINSERT1&latex=x%2B1 HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
            bytes.len()
        )
        .into_bytes();
        raw.extend_from_slice(&bytes);
        let response = request(&raw);
        assert!(
            status(&response).contains("200"),
            "{} — {}",
            status(&response),
            body(&response)
        );
        let json = body(&response);
        let encoded = json.rsplit_once("\"docx\":\"").unwrap().1.split('"').next().unwrap();
        let decoded = base64_decode(encoded);
        let document = mt_docx::Document::open(&decoded).expect("the returned container opens");
        let equations = document.equations();
        assert_eq!(equations.len(), 1, "the marker became one equation object");
        match &equations[0].kind {
            mt_docx::EquationKind::Ole { prog_id: Some(prog_id), part: Some(part) } => {
                assert_eq!(prog_id, "Equation.DSMT4", "MathType's own ProgID by default");
                let bin = document.part(part).expect("part present").expect("part reads");
                let stream = mt_mtef::ole::read_from_ole(&bin)
                    .expect("the compound file reads")
                    .expect("a compound file was created");
                assert_eq!(mt_tex::to_latex(&mt_mtef::read(&stream).expect("stream reads").math), "x+1");
            }
            kind => panic!("expected an equation object, got {kind:?}"),
        }
        let preview = document
            .part_names()
            .find(|name| name.starts_with("word/media/") && name.ends_with(".png"))
            .expect("a preview picture part exists");
        let picture = document.part(preview).expect("present").expect("reads");
        assert_eq!(&picture[..4], b"\x89PNG", "the preview is a rendered PNG");
        assert!(
            !document.document_xml().contains("MTRSINSERT1"),
            "the marker text is gone"
        );
    }

    #[test]
    fn activate_rejects_an_unknown_object() {
        let bytes = ole_docx();
        let mut raw = format!(
            "POST /activate?index=5 HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
            bytes.len()
        )
        .into_bytes();
        raw.extend_from_slice(&bytes);
        let response = request(&raw);
        assert!(
            status(&response).contains("400"),
            "{} — {}",
            status(&response),
            body(&response)
        );
    }
}
