use super::*;

// ── Table ───────────────────────────────────────────────────────────

#[test]
fn table_rows_are_flattened() {
    let doc = parse("| A | B |\n|---|---|\n| 1 | 2 |\n");
    let table_rows: Vec<_> = doc
        .rows
        .iter()
        .filter(|r| matches!(r.kind, MarkdownPreviewRowKind::TableRow { .. }))
        .collect();
    assert!(table_rows.len() >= 2);
    assert!(matches!(
        table_rows[0].kind,
        MarkdownPreviewRowKind::TableRow { is_header: true }
    ));
    assert_eq!(table_rows[0].text.as_ref(), "A | B");
    assert_eq!(table_rows[1].text.as_ref(), "1 | 2");
}

#[test]
fn table_rows_align_columns_across_block() {
    let doc = parse("| Name | Age |\n|---|---|\n| Alexander | 3 |\n| Bo | 27 |\n");
    let table_rows: Vec<_> = doc
        .rows
        .iter()
        .filter(|r| matches!(r.kind, MarkdownPreviewRowKind::TableRow { .. }))
        .collect();
    assert_eq!(table_rows.len(), 3);

    let header_sep = table_rows[0]
        .text
        .find('|')
        .expect("header row should contain a column separator");
    let first_row_sep = table_rows[1]
        .text
        .find('|')
        .expect("body row should contain a column separator");
    let second_row_sep = table_rows[2]
        .text
        .find('|')
        .expect("body row should contain a column separator");

    assert_eq!(header_sep, first_row_sep);
    assert_eq!(first_row_sep, second_row_sep);
}

#[test]
fn table_alignment_without_inline_spans_keeps_rows_plain() {
    let doc = parse("| Name | Age |\n|---|---|\n| Alexander | 3 |\n| Bo | 27 |\n");
    let table_rows: Vec<_> = doc
        .rows
        .iter()
        .filter(|r| matches!(r.kind, MarkdownPreviewRowKind::TableRow { .. }))
        .collect();

    assert!(table_rows.iter().all(|row| row.inline_spans.is_empty()));
    assert_eq!(table_rows[0].text.as_ref(), "Name      | Age");
    assert_eq!(table_rows[1].text.as_ref(), "Alexander | 3");
    assert_eq!(table_rows[2].text.as_ref(), "Bo        | 27");
}

#[test]
fn table_alignment_preserves_inline_spans_after_padding_cells() {
    let doc = parse(
        "| A | **Header Bold** |\n| --- | --- |\n| A much longer first column | [link](https://example.com) |\n",
    );
    let table_rows: Vec<_> = doc
        .rows
        .iter()
        .filter(|r| matches!(r.kind, MarkdownPreviewRowKind::TableRow { .. }))
        .collect();
    assert_eq!(table_rows.len(), 2);

    let header_sep = table_rows[0]
        .text
        .find('|')
        .expect("header row should contain a column separator");
    let body_sep = table_rows[1]
        .text
        .find('|')
        .expect("body row should contain a column separator");
    assert_eq!(header_sep, body_sep);

    let header_bold = spans_with_style(table_rows[0], MarkdownInlineStyle::Bold);
    assert_eq!(header_bold.len(), 1);
    assert_eq!(
        &table_rows[0].text.as_ref()[header_bold[0].byte_range.clone()],
        "Header Bold"
    );

    let body_links = spans_with_style(table_rows[1], MarkdownInlineStyle::Link);
    assert_eq!(body_links.len(), 1);
    assert_eq!(
        &table_rows[1].text.as_ref()[body_links[0].byte_range.clone()],
        "link"
    );
}

#[test]
fn table_alignment_handles_inline_spans_in_earlier_cells() {
    let doc =
        parse("| **Header Bold** | B |\n| --- | --- |\n| [link](https://example.com) | plain |\n");
    let table_rows: Vec<_> = doc
        .rows
        .iter()
        .filter(|r| matches!(r.kind, MarkdownPreviewRowKind::TableRow { .. }))
        .collect();
    assert_eq!(table_rows.len(), 2);

    let header_bold = spans_with_style(table_rows[0], MarkdownInlineStyle::Bold);
    assert_eq!(header_bold.len(), 1);
    assert_eq!(
        &table_rows[0].text.as_ref()[header_bold[0].byte_range.clone()],
        "Header Bold"
    );

    let body_links = spans_with_style(table_rows[1], MarkdownInlineStyle::Link);
    assert_eq!(body_links.len(), 1);
    assert_eq!(
        &table_rows[1].text.as_ref()[body_links[0].byte_range.clone()],
        "link"
    );
}

#[test]
fn row_width_cache_does_not_affect_preview_row_equality() {
    let cached = parse("Paragraph\n").rows.remove(0);
    let fresh = parse("Paragraph\n").rows.remove(0);

    cached.measured_width_px.get_or_init(1, || 123);

    assert_eq!(cached, fresh);
}

#[test]
fn cloned_row_preserves_cached_width_measurement() {
    let cached = parse("Paragraph\n").rows.remove(0);
    cached.measured_width_px.get_or_init(1, || 123);

    let cloned = cached.clone();

    assert_eq!(cloned.measured_width_px.get_or_init(1, || 999), 123);
}
