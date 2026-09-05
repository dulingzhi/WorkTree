use super::*;

#[test]
fn unsupported_html_degrades_cleanly() {
    let doc = parse("<div>block html</div>\n");
    assert_eq!(doc.rows.len(), 1);
    assert_eq!(doc.rows[0].kind, MarkdownPreviewRowKind::PlainFallback);
    assert_eq!(doc.rows[0].text.as_ref(), "<div>block html</div>");
}

#[test]
fn inline_html_is_preserved_inside_paragraphs() {
    let doc = parse("Text with <b>html</b> inline\n");
    assert_eq!(doc.rows.len(), 1);
    assert_eq!(doc.rows[0].kind, MarkdownPreviewRowKind::Paragraph);
    assert_eq!(doc.rows[0].text.as_ref(), "Text with <b>html</b> inline");
}

#[test]
fn html_comments_are_hidden_from_preview() {
    let doc = parse("Visible <!-- hidden --> text\n");
    assert_eq!(doc.rows.len(), 1);
    assert_eq!(doc.rows[0].text.as_ref(), "Visible text");
}

#[test]
fn block_html_comments_do_not_create_rows() {
    let doc = parse("<!-- hidden -->\nVisible\n");
    assert_eq!(doc.rows.len(), 1);
    assert_eq!(doc.rows[0].text.as_ref(), "Visible");
}

#[test]
fn custom_anchor_tags_are_hidden_from_preview() {
    let doc = parse("# Section Heading\n\n<a name=\"my-custom-anchor-point\"></a>\nVisible\n");
    assert_eq!(doc.rows.len(), 3);
    assert_eq!(
        doc.rows[0].kind,
        MarkdownPreviewRowKind::Heading { level: 1 }
    );
    assert_eq!(doc.rows[0].text.as_ref(), "Section Heading");
    assert_eq!(doc.rows[1].kind, MarkdownPreviewRowKind::Spacer);
    assert_eq!(doc.rows[2].text.as_ref(), "Visible");
}

#[test]
fn custom_anchor_id_tags_are_hidden_from_preview() {
    let doc = parse("# Section Heading\n\n<a id=\"jump-target\"></a>\nVisible\n");
    assert_eq!(doc.rows.len(), 3);
    assert_eq!(
        doc.rows[0].kind,
        MarkdownPreviewRowKind::Heading { level: 1 }
    );
    assert_eq!(doc.rows[0].text.as_ref(), "Section Heading");
    assert_eq!(doc.rows[1].kind, MarkdownPreviewRowKind::Spacer);
    assert_eq!(doc.rows[2].text.as_ref(), "Visible");
}
