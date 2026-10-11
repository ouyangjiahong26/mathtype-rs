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
  if (dir && !/[\\\/]$/.test(dir)) dir += '\\';
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
    if (typeof obj.SetLatex === 'function') obj.SetLatex(latex);
    if (typeof obj.Save === 'function') obj.Save();
    return { ok: true, reason: '' };
  } catch (e) {
    // Put the LaTeX back so the picture fallback still has it.
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
    outPath = doc.Path + '\\' + stripExtension(doc.Name) + '.png';
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

// Probe the bridge and report the result.
function OnCheckBridge(control) {
  bridgeGet('/health', function (err, body) {
    if (err) { alert(bridgeUnreachableMessage(err.message)); return; }
    alert('转换服务正常（' + BRIDGE_BASE + '）：' + body);
  });
}
