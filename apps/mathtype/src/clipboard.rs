//! 剪贴板多格式互通（#12）。
//!
//! 复制一条公式时同时提供四种格式：自有 MTEF 字节、HTML（MathML 与 OMML 并存，浏览器
//! 与 Word 各取所需）、LaTeX 纯文本、PNG 位图回退。粘贴时按优先级降级取用：
//! MTEF、HTML、纯文本，位图不进入公式树。
//!
//! X11 的剪贴板需要一个存活的属主进程来服务多个 target，`clipboard_owner.py` 就是这个
//! 属主（公式负载只有几 KB，不需要 INCR）。Wayland 应用经合成器桥接读取 X 属主的内容。

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// 一条公式对应的四种剪贴板负载。
pub struct Payload {
    pub latex: String,
    pub html: String,
    pub mtef: Vec<u8>,
    pub png: Vec<u8>,
}

/// 构造一条公式的全部剪贴板负载。
pub fn payload(math: &mt_core::Math) -> Result<Payload, String> {
    let latex = mt_tex::to_latex(math);
    let mathml = mt_mathml::write(math);
    let omml = mt_omml::write(math);
    let html = html_fragment(&mathml, &omml);
    let mtef = mt_mtef::write(math);
    let png = super::write_math_with_style(
        math,
        super::Format::Png,
        &mt_layout::LayoutStyle::display(18.0),
        3.0,
    )?;
    Ok(Payload {
        latex,
        html,
        mtef,
        png,
    })
}

/// 把 MathML 与 OMML 装进一份 HTML：浏览器渲染 MathML、忽略未知标签，Word 认
/// OMML、忽略 MathML，两边都能取到可用的形态。写出器给的 MathML 带 `m:` 前缀
/// （XML 合法），但浏览器按 HTML 解析剪贴板内容，只认无前缀的 `math` 标签，这里
/// 转成默认命名空间形态。
fn html_fragment(mathml: &str, omml: &str) -> String {
    const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";
    let mathml = mathml.replace(
        &format!("xmlns:m=\"{MATHML_NS}\""),
        &format!("xmlns=\"{MATHML_NS}\""),
    );
    let mathml = mathml.replace("<m:", "<").replace("</m:", "</");
    format!("<html><body><!--StartFragment-->{mathml}{omml}<!--EndFragment--></body></html>")
}

/// 取剪贴板属主脚本（随二进制部署）并写到临时目录，返回其路径。
fn stage_owner_script() -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!(
        "mathtype-clipboard-owner-{}.py",
        std::process::id()
    ));
    std::fs::write(&path, include_str!("clipboard_owner.py"))
        .map_err(|error| format!("写入剪贴板属主脚本失败：{error}"))?;
    Ok(path)
}

/// 把一条公式以全部格式写入系统剪贴板。属主进程接管剪贴板直到有程序替换它。
pub fn copy(math: &mt_core::Math) -> Result<(), String> {
    let payload = payload(math)?;
    let script = stage_owner_script()?;
    let mut files = Vec::new();
    for (name, bytes) in [
        ("application/x-mathtype-mtef", payload.mtef.as_slice()),
        ("text/html", payload.html.as_bytes()),
        ("text/plain", payload.latex.as_bytes()),
        ("image/png", payload.png.as_slice()),
    ] {
        let safe_name = name.replace('/', "_");
        let path = std::env::temp_dir().join(format!(
            "mathtype-clip-{safe_name}-{}.bin",
            std::process::id()
        ));
        std::fs::write(&path, bytes).map_err(|error| format!("写入负载失败：{error}"))?;
        files.push((name.to_owned(), path));
    }
    let mut command = Command::new("python3");
    command.arg(&script);
    for (name, path) in &files {
        command.arg("--target").arg(name).arg("--file").arg(path);
    }
    if let Some(display) = std::env::var_os("DISPLAY") {
        command.env("DISPLAY", display);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("启动剪贴板属主失败：{error}"))?;
    Ok(())
}

/// 读取系统剪贴板并解析成公式树，按 MTEF、HTML、纯文本的顺序降级。
/// `image/png` 等位图格式无法还原成树，返回带说明的错误。
pub fn paste() -> Result<mt_core::Math, String> {
    let targets = clipboard_targets()?;
    let prefer = ["application/x-mathtype-mtef", "text/html", "text/plain"];
    for target in prefer {
        if targets.iter().any(|offered| offered == target) {
            let data = clipboard_read(target)?;
            if let Some(math) = parse_clipboard(target, &data) {
                return Ok(math);
            }
        }
    }
    if targets.iter().any(|offered| offered.starts_with("image/")) {
        return Err("剪贴板里是图片，无法还原成可编辑公式".to_owned());
    }
    Err("剪贴板里没有可识别的公式格式".to_owned())
}

/// 把一种 target 的字节解析成公式树，解析不了返回 None。
fn parse_clipboard(target: &str, data: &[u8]) -> Option<mt_core::Math> {
    match target {
        "application/x-mathtype-mtef" => mt_mtef::read(data).ok().map(|parsed| parsed.math),
        "text/html" | "text/plain" => {
            let text = String::from_utf8_lossy(data);
            let text = strip_html(&text);
            if text.trim().is_empty() {
                return None;
            }
            let format = super::detect_format(&text);
            super::read_math(text.as_bytes(), format).ok()
        }
        _ => None,
    }
}

/// 取出 HTML 中的 MathML 部分（detect 只认纯文本形态，HTML 里先剥离标签外的说明）。
fn strip_html(html: &str) -> String {
    if !html.contains('<') {
        return html.to_owned();
    }
    // MathML 文档形态是可解析输入；HTML 包装里直接找 <math 起始的片段。
    if let Some(at) = html.find("<math") {
        let end = html[at..]
            .find("</math>")
            .map(|index| at + index + "</math>".len())
            .unwrap_or(html.len());
        return html[at..end].to_owned();
    }
    // 没有 MathML：取纯文本内容交给 detect_format（LaTeX 等）。
    let mut text = String::new();
    let mut inside = false;
    for ch in html.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => text.push(ch),
            _ => {}
        }
    }
    text
}

/// 列出剪贴板当前提供的 target。
fn clipboard_targets() -> Result<Vec<String>, String> {
    let output = Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "TARGETS", "-o"])
        .output()
        .map_err(|error| format!("读取剪贴板清单失败：{error}"))?;
    if !output.status.success() {
        return Err("剪贴板不可用（xclip 读取失败）".to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "TARGETS")
        .map(str::to_owned)
        .collect())
}

/// 读取剪贴板中一个 target 的字节。
fn clipboard_read(target: &str) -> Result<Vec<u8>, String> {
    let output = Command::new("xclip")
        .args(["-selection", "clipboard", "-t", target, "-o"])
        .output()
        .map_err(|error| format!("读取剪贴板 {target} 失败：{error}"))?;
    if !output.status.success() || output.stdout.is_empty() {
        return Err(format!("剪贴板 {target} 没有内容"));
    }
    Ok(output.stdout)
}
