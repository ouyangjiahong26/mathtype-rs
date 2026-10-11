# WPS 侧对象激活能力实测报告（#15）

日期：2026-10-11 · 分支：`docs/wps-activation-report` · 全部证据与脚本在 `/tmp/wps-probe/`（不入库）。

## 1. 结论速览（三分类）

| 能力 | 分类 | 依据 |
| --- | --- | --- |
| 转换桥 `mathtype --serve` 在 Linux 供 WPS 加载项调用 | **已验证可行** | §4.1：`/health`、`latex→latex`、`latex→png`、`latex→mtef` 全部 200 |
| WPS Linux 支持 JS 加载项（jsaddons）并能拉取我们的加载项 | **已验证可行**（发现与拉取） | §4.2：WPS 实际从 `http://127.0.0.1:3890/` 抓取了 `ribbon.xml`、`main.js` |
| 加载项 ribbon 完整加载（OnAddinLoad / 四个按钮可用） | **需人工步骤** | §4.3：本机 WPS 12.1.2 只对 `authaddin.json` 中已授权的加载项（Zotero）完成加载；我们的加载项无未attended授权路径，无法盲操作完成一次性启用 |
| WPS Linux 打开含第三方 ProgID OLE 对象的 docx | **已验证可行**（不报错、不丢数据） | §5.1：窗口正常打开，无错误对话框，对象部件保留 |
| 双击激活我们的 OLE 服务器（进入编辑器） | **不可行（本机/本版本），且原因明确** | §5.2：双击后无进程拉起（execve 计数不变）、无对话框、无编辑器出现；Linux 无 COM/SCM，我们的 ole-server 是 Windows 二进制 |
| 修改后保存回写对象数据 | **未完成验证**（限制） | §5.3：未修改文档上 Ctrl+S 不改写文件（md5 不变，符合预期）；盲操作下无法稳定先修改再保存，留人工步骤 |

## 2. 环境

- OS：Ubuntu-ish Linux x64，GNOME Wayland（mutter）+ Xwayland，`DISPLAY=:0`，双屏 8000x5120（Qt 2x DPI）。
- WPS：`wps-office 12.1.2.28080.AK.preread.sw`（dpkg），安装于 `/opt/kingsoft/wps-office/office6`，`cfgs/setup.cfg`：`MajorVersion=12 / 1 / 2 / Build 28080, BuildId=260810-M88I10, x64, UILanguage=2052`。注意：任务书假设的 `/opt/kingsoft/office6` 不存在，实际路径如上。
- 本机无 Word；`soffice`（LibreOffice）可用，用于交叉验证 docx 结构。
- 无 OCR/视觉通道（Wayland 下 root 截图全黑、GNOME 截图 D-Bus 被拒、无 tesseract；rapidocr 装上后无像素可喂）。GUI 验证依靠：xdotool（窗口名/激活/点击）、AT-SPI 无障碍树（`QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1` 后 WPS 暴露部分控件）、strace（文件/进程）、HTTP 服务日志、剪贴板。

## 3. 加载项部署（install.ps1 的 Linux 等价步骤）

记录于 `/tmp/wps-probe/install-addin.sh`（未入仓库）。要点：

1. 拷贝 `ribbon.xml` + `main.js`（另复制到 `js/main.js`、生成 `index.html`）到
   `~/.local/share/Kingsoft/wps/jsaddons/MathTypeRs_0.1.0/` 与 `MathTypeRs_/`。
2. 在 `~/.local/share/Kingsoft/wps/jsaddons/jsplugins.xml` 与 `publish.xml` 注册（保留 Zotero 等已有条目）。
3. **本地目录形式不生效**；改为官方 wpsjs/MathCapture 形式：静态 HTTP 服务
   （`python3 -m http.server 3890 --bind 127.0.0.1`，工作目录含 ribbon.xml/main.js/index.html）
   + `publish.xml` 条目
   `<jspluginonline name="MathTypeRs" type="wps" url="http://127.0.0.1:3890/" debug="" enable="enable_dev" install="null"/>`。

产品侧小修（本分支）：`apps/wps-addin/main.js` 两处平台分隔符——`tempPngPath()` 与
`OnExportPng` 现按 `GetTempPath()`/`doc.Path` 里实际出现的分隔符补 `\` 或 `/`
（Windows 路径行为不变；`node --check` 通过；无自动测试，属加载项脚本的运行时适配）。

## 4. JS 加载项实测

### 4.1 转换桥（已验证）

```
cargo build -p mathtype && ./target/debug/mathtype --serve 8760   # /tmp/wps-probe/bridge.log
curl http://127.0.0.1:8760/health                                  -> ok
POST /convert?from=latex&to=latex  'E=mc^2'                        -> E=mc^2
POST /convert?from=latex&to=png    'E=mc^2'                        -> PNG 232x79
POST /convert?from=latex&to=mtef   '\frac{a}{b}+\sqrt{x_i^2}'      -> 288 字节 EQNOLEFILEHDR+MTEF
```

`main.js` 依赖的全部端点（`/health`、`from=latex&to=latex`、`from=latex&to=png`）在 Linux 均可用。

### 4.2 WPS 确实发现了我们的加载项（已验证）

- strace（`strace-eq.log`）显示 WPS 启动时读取 `jsaddons/publish.xml`、`jsaddons/databuffer`、`authaddin.json`；
  但**从不读取本地 `jsplugins.xml`**——官方文档（open.wps.cn《WPS 加载项开发说明》）确认
  jsplugins.xml 模式“从 12.1.0.16910 版本开始已被限制”，本机 12.1.2.28080 属受限版本。
- 换成 `jspluginonline` + http URL 后，WPS 在 11:54:04 实际发起拉取
  （`/tmp/wps-probe/addin-http.log`）：`GET /ribbon.xml 200`、`GET /main.js 200`。

### 4.3 未能完成完整加载（需人工步骤）

- 拉取之后没有后续请求（未打开 index.html），`OnAddinLoad` 的桥接探针
  （`GET /health?src=wps-addin`）从未出现在 bridge.log；随后多次重启不再重新拉取。
- `authaddin.json`（WPS 的用户授权记录，含不可伪造的加密 md5 令牌）自始至终只有 Zotero 条目。
  Zotero 的装机先例（`Zotero-Jsa_1.0.0/install.py` + `wps权限设置.docx`）表明：加载项需用户在
  WPS 界面里完成一次性启用（宏安全性/信任 JS 项目访问等，本机 `Office.conf` 中
  `JSIDEProjectTrust=1`、`VbaSecurityLevel=1` 已就绪），之后 WPS 才把它写进 `authaddin.json` 并每次加载。
- 本机无法视觉读取 WPS 菜单（见 §2），故一次性人工启用未能盲操作完成。

**人工复现步骤（约 2 分钟）**：① 重跑 §3 第 3 步（服务 + publish.xml）；
② 启动 WPS 文字，打开任一 docx；③ 工具/开发工具 → 加载项（COM 加载项）里勾选 `MathTypeRs`
（或按 Zotero 文档：工具→宏安全性→可靠发行商→勾选“信任对于 wps js 项目的访问”）；
④ 观察点：功能区出现“公式”标签（4 个大按钮）；`addin-http.log` 出现 `index.html`/`main.js` 请求；
`bridge.log` 出现 `GET /health?src=wps-addin -> 200`（= OnAddinLoad 已执行）。
⑤ 选中 LaTeX 文本点“插入公式”：预期先试 `InlineShapes.AddOLEObject`（WPS JSAPI 若未暴露则回退），
回退路径为图片插入——`bridge.log` 应出现 `POST /convert?from=latex&to=png -> 200`；
“转为 LaTeX”对应 `from=latex&to=latex`。

## 5. OLE 对象呈现实测（核心问题）

### 5.1 构造与打开（已验证）

构造脚本（`/tmp/wps-probe/`）：`make-cfb.py`（手写最小 CFB 复合文件：512B 扇区、1 FAT、
1 目录扇区、miniFAT + 64B mini 流，根目录 CLSID={7C3E9A54-1B62-4E7D-9F03-5A8C2D6E1B47}，
双流 `Equation Native`（EQNOLEFILEHDR+MTEF，即 mt_mtef 输出）与 `MathTypeRs Equation`（UTF-8 LaTeX））、
`make-ole-docx.py`（zipfile 打包 docx：`w:object` = `v:shape`+`v:imagedata`（桥渲染的 PNG 预览）
+ `o:OLEObject Type="Embed" ProgID="MathTypeRs.Equation.1"` → `embeddings/oleObject1.bin`）。

- 自校验：olefile 0.47 读回双流与 CLSID 正确；用我们自己的桥做 `mtef→latex` 往返得到原式；
  soffice 将该 docx 转 PDF 成功（`check/eq-test.pdf`）。
- WPS 打开 `/tmp/wps-probe/eq-big-work.docx`：窗口 `eq-big-work.docx - WPS 文字 - 兼容模式` 正常出现，
  **无错误/警告对话框，无崩溃**；strace 显示读取 docx；`.bin` 未落盘解包（内存内解析）。

### 5.2 双击激活（不可行，原因明确）

对对象所在行多点位双击后：

- `strace-eq2.log` 的 execve 总数不变（前后均 375）——**没有任何进程被拉起**；
- 无 `mathtype`/ole-server 子进程；无“错误/无法激活/服务器”等对话框窗口；
- 与预期一致：Linux 无 COM/SCM 激活基础设施，我们的 ole-server 是 Windows 二进制，
  WPS Linux 对未知 ProgID 的嵌入对象既不激活也不报错，静默呈现。

（限制：本机无视觉通道，未能截图确认 WPS 把对象呈现为预览图还是占位图标；盲点击的
坐标校准受 Xwayland 焦点/覆盖窗口干扰，但多点位覆盖了对象行，且“无进程/无对话框”的
负结果与静默呈现的推断一致。人工 1 分钟可补：打开 `eq-big.docx` 看第二行居中公式图即可。）

### 5.3 保存回写（未完成验证，限制）

未修改文档上 Ctrl+S：`eq-big-work.docx` md5 前后一致（`c7612b5e…`），WPS 不做无谓改写；
“先输入文字再保存”的修改-保存-解包回读实验在盲操作下不稳定，未取得有效样本。
人工补测：打开 `eq-big.docx` → 在文末输入一个字符 → Ctrl+S →
`unzip -p 文件 word/document.xml | grep OLEObject` 应仍含 `ProgID="MathTypeRs.Equation.1"`，
且 `word/embeddings/oleObject1.bin` 仍在（WPS 对未知对象按原样保留是常见行为，但需实测确认）。

## 6. 对 Windows 侧 #19 的启示

1. **插入/激活的真实路径仍是 OLE**：`AddOLEObject(MathTypeRs.Equation.1)` + 双击 DoVerb，
   只有 Windows 的 Word/WPS 能走通；WPS Linux 只能“呈现不可编辑”。
2. **本报告的构造物可直接当 #19 的测试夹具生成器**：`make-cfb.py` 产出的复合文件已通过
   olefile 与 mt_mtef 双向校验（CLSID、双流、EQNOLEFILEHDR+MTEF 布局与 ole-server
   `storage.rs`/`clsid.rs` 的约定一致），Windows 侧可用来做“打开别人生成的文档”的回归。
3. **WPS Linux 侧的可用编辑形态**是 JS 加载项 + 桥（图片/OMML 互通），不要在 Linux 上
   规划 OLE 激活；`publish.xml + jspluginonline + localhost HTTP` 是官方可用部署形态，
   但需用户一次性授权（安装器应内置该引导，参考 Zotero 的 install.py + 权限说明文档）。

## 7. 证据索引（/tmp/wps-probe/）

- `bridge.log`（桥全程日志）；`addin-http.log`（WPS 拉取加载项的 200 记录，11:54:04）；
- `strace-eq.log`、`strace-eq2.log`（WPS 启动读取 publish.xml/authaddin.json、execve 计数）；
- `install-addin.sh`、`make-cfb.py`、`make-ole-docx.py`、`make-text-docx.py`、`atspi.py`、`capture.py`、`ocr.py`（探查工具）；
- `oleObject1.bin`、`eq.mtef`、`eq.tex`、`eq-big.png`、`eq-test.docx`、`eq-big.docx`（实验物）；
- `atspi-*.txt`（WPS 暴露的控件树快照）、`*.log`（各次启动记录）。
