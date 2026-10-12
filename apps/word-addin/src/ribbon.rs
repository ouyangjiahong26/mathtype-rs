//! The ribbon the add-in contributes to Word.

/// The ribbon markup returned from `GetCustomUI`.
///
/// The `onAction` values are member names of the add-in object, which Office looks up with
/// `GetIDsOfNames` and calls through `Invoke`.
pub const CUSTOM_UI: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<customUI xmlns="http://schemas.microsoft.com/office/2006/01/customui">
  <ribbon>
    <tabs>
      <tab id="MathTypeRsTab" label="MathType-rs">
        <group id="MathTypeRsGroup" label="公式">
          <button id="MathTypeRsInsertObject" label="插入公式对象" size="large"
                  onAction="InsertEquationObject"
                  screentip="把选中的 LaTeX 插入为可编辑的公式对象"
                  supertip="以嵌入对象形式插入公式；双击对象打开公式编辑器，保存时把编辑结果写回文档。"/>
          <button id="MathTypeRsToEquation" label="转为公式"
                  onAction="ConvertSelectionToEquation"
                  screentip="把选中的 LaTeX 转换为公式"
                  supertip="读取选区中的 LaTeX 源码，用公式核心排版后作为 Word 原生公式插入。"/>
          <button id="MathTypeRsToLatex" label="转为 LaTeX"
                  onAction="ConvertSelectionToLatex"
                  screentip="把选中的公式转换为 LaTeX"
                  supertip="读取选区中的 Word 公式，写出等价的 LaTeX 源码替换选区。"/>
          <button id="MathTypeRsToggle" label="智能切换"
                  onAction="TeXToggle"
                  screentip="按选区内容双向转换"
                  supertip="选区里是公式就转成 LaTeX，是 LaTeX 就转成公式。"/>
          <button id="MathTypeRsExportPng" label="导出 PNG"
                  onAction="ExportSelectionPng"
                  screentip="把选中的 LaTeX 渲染成图片"
                  supertip="把选区中的 LaTeX 排版渲染为 PNG，写到文档所在目录。"/>
          <button id="MathTypeRsExportHtml" label="导出网页"
                  onAction="ExportDocumentHtml"
                  screentip="把文档导出为网页"
                  supertip="把整个文档导出为网页目录：公式转为 MathML，图片写入 media/ 子目录，浏览器直接打开。"/>
          <button id="MathTypeRsBatchConvert" label="批量转换…" size="large"
                  onAction="BatchConvertDocument"
                  screentip="整篇文档的公式批量转换"
                  supertip="读取整篇文档，把每个公式转换为 Office 原生公式（读取失败的公式对象跳过并记录），结果另存为 &lt;名&gt;-converted.docx；有选区时只转换选区内的公式（在选区两侧加书签定位）。"/>
        </group>
        <group id="MathTypeRsNumberingGroup" label="编号">
          <button id="MathTypeRsInsertNumbered" label="插入编号公式" size="large"
                  onAction="InsertNumberedEquation"
                  screentip="把选中的 LaTeX 插入为带编号的公式"
                  supertip="插入公式与制表位，编号由 Word 域生成（MACROBUTTON + SEQ），更新域后自动重排；编号格式沿用本文档的编号设置。"/>
          <button id="MathTypeRsChapterBreak" label="下一章"
                  onAction="InsertChapterBreak"
                  screentip="插入章复位域"
                  supertip="在光标处插入复位域：章计数加一，公式计数清零，之后更新域时编号按新章重排。"/>
          <button id="MathTypeRsSectionBreak" label="下一节"
                  onAction="InsertSectionBreak"
                  screentip="插入节复位域"
                  supertip="在光标处插入复位域：节计数加一，公式计数清零，之后更新域时编号按新节重排。"/>
          <button id="MathTypeRsCrossRef" label="交叉引用" size="large"
                  onAction="InsertCrossReference"
                  screentip="引用最近一个编号"
                  supertip="在光标处插入指向最近一个公式编号的 REF 域（超链接）；编号变化后更新域即同步。"/>
        </group>
      </tab>
    </tabs>
  </ribbon>
</customUI>
"#;

/// Member ids of the object's fixed members, as the host spells them.
///
/// The extensibility ids are the ones the host's type library gives `IDTExtensibility2`; the
/// ribbon is also reachable by name, in case the host looks it up on the dispatch interface.
pub mod dispid {
    /// `DISPID_VALUE`: the default member. A host asking for it is given the add-in object, which
    /// is what `Application.COMAddIns("MathTypeRs.Addin").Object` needs.
    pub const DEFAULT: i32 = 0;

    /// `OnConnection`.
    pub const ON_CONNECTION: i32 = 1;
    /// `OnDisconnection`.
    pub const ON_DISCONNECTION: i32 = 2;
    /// `OnAddInsUpdate`.
    pub const ON_ADD_INS_UPDATE: i32 = 3;
    /// `OnStartupComplete`.
    pub const ON_STARTUP_COMPLETE: i32 = 4;
    /// `OnBeginShutdown`.
    pub const ON_BEGIN_SHUTDOWN: i32 = 5;
    /// `GetCustomUI`.
    pub const GET_CUSTOM_UI: i32 = 6;

    /// The member id of the ribbon or connection member `name`, if it is one.
    pub fn fixed(name: &str) -> Option<i32> {
        let (name, dispid) = FIXED.iter().find(|(member, _)| name.eq_ignore_ascii_case(member))?;
        let _ = name;
        Some(*dispid)
    }

    /// The fixed members of the object, by name.
    const FIXED: [(&str, i32); 7] = [
        ("Object", DEFAULT),
        ("OnConnection", ON_CONNECTION),
        ("OnDisconnection", ON_DISCONNECTION),
        ("OnAddInsUpdate", ON_ADD_INS_UPDATE),
        ("OnStartupComplete", ON_STARTUP_COMPLETE),
        ("OnBeginShutdown", ON_BEGIN_SHUTDOWN),
        ("GetCustomUI", GET_CUSTOM_UI),
    ];
}

#[cfg(test)]
mod tests {
    use super::CUSTOM_UI;

    /// The callbacks the ribbon names, read out of the markup itself.
    fn actions() -> Vec<&'static str> {
        CUSTOM_UI
            .split("onAction=\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .collect()
    }

    #[test]
    fn every_button_names_a_callback() {
        assert_eq!(
            actions(),
            [
                "InsertEquationObject",
                "ConvertSelectionToEquation",
                "ConvertSelectionToLatex",
                "TeXToggle",
                "ExportSelectionPng",
                "ExportDocumentHtml"
            ]
        );
    }

    #[test]
    fn the_tab_is_labelled_in_chinese() {
        assert!(CUSTOM_UI.contains("label=\"公式\""));
        assert!(CUSTOM_UI.contains(r#"xmlns="http://schemas.microsoft.com/office/2006/01/customui""#));
    }

    #[test]
    fn the_markup_is_balanced() {
        let mut stack: Vec<&str> = Vec::new();
        let mut rest = CUSTOM_UI;
        while let Some(at) = rest.find('<') {
            rest = &rest[at..];
            let end = rest.find('>').expect("every tag ends");
            let tag = &rest[1..end];
            rest = &rest[end + 1..];
            if tag.starts_with('?') || tag.starts_with('!') {
                continue;
            }
            if let Some(name) = tag.strip_prefix('/') {
                assert_eq!(stack.pop(), Some(name), "a closing tag matches the last opening one");
            } else if !tag.ends_with('/') {
                stack.push(tag.split_whitespace().next().expect("a tag has a name"));
            }
        }
        assert!(stack.is_empty(), "every element is closed: {stack:?}");
    }
}
