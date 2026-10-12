# mathtype-rs

用 Rust 重建的公式编辑与互通能力，目标是对标商业公式编辑器 MathType 的功能面与文档互通能力。

## 功能概览

- 桌面所见即所得编辑器：面板插入模板与符号，画布直接编辑，光标导航与撤销。
- LaTeX、Presentation MathML、Office 原生公式（OMML）、MTEF 之间双向转换。
- 渲染输出 PNG、SVG、EPS、EMF。EMF 是矢量预览图，供需要矢量图的宿主使用。
- .docx 读写与批量转换：定位全篇公式，预览统计，逐项导出或替换。
- 公式编号与交叉引用：生成 SEQ、MACROBUTTON、REF 域，按章节断点复位计数器。
- Word 与 WPS 加载项：在宿主文档里插入与转换公式。

## crate 地图

| crate | 职责 |
| --- | --- |
| `mt-core` | 公式树、槽位访问、路径导航。所有其它 crate 的共同契约 |
| `mt-tex` | LaTeX 源码与公式树的双向转换 |
| `mt-mathml` | Presentation MathML 与公式树的双向转换 |
| `mt-omml` | Office 原生公式与公式树的双向转换 |
| `mt-mtef` | 嵌入对象数据流与偏好预设的读写 |
| `mt-docx` | .docx 容器读写与轻索引：定位段落/公式/嵌入对象/图片，替换与插入，未触碰部件原样写回 |
| `mt-html` | .docx 导出为网页：结构映射为 HTML，公式为行内 MathML（或 PNG），图片入 media/ 目录或 data URI 内嵌 |
| `mt-style-preset` | 样式预设的文本格式、内置预设，与 `LayoutStyle`、MTEF 偏好的互转 |
| `mt-batch` | 文档级批量转换：枚举全篇公式、预览统计、逐项转换（清单/导出/替换三种模式）、失败逐项记录、中止回调 |
| `mt-numbering` | 公式编号与交叉引用：SEQ/MACROBUTTON/REF 域生成、编号段落组装、章/节复位域、编号设置持久化、已有编号枚举 |
| `mt-layout` | 公式树排成带位置的盒子树。字形度量抽象，间距表与字体槽可由预设注入 |
| `mt-render` | 盒子树输出到屏幕、PNG、SVG、EPS、EMF |
| `mt-edit` | 编辑模型：光标导航、模板插入、撤销、面板内容 |
| `apps/mathtype` | 桌面编辑器，同一份二进制还提供转换命令行与回环 HTTP 转换接口（供宿主加载项调用） |
| `apps/ole-server` | 嵌入对象服务器：宿主双击激活与保存写回（ADR 0002 为准） |
| `apps/word-addin` | Word 加载项：在文档里插入/转换公式 |
| `apps/wps-addin` | WPS 加载项：命令面为 Word 子集，见 `CONTEXT.md` 的 crate 地图 |

## 快速开始

构建全部 crate：

```bash
cargo build --release
```

不带参数启动图形编辑器：

```bash
cargo run -p mathtype --release
```

单次转换，结果写文件：

```bash
cargo run -p mathtype --release -- --to png --out eq.png '\frac{-b\pm\sqrt{b^2-4ac}}{2a}'
```

`--to` 接受 `latex`、`mathml`、`omml`、`mtef`、`svg`、`png`、`eps`，省略时默认 `svg`。`--from` 省略时按输入推断，`--in`、`--out` 从文件读写，`--size` 设全尺寸点数（默认 18），`--scale` 设 png 的每点设备像素（默认 3）。`--help` 打印完整用法。

## 子命令

| 命令 | 说明 |
| --- | --- |
| `--serve [端口]` | 回环 HTTP 转换接口，默认端口 8760，供不能启动进程的宿主加载项（WPS）调用 |
| `--edit [文件]` | 在编辑器里打开以文本保存的公式，默认 `equation.tex`，保存时写回 |
| `batch-convert` | 文档级批量转换：枚举全篇公式，按清单、导出或替换模式处理 |
| `number` | 为文档里的公式生成编号域，可按章节或全篇、左或右侧 |
| `crossref` | 插入指向编号书签的 REF 交叉引用域 |
| `export-html` | .docx 导出为网页目录，公式为行内 MathML 或 PNG 图片 |
| `clip-copy` | 把 LaTeX 公式以 MTEF、HTML、LaTeX、PNG 写入剪贴板 |
| `clip-paste` | 读回剪贴板里的公式并输出 LaTeX |

## 文档

- `CONTEXT.md`：领域词汇与代码地图
- `docs/adr/`：架构决策记录
- `AGENTS.md`：协作约定

## 开发

测试命令与 CI 一致：

```bash
cargo test --workspace
```

提交标题用逗号、冒号或空格分隔语义，不用破折号。

## 许可证

MIT OR Apache-2.0，见 `LICENSE-MIT` 与 `LICENSE-APACHE`。
