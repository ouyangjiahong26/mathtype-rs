# CONTEXT.md

面向公式编辑器的领域词汇与代码地图。提到领域概念时按这里的用词。

## 项目

用 Rust 重建一套完整的公式编辑与互通能力：桌面所见即所得编辑器、Word 与 WPS 加载项、以及
LaTeX / MathML / Office 原生公式（OMML）/ 图片之间的双向转换。目标是对标商业公式编辑器
MathType 的功能面与文档互通能力。

## 术语表

| 术语 | 含义 | 代码位置 |
| --- | --- | --- |
| 公式 (equation) | 一篇被排版的数学表达式，含一个根节点与字号预设 | `mt_core::Math` |
| 公式树 (equation tree) | 公式的树形表示：行、字符、模板 | `mt_core::Node` |
| 行 (row) | 横向排列的一串项；空行即空槽位 | `mt_core::Node::Row` |
| 槽位 (slot) | 模板按视觉顺序暴露的子位置，统一为整数下标 | `Node::slot` / `slot_count` |
| 模板 (template) | 自带槽位的构造：分式、根式、上下标、定界符、矩阵、重音、大运算符 | `Node::Frac` 等 |
| 路径 (path) | 从根到某节点所经过的槽位下标序列 | `mt_core::Path` |
| 原子类 (atom class) | 决定相邻项间距的类别：Ord/Op/Bin/Rel/Open/Close/Punct/Inner | `mt_core::Class` |
| 字体 (font) | 变量体、正体、粗体、花体、双线体等具体字面 | `mt_core::Font` |
| 字号预设 (size preset) | 全尺寸/正文/一级脚本/二级脚本四档 | `mt_core::MathStyle` |
| 光标 (cursor) | 所在行 + 行内插入位置 | `mt_edit::Cursor` |
| 面板 (palette) | 工具栏里成组的符号与模板条目 | `mt_edit::Palette` |
| 转换器 (translator) | 公式树与某外部格式之间的双向映射 | `mt-tex` / `mt-mathml` / `mt-omml` / `mt-mtef` |
| 嵌入对象 (embedded object) | Word/WPS 文档中以对象形式存的公式，带可重新编辑的数据流 | `mt-mtef` |
| 加载项 (add-in) | 宿主应用里提供公式命令的扩展 | `apps/`（Word、WPS）|
| 样式预设 (style preset) | 字号、间距、字体槽与对齐默认值的一整套样式；文本格式，可导入导出 | `mt-style-preset` |
| 字体槽 (style slot) | 功能样式（文本/函数/变量/希腊/符号/向量/数字）各自使用的字体 | `mt_layout::StyleFonts` |
| 数学字母变体 (math variant) | 字母在某字体下的 Unicode 数学字母数字区码位，单一数学字体面内取字形 | `mt_layout::math_variant` |
| 公式库 (formula library) | 用户数据目录下可检索、可插入的命名公式集 | `apps/mathtype/src/library.rs` |

## crate 地图

| crate | 职责 |
| --- | --- |
| `mt-core` | 公式树、槽位访问、路径导航。所有其它 crate 的共同契约 |
| `mt-tex` | LaTeX 源码 ⇄ 公式树 |
| `mt-mathml` | Presentation MathML ⇄ 公式树 |
| `mt-omml` | Office 原生公式 ⇄ 公式树 |
| `mt-mtef` | 嵌入对象数据流与偏好预设的读写 |
| `mt-docx` | .docx 容器读写与轻索引：定位段落/公式/嵌入对象/图片，替换与插入，未触碰部件原样写回 |
| `mt-html` | .docx → 网页导出：结构映射为 HTML，公式为行内 MathML（或 PNG），图片入 media/ 目录或 data URI 内嵌 |
| `mt-style-preset` | 样式预设的文本格式、内置预设，与 `LayoutStyle`、MTEF 偏好的互转 |
| `mt-layout` | 公式树 → 带位置的盒子树；字形度量抽象；间距表与字体槽可由预设注入 |
| `mt-render` | 盒子树 → 屏幕 / PNG / SVG / EPS / WMF |
| `mt-edit` | 编辑模型：光标导航、模板插入、撤销、面板内容 |
| `apps/mathtype` | 桌面编辑器；同一份二进制还提供转换命令行与回环 HTTP 转换接口（供宿主加载项调用） |
| `apps/ole-server` | 嵌入对象服务器：宿主双击激活与保存写回（ADR 0002 为准） |
| `apps/word-addin` | Word 加载项：在文档里插入/转换公式 |
| `apps/wps-addin` | WPS 加载项：命令面为 Word 子集——插入（OLE 或图片）、转 LaTeX、导出 PNG、导出网页、检查转换服务 |

## 约定

- `mt-core` 的树形结构是冻结契约：其它 crate 只读它、不改它；确需扩展时先改契约再加功能。
- 每个 crate 自带测试，验证命令为 `cargo test -p <crate>`。
- 新增第三方依赖需要在提交说明里给出理由。

## 相关文档

- `docs/agents/domain.md` — 领域文档的读取方式
- `docs/adr/` — 已确立的架构决策
