// MathTypeRs WPS 文字 add-in.
//
// All equation work is delegated to the local conversion bridge (mathtype --serve).
// The add-in itself only reads the current selection, talks to the bridge over HTTP,
// and puts the result back into the document.
//
// Written in ES5 on purpose: the WPS embedded browser is an older Chromium build,
// where `fetch` promises can stay pending forever, so XMLHttpRequest is used instead.

// Single source of truth for the bridge address.
var BRIDGE_BASE = 'http://127.0.0.1:8760';

// Command the user has to run when the bridge is not reachable.
var BRIDGE_COMMAND = 'mathtype --serve 8760';

// ProgID of the equation OLE server. When it is registered, "插入公式" embeds
// an editable equation object instead of a flat picture.
var OLE_PROGID = 'MathTypeRs.Equation.1';

// The JS Core exposes the application as `window.wps` (alias of `window.Application`).
function hostApp() {
  return window.wps || window.Application;
}

function bridgeUnreachableMessage(detail) {
  return '无法连接本地转换服务 ' + BRIDGE_BASE + '。\n' +
    '请先运行：' + BRIDGE_COMMAND + '\n\n' +
    '详细信息：' + detail;
}

// ---------------------------------------------------------------------------
// Bridge HTTP helpers (callback style, XMLHttpRequest only)
// ---------------------------------------------------------------------------

// GET a text resource. onDone(err, text).
function bridgeGet(path, onDone) {
  var xhr = new XMLHttpRequest();
  xhr.open('GET', BRIDGE_BASE + path, true);
  xhr.onreadystatechange = function () {
    if (xhr.readyState !== 4) return;
    if (xhr.status === 200) onDone(null, xhr.responseText);
    else onDone(new Error('HTTP ' + xhr.status), null);
  };
  xhr.onerror = function () { onDone(new Error('网络错误'), null); };
  xhr.send();
}

// POST a text body to /convert and receive text. onDone(err, text).
function bridgePostText(query, body, onDone) {
  var xhr = new XMLHttpRequest();
  xhr.open('POST', BRIDGE_BASE + '/convert?' + query, true);
  xhr.setRequestHeader('Content-Type', 'text/plain; charset=utf-8');
  xhr.onreadystatechange = function () {
    if (xhr.readyState !== 4) return;
    if (xhr.status === 200) onDone(null, xhr.responseText);
    else onDone(new Error('HTTP ' + xhr.status + ': ' + xhr.responseText), null);
  };
  xhr.onerror = function () { onDone(new Error('网络错误'), null); };
  xhr.send(body);
}

// POST a text body to /convert and receive raw bytes as a binary string.
// `charset=x-user-defined` makes the browser hand back each byte as one char.
// onDone(err, binaryString).
function bridgePostBinary(query, body, onDone) {
  var xhr = new XMLHttpRequest();
  xhr.open('POST', BRIDGE_BASE + '/convert?' + query, true);
  xhr.overrideMimeType('text/plain; charset=x-user-defined');
  xhr.setRequestHeader('Content-Type', 'text/plain; charset=utf-8');
  xhr.onreadystatechange = function () {
    if (xhr.readyState !== 4) return;
    if (xhr.status === 200) onDone(null, xhr.responseText);
    else onDone(new Error('HTTP ' + xhr.status + ': ' + xhr.responseText), null);
  };
  xhr.onerror = function () { onDone(new Error('网络错误'), null); };
  xhr.send(body);
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

function selectionLatex() {
  var sel = hostApp().Selection;
  var text = sel.Text;
  return text ? text : '';
}

function isBlank(s) {
  return !s || s.replace(/\s/g, '') === '';
}

function tempPngPath() {
  var dir = '';
  try { dir = hostApp().Env.GetTempPath(); } catch (e) { dir = ''; }
  if (!dir) dir = '';
  // Append the separator the platform actually uses: GetTempPath returns a
  // Windows path on Windows hosts and a POSIX path on the Linux ones.
  if (dir && !/[\\\/]$/.test(dir)) dir += dir.indexOf('\\') >= 0 ? '\\' : '/';
  var name = 'mathtype-rs-' + new Date().getTime() + '.png';
  return dir + name;
}

function stripExtension(name) {
  var dot = name.lastIndexOf('.');
  return dot > 0 ? name.substring(0, dot) : name;
}

function writeBinaryFile(path, data) {
  hostApp().FileSystem.writeAsBinaryString(path, data);
}

// ---------------------------------------------------------------------------
// Ribbon callbacks
// ---------------------------------------------------------------------------

// Called once when WPS finishes loading the add-in's ribbon.
// The single health probe issued here is the bridge log line that proves the
// add-in was loaded.
function OnAddinLoad(ribbonUI) {
  bridgeGet('/health?src=wps-addin', function (err, body) {
    if (err) {
      console.error('[MathTypeRs] bridge health probe failed: ' + err.message);
    } else {
      console.log('[MathTypeRs] bridge health probe ok: ' + body);
    }
  });
  return true;
}

// Try to embed an editable equation object (OLE) so a double-click reopens the
// editor and saving writes the edited equation back. Returns {ok, reason}.
function tryInsertOle(latex) {
  var app = hostApp();
  var sel = app.Selection;
  var shapes = sel.InlineShapes;
  if (!shapes || typeof shapes.AddOLEObject !== 'function') {
    return { ok: false, reason: 'InlineShapes.AddOLEObject is not exposed by the WPS JSAPI' };
  }
  try {
    sel.Text = '';  // drop the LaTeX source; the embedded object replaces it
    var shape = shapes.AddOLEObject(OLE_PROGID, '', false, false, '', 0, '', sel.Range);
    var obj = shape && shape.OLEFormat && shape.OLEFormat.Object;
    if (!obj) throw new Error('OLEFormat.Object is not reachable');
    // The object only counts as inserted when it can take and store the equation; a missing member
    // means the source must go back rather than being lost.
    if (typeof obj.SetLatex !== 'function') throw new Error('the object does not accept LaTeX (no SetLatex)');
    if (typeof obj.Save !== 'function') throw new Error('the object has nowhere to store the equation (no Save)');
    obj.SetLatex(latex);
    obj.Save();
    return { ok: true, reason: '' };
  } catch (e) {
    // Put the LaTeX back so the source is not lost; the picture fallback still has it.
    try { sel.Text = latex; } catch (e2) {}
    return { ok: false, reason: (e && e.message ? e.message : e) };
  }
}

// Insert the selected LaTeX as an editable embedded object, or - when the OLE
// server is unavailable - as an inline picture rendered by the bridge.
function OnInsertEquation(control) {
  var app = hostApp();
  var latex = selectionLatex();
  if (isBlank(latex)) {
    alert('请先选中包含 LaTeX 公式的文本，再执行“插入公式”。');
    return;
  }

  var ole = tryInsertOle(latex);
  if (ole.ok) {
    console.log('[MathTypeRs] equation inserted as OLE object ' + OLE_PROGID);
    return;
  }
  console.log('[MathTypeRs] OLE insertion unavailable (' + ole.reason + '); inserting a rendered picture instead');

  bridgePostBinary('from=latex&to=png', latex, function (err, data) {
    if (err) { alert(bridgeUnreachableMessage(err.message)); return; }
    var path = tempPngPath();
    try {
      writeBinaryFile(path, data);
    } catch (e) {
      alert('写入临时图片失败：' + (e && e.message ? e.message : e) + '\n路径：' + path);
      return;
    }
    try {
      app.Selection.Text = '';
      app.Selection.InlineShapes.AddPicture(path, false, true);
    } catch (e2) {
      alert('插入图片失败：' + (e2 && e2.message ? e2.message : e2));
    }
  });
}

// Normalise the selected LaTeX through the bridge and replace the text with it.
function OnEquationToLatex(control) {
  var app = hostApp();
  var latex = selectionLatex();
  if (isBlank(latex)) {
    alert('请先选中包含 LaTeX 公式的文本，再执行“转为 LaTeX”。');
    return;
  }
  bridgePostText('from=latex&to=latex', latex, function (err, canonical) {
    if (err) { alert(bridgeUnreachableMessage(err.message)); return; }
    try {
      app.Selection.Text = canonical;
    } catch (e) {
      alert('替换文本失败：' + (e && e.message ? e.message : e));
    }
  });
}

// Render the selected LaTeX and save the PNG next to the document.
function OnExportPng(control) {
  var app = hostApp();
  var latex = selectionLatex();
  if (isBlank(latex)) {
    alert('请先选中包含 LaTeX 公式的文本，再执行“导出 PNG”。');
    return;
  }
  var doc = app.ActiveDocument;
  var outPath;
  if (doc && doc.Path) {
    // doc.Path is a Windows path on Windows hosts and a POSIX one on Linux.
    outPath = doc.Path + (doc.Path.indexOf('\\') >= 0 ? '\\' : '/') +
      stripExtension(doc.Name) + '.png';
  } else {
    outPath = tempPngPath();
  }
  bridgePostBinary('from=latex&to=png', latex, function (err, data) {
    if (err) { alert(bridgeUnreachableMessage(err.message)); return; }
    try {
      writeBinaryFile(outPath, data);
    } catch (e) {
      alert('写入 PNG 失败：' + (e && e.message ? e.message : e) + '\n路径：' + outPath);
      return;
    }
    alert('已导出：' + outPath);
  });
}

// Export the whole document as a self-contained web page (inline MathML, embedded pictures).
// The bridge returns the HTML text with each byte as one character, so it is written back out
// byte-for-byte with writeAsBinaryString.
function OnExportHtml(control) {
  var app = hostApp();
  var doc = app.ActiveDocument;
  if (!doc || !doc.FullName || !doc.Path) {
    alert('请先保存文档，再执行“导出网页”。');
    return;
  }
  var outPath = doc.Path + '\\' + stripExtension(doc.Name) + '.html';
  var data;
  try {
    data = app.FileSystem.readAsBinaryString(doc.FullName);
  } catch (e) {
    alert('读取文档失败：' + (e && e.message ? e.message : e));
    return;
  }
  var xhr = new XMLHttpRequest();
  xhr.open('POST', BRIDGE_BASE + '/export-html', true);
  // x-user-defined keeps the .docx bytes one-character-per-byte on the way in, and hands the
  // HTML back the same way on the way out — the old-Chromium binary transport this host needs.
  xhr.overrideMimeType('text/plain; charset=x-user-defined');
  xhr.setRequestHeader('Content-Type', 'application/octet-stream');
  xhr.onreadystatechange = function () {
    if (xhr.readyState !== 4) return;
    if (xhr.status !== 200) {
      alert(bridgeUnreachableMessage('HTTP ' + xhr.status + ': ' + xhr.responseText));
      return;
    }
    try {
      writeBinaryFile(outPath, xhr.responseText);
    } catch (e) {
      alert('写入网页失败：' + (e && e.message ? e.message : e) + '\n路径：' + outPath);
      return;
    }
    alert('已导出：' + outPath);
  };
  xhr.onerror = function () { alert(bridgeUnreachableMessage('网络错误')); };
  xhr.send(data);
}

// Batch-convert every formula of the document through the bridge (#13). The document is
// read as bytes, POSTed to /batch, and the answer is JSON: the report plus, in replace
// mode, the rewritten .docx base64-encoded. The converted document is saved next to the
// original as <名>-converted.docx; the original is never touched in place.
function OnBatchConvert(control) {
  var app = hostApp();
  var doc = app.ActiveDocument;
  if (!doc || !doc.FullName || !doc.Path) {
    alert('请先保存文档，再执行“批量转换”。');
    return;
  }
  var sep = doc.Path.indexOf('\\') >= 0 ? '\\' : '/';
  var outPath = doc.Path + sep + stripExtension(doc.Name) + '-converted.docx';
  var format = 'omath';
  try {
    var asked = window.prompt('目标格式：latex / mathml / omath / png', 'omath');
    if (asked === null) return;
    asked = (asked || '').replace(/\s/g, '').toLowerCase();
    if (asked === 'latex' || asked === 'mathml' || asked === 'omath' || asked === 'png') {
      format = asked;
    }
  } catch (e) { /* keep the default */ }
  var data;
  try {
    data = app.FileSystem.readAsBinaryString(doc.FullName);
  } catch (e) {
    alert('读取文档失败：' + (e && e.message ? e.message : e));
    return;
  }
  var xhr = new XMLHttpRequest();
  // x-user-defined keeps the .docx bytes one-character-per-byte on the way in.
  xhr.open('POST', BRIDGE_BASE + '/batch?to=' + format + '&mode=replace', true);
  xhr.overrideMimeType('text/plain; charset=x-user-defined');
  xhr.setRequestHeader('Content-Type', 'application/octet-stream');
  xhr.onreadystatechange = function () {
    if (xhr.readyState !== 4) return;
    if (xhr.status !== 200) {
      alert(bridgeUnreachableMessage('HTTP ' + xhr.status + ': ' + xhr.responseText));
      return;
    }
    var answer;
    try {
      answer = JSON.parse(xhr.responseText);
    } catch (e) {
      alert('批量转换返回了无法解析的结果：' + (e && e.message ? e.message : e));
      return;
    }
    var report = answer.report || {};
    if (report.mode !== 'replace' || typeof answer.docx !== 'string') {
      alert('批量转换返回了意外的结果形态。');
      return;
    }
    try {
      writeBinaryFile(outPath, base64ToBinary(answer.docx));
    } catch (e) {
      alert('写入转换后的文档失败：' + (e && e.message ? e.message : e) + '\n路径：' + outPath);
      return;
    }
    var summary = '批量转换完成：共 ' + (report.total || 0) + ' 个，成功 ' +
      (report.succeeded || 0) + '，失败 ' + ((report.failures || []).length) + '。\n' +
      '已另存为：' + outPath;
    var failures = report.failures || [];
    for (var i = 0; i < failures.length && i < 5; i++) {
      summary += '\n  #' + failures[i].index + ' ' + failures[i].location + '：' + failures[i].error;
    }
    if (failures.length > 5) summary += '\n  …（其余见转换服务日志）';
    summary += '\n注意：整篇文档级转换；原文档未被修改。';
    alert(summary);
  };
  xhr.onerror = function () { alert(bridgeUnreachableMessage('网络错误')); };
  xhr.send(data);
}

// Decode standard base64 back into a one-character-per-byte binary string.
function base64ToBinary(text) {
  var alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  var out = [];
  var buffer = 0, bits = 0;
  for (var i = 0; i < text.length; i++) {
    var ch = text.charAt(i);
    if (ch === '=') break;
    var value = alphabet.indexOf(ch);
    if (value < 0) continue;
    buffer = (buffer << 6) | value;
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      out.push(String.fromCharCode((buffer >> bits) & 0xff));
    }
  }
  return out.join('');
}

// Probe the bridge and report the result.
function OnCheckBridge(control) {
  bridgeGet('/health', function (err, body) {
    if (err) { alert(bridgeUnreachableMessage(err.message)); return; }
    alert('转换服务正常（' + BRIDGE_BASE + '）：' + body);
  });
}
