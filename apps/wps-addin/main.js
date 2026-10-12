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
  registerActivationEvents();
  return true;
}

// ---------------------------------------------------------------------------
// Insert commands, modelled on MathType's tab: inline, display, numbered left
// and right. Each opens the desktop editor (through the bridge) for a new
// equation and inserts the result as a real equation OBJECT in MathType's own
// format (ProgID Equation.DSMT4, an MTEF stream inside a compound file plus a
// rendered preview), so MathType users can edit the same equations. Because the
// host cannot create such an object directly, the add-in leaves a marker text
// in the document; the bridge replaces it at the file level and the document
// is reopened.
// ---------------------------------------------------------------------------

var inserting = false;

// The 1-based number of the paragraph the caret sits in, counted the way the host
// counts (document order, table cells included) — the same order the bridge's index
// walks, which is what lets it place an insertion by number alone.
function caretParagraphIndex(app) {
  var sel = app.Selection;
  var start = null;
  try { start = sel.Paragraphs.Item(1).Range.Start; } catch (e) { return 0; }
  if (start === null) return 0;
  var paragraphs = app.ActiveDocument.Paragraphs;
  var count = paragraphs.Count;
  for (var i = 1; i <= count; i++) {
    var paragraph;
    try { paragraph = paragraphs.Item(i); } catch (e2) { continue; }
    var paragraphStart = null;
    try { paragraphStart = paragraph.Range.Start; } catch (e3) { paragraphStart = null; }
    if (paragraphStart !== null && paragraphStart === start) return i;
  }
  return 0;
}

// POST /compose and answer with the LaTeX the user typed in the editor.
function bridgeCompose(onDone) {
  var xhr = new XMLHttpRequest();
  xhr.open('POST', BRIDGE_BASE + '/compose', true);
  xhr.setRequestHeader('Content-Type', 'text/plain; charset=utf-8');
  xhr.timeout = 3600000;
  xhr.onreadystatechange = function () {
    if (xhr.readyState !== 4) return;
    if (xhr.status !== 200) {
      onDone(new Error('HTTP ' + xhr.status + ': ' + xhr.responseText), null);
      return;
    }
    try {
      var answer = JSON.parse(xhr.responseText);
      onDone(null, answer.latex || '');
    } catch (e) {
      onDone(new Error('编辑器返回了无法解析的结果：' + (e && e.message ? e.message : e)), null);
    }
  };
  xhr.onerror = function () { onDone(new Error('网络错误'), null); };
  xhr.send('');
}

// The bridge makes the object at the file level and answers with a receipt.
function bridgeObjectize(path, latex, form, para, onDone) {
  var query = '/objectize?path=' + encodeURIComponent(path) +
    '&latex=' + encodeURIComponent(latex) +
    '&progid=' + encodeURIComponent('Equation.DSMT4') +
    '&form=' + encodeURIComponent(form) +
    '&para=' + para;
  var xhr = new XMLHttpRequest();
  xhr.open('POST', BRIDGE_BASE + query, true);
  xhr.setRequestHeader('Content-Type', 'text/plain; charset=utf-8');
  xhr.timeout = 3600000;
  xhr.onreadystatechange = function () {
    if (xhr.readyState !== 4) return;
    if (xhr.status !== 200) {
      onDone(new Error('HTTP ' + xhr.status + ': ' + xhr.responseText));
      return;
    }
    onDone(null);
  };
  xhr.onerror = function () { onDone(new Error('网络错误')); };
  xhr.send('');
}

// Insert one equation object in the requested form.
function insertEquationObject(form) {
  if (inserting) return;
  var app = hostApp();
  var doc = app.ActiveDocument;
  if (!doc || !doc.FullName || !doc.Path) {
    alert('请先保存文档，再插入公式。');
    return;
  }
  // The bridge edits the file on disk; unsaved WPS-side edits would be discarded.
  var unsaved = true;
  try { unsaved = doc.Saved === false; } catch (e0) { unsaved = false; }
  if (unsaved) {
    var go = false;
    try { go = wps.confirm('WPS 显示文档有未保存的修改（兼容模式打开也会这样标记）。\n插入公式将以磁盘上的版本为准并重新打开，未保存的修改会丢弃。继续吗？'); } catch (e1) { go = true; }
    if (!go) return;
  }
  inserting = true;
  bridgeCompose(function (err, latex) {
    if (err) { inserting = false; alert(bridgeUnreachableMessage(err.message)); return; }
    if (isBlank(latex)) { inserting = false; return; } // empty session: nothing to insert
    // The caret's paragraph number, as the host counts paragraphs; the bridge places the
    // object there (inline) or in a new paragraph after it (display and numbered). Nothing
    // is typed or saved in WPS, so WPS's writer never touches the document.
    var para = caretParagraphIndex(app);
    if (!para) {
      inserting = false;
      alert('无法定位插入点，请将光标放在正文段落中再试。');
      return;
    }
    var fullPath = doc.FullName;
    bridgeObjectize(fullPath, latex, form, para, function (err2) {
      inserting = false;
      if (err2) { alert(bridgeUnreachableMessage(err2.message)); return; }
      try {
        try { app.DisplayAlerts = 0; } catch (e4) { /* not all builds expose it */ }
        doc.Saved = true;
        doc.Close(0);
        try { app.DisplayAlerts = 1; } catch (e5) { /* leave as is */ }
        app.Documents.Open(fullPath);
      } catch (e3) {
        try { app.DisplayAlerts = 1; } catch (e6) { /* leave as is */ }
        alert('公式已写入。请手动操作：关闭文档时选择「不保存」，然后重新打开：' + fullPath +
          '\n（不要用 WPS「保存/另存」，那会重写整个文档并破坏 MathType 格式）');
      }
    });
  });
}

function OnInsertInline(control) { insertEquationObject('inline'); }
function OnInsertDisplay(control) { insertEquationObject('display'); }
function OnInsertNumberedRight(control) { insertEquationObject('numbered-right'); }
function OnInsertNumberedLeft(control) { insertEquationObject('numbered-left'); }

function OnCheckBridge(control) {
  bridgeGet('/health', function (err, body) {
    if (err) { alert(bridgeUnreachableMessage(err.message)); return; }
    alert('转换服务正常（' + BRIDGE_BASE + '）：' + body);
  });
}

// ---------------------------------------------------------------------------
// Object activation ("double-click to edit") — WPS on Linux has no OLE host, so
// the add-in asks the bridge to hand the object's equation to the desktop
// editor and gives back the updated document. Selected object + button always
// works; a real double-click works wherever WPS exposes the event.
// ---------------------------------------------------------------------------

// One editing round trip is in flight; further triggers are ignored.
var activating = false;

// Equations the bridge can edit: our own objects and MathType's (`Equation.DSMT4`).
function isOurEquationShape(shape) {
  try {
    var progId = shape.OLEFormat.ProgID;
    return !!progId && (progId.indexOf('MathTypeRs') === 0 || progId === 'Equation.DSMT4');
  } catch (e) {
    return false;
  }
}

// The 1-based ordinal of the MathTypeRs object under the selection, counting
// only such objects, or 0 when none is selected.
function selectedEquationOrdinal(app) {
  var doc = app.ActiveDocument;
  if (!doc || !doc.InlineShapes) return 0;
  var sel = null;
  try {
    if (app.Selection && app.Selection.InlineShapes && app.Selection.InlineShapes.Count > 0) {
      sel = app.Selection.InlineShapes.Item(1);
    }
  } catch (e) { sel = null; }
  var ordinal = 0;
  var start = null;
  try { start = sel ? sel.Range.Start : null; } catch (e2) { start = null; }
  var count = doc.InlineShapes.Count;
  for (var i = 1; i <= count; i++) {
    var shape;
    try { shape = doc.InlineShapes.Item(i); } catch (e3) { continue; }
    if (!isOurEquationShape(shape)) continue;
    ordinal++;
    if (start !== null) {
      var shapeStart = null;
      try { shapeStart = shape.Range.Start; } catch (e4) { shapeStart = null; }
      if (shapeStart !== null && shapeStart === start) return ordinal;
    }
  }
  // Nothing (or nothing recognisable) selected: the only object still works.
  return ordinal === 1 ? 1 : 0;
}

// Save, hand the object's equation to the editor through the bridge (which reads
// and rewrites the file itself, so document size is no limit), and reopen it.
function activateEquationObject(app, index) {
  if (activating) return;
  var doc = app.ActiveDocument;
  if (!doc || !doc.FullName || !doc.Path) {
    alert('请先保存文档，再编辑公式对象。');
    return;
  }
  activating = true;
  var fullPath = doc.FullName;
  // Never call doc.Save() here: WPS's own writer re-serialises the whole document
  // (and that is what scrambles MathType documents). The bridge edits the file on
  // disk, so whatever WPS holds unsaved would be discarded by the reopen — ask first.
  var unsaved = true;
  try { unsaved = doc.Saved === false; } catch (e0) { unsaved = false; }
  if (unsaved) {
    var go = false;
    try { go = wps.confirm('WPS 显示文档有未保存的修改（兼容模式打开也会这样标记）。\n编辑公式将以磁盘上的版本为准并重新打开，未保存的修改会丢弃。继续吗？'); } catch (e1) { go = true; }
    if (!go) { activating = false; return; }
  }
  var xhr = new XMLHttpRequest();
  xhr.open('POST', BRIDGE_BASE + '/activate?index=' + index +
    '&path=' + encodeURIComponent(fullPath) + '&out=' + encodeURIComponent(fullPath), true);
  xhr.setRequestHeader('Content-Type', 'text/plain; charset=utf-8');
  xhr.timeout = 3600000; // the request lives as long as the editing session
  xhr.onreadystatechange = function () {
    if (xhr.readyState !== 4) return;
    activating = false;
    if (xhr.status !== 200) {
      alert(bridgeUnreachableMessage('HTTP ' + xhr.status + ': ' + xhr.responseText));
      return;
    }
    try {
      try { app.DisplayAlerts = 0; } catch (e4) { /* not all builds expose it */ }
      doc.Saved = true;
      doc.Close(0);
      try { app.DisplayAlerts = 1; } catch (e5) { /* leave as is */ }
      app.Documents.Open(fullPath);
    } catch (e) {
      try { app.DisplayAlerts = 1; } catch (e6) { /* leave as is */ }
      alert('文档已在磁盘上更新。请手动操作：关闭文档时选择「不保存」，然后重新打开：' + fullPath +
        '\n（不要用 WPS「保存/另存」，那会重写整个文档并破坏 MathType 格式）');
    }
  };
  xhr.onerror = function () {
    activating = false;
    alert(bridgeUnreachableMessage('网络错误'));
  };
  xhr.send('');
}

// Ribbon entry: edit the selected equation object.
function OnEditEquation(control) {
  var app = hostApp();
  var index = selectedEquationOrdinal(app);
  if (index === 0) {
    alert('请先点击选中一个 MathType 公式对象，再点“编辑公式”。');
    return;
  }
  activateEquationObject(app, index);
}

// Where WPS exposes them, hook the document events: a double click edits the
// object outright, and selecting one explains what to do.
function registerActivationEvents() {
  function hook(name, handler) {
    try {
      if (typeof wps !== 'undefined' && wps.ApiEvent && wps.ApiEvent.AddApiEventListener) {
        wps.ApiEvent.AddApiEventListener(name, handler);
        return true;
      }
    } catch (e) { /* fall through */ }
    try {
      if (typeof Application !== 'undefined' && Application.ApiEvent &&
          Application.ApiEvent.AddApiEventListener) {
        Application.ApiEvent.AddApiEventListener(name, handler);
        return true;
      }
    } catch (e2) { /* this build does not expose the event */ }
    return false;
  }
  var doubleClick = hook('WindowBeforeDoubleClick', function () {
    var app = hostApp();
    var index = selectedEquationOrdinal(app);
    if (index === 0) return;
    try { if (typeof wps !== 'undefined' && wps.ApiEvent) wps.ApiEvent.Cancel = true; } catch (e) {}
    activateEquationObject(app, index);
  });
  var selection = hook('WindowSelectionChange', function () {
    var app = hostApp();
    if (selectedEquationOrdinal(app) > 0) {
      console.log('[MathTypeRs] 公式对象已选中：双击或点“编辑公式”进入编辑。');
    }
  });
  if (doubleClick || selection) {
    console.log('[MathTypeRs] 激活事件已注册（双击=' + doubleClick + '，选区=' + selection + '）。');
  }
}
