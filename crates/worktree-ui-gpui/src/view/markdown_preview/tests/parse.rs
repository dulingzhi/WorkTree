use super::*;

#[test]
fn parse_does_not_reserve_rows_beyond_the_row_cap() {
    // Rows are per markdown *block*, so the source line count is unrelated
    // to how many there will be -- and `MAX_PREVIEW_SOURCE_BYTES` admits a
    // 1 MiB file of short lines. Reserving one row per line there costs
    // hundreds of megabytes for a document the parser caps far below it.
    let source = "\n".repeat(50_000);
    assert!(source.len() <= MAX_PREVIEW_SOURCE_BYTES);

    let doc = parse_markdown(&source).expect("blank lines parse");

    assert!(
        doc.rows.capacity() <= MAX_PREVIEW_ROWS,
        "reserved {} rows for a document capped at {MAX_PREVIEW_ROWS}",
        doc.rows.capacity()
    );
}

// ── Heading tests ───────────────────────────────────────────────────

#[test]
fn heading_levels_are_preserved() {
    let doc = parse("# H1\n## H2\n### H3\n#### H4\n##### H5\n###### H6\n");
    assert_eq!(
        row_kinds(&doc),
        vec![
            &MarkdownPreviewRowKind::Heading { level: 1 },
            &MarkdownPreviewRowKind::Heading { level: 2 },
            &MarkdownPreviewRowKind::Heading { level: 3 },
            &MarkdownPreviewRowKind::Heading { level: 4 },
            &MarkdownPreviewRowKind::Heading { level: 5 },
            &MarkdownPreviewRowKind::Heading { level: 6 },
        ]
    );
    assert_eq!(row_texts(&doc), vec!["H1", "H2", "H3", "H4", "H5", "H6"]);
}

#[test]
fn top_level_heading_inserts_section_spacer_before_following_content() {
    let doc = parse("# Title\n\nParagraph\n");

    assert_eq!(doc.rows.len(), 3);
    assert_eq!(
        doc.rows[0].kind,
        MarkdownPreviewRowKind::Heading { level: 1 }
    );
    assert_eq!(doc.rows[1].kind, MarkdownPreviewRowKind::Spacer);
    assert_eq!(doc.rows[1].change_hint, MarkdownChangeHint::None);
    assert_eq!(doc.rows[2].kind, MarkdownPreviewRowKind::Paragraph);
}

#[test]
fn content_before_top_level_heading_gets_section_spacer_before_heading() {
    let doc = parse("Paragraph\n\n# Title\n");

    assert_eq!(doc.rows.len(), 3);
    assert_eq!(doc.rows[0].kind, MarkdownPreviewRowKind::Paragraph);
    assert_eq!(doc.rows[1].kind, MarkdownPreviewRowKind::Spacer);
    assert_eq!(
        doc.rows[2].kind,
        MarkdownPreviewRowKind::Heading { level: 1 }
    );
}

#[test]
fn consecutive_headings_do_not_insert_spacers_between_heading_rows() {
    let doc = parse("# Title\n## Subtitle\n");

    assert_eq!(
        row_kinds(&doc),
        vec![
            &MarkdownPreviewRowKind::Heading { level: 1 },
            &MarkdownPreviewRowKind::Heading { level: 2 },
        ]
    );
}

#[test]
fn middle_heading_uses_one_section_spacer_not_two() {
    // The break above a section is a full blank row; the gap below the
    // heading comes from its own insets, so a second blank row here would
    // read as a hole in the document.
    let doc = parse("Intro\n\n# Title\n\nBody\n");

    assert_eq!(
        row_kinds(&doc),
        vec![
            &MarkdownPreviewRowKind::Paragraph,
            &MarkdownPreviewRowKind::Spacer,
            &MarkdownPreviewRowKind::Heading { level: 1 },
            &MarkdownPreviewRowKind::Paragraph,
        ]
    );
}

#[test]
fn heading_section_spacers_are_not_doubled_or_trailing() {
    // Consecutive headings share one break, and a heading that ends the
    // document gets no dangling spacer under it.
    assert_eq!(
        row_kinds(&parse("Intro\n\n# Title\n## Subtitle\n\nBody\n")),
        vec![
            &MarkdownPreviewRowKind::Paragraph,
            &MarkdownPreviewRowKind::Spacer,
            &MarkdownPreviewRowKind::Heading { level: 1 },
            &MarkdownPreviewRowKind::Heading { level: 2 },
            &MarkdownPreviewRowKind::Spacer,
            &MarkdownPreviewRowKind::Paragraph,
        ]
    );
    assert_eq!(
        row_kinds(&parse("Body\n\n# Title\n")),
        vec![
            &MarkdownPreviewRowKind::Paragraph,
            &MarkdownPreviewRowKind::Spacer,
            &MarkdownPreviewRowKind::Heading { level: 1 },
        ]
    );
}

// ── Paragraph tests ─────────────────────────────────────────────────

#[test]
fn paragraph_produces_one_row() {
    let doc = parse("Hello world.\n");
    assert_eq!(doc.rows.len(), 1);
    assert_eq!(doc.rows[0].kind, MarkdownPreviewRowKind::Paragraph);
    assert_eq!(doc.rows[0].text.as_ref(), "Hello world.");
}

#[test]
fn multiline_paragraph_normalizes_whitespace() {
    let doc = parse("Line one\nLine two\nLine three\n");
    assert_eq!(doc.rows.len(), 1);
    assert_eq!(doc.rows[0].text.as_ref(), "Line one Line two Line three");
}

#[test]
fn hard_breaks_split_paragraph_rows() {
    let doc = parse("This example  \nWill span two lines\n");
    assert_eq!(doc.rows.len(), 2);
    assert_eq!(doc.rows[0].kind, MarkdownPreviewRowKind::Paragraph);
    assert_eq!(doc.rows[0].text.as_ref(), "This example");
    assert_eq!(doc.rows[1].kind, MarkdownPreviewRowKind::Paragraph);
    assert_eq!(doc.rows[1].text.as_ref(), "Will span two lines");
}

#[test]
fn backslash_hard_breaks_split_paragraph_rows() {
    let doc = parse("This example\\\nWill span two lines\n");
    assert_eq!(doc.rows.len(), 2);
    assert_eq!(doc.rows[0].text.as_ref(), "This example");
    assert_eq!(doc.rows[1].text.as_ref(), "Will span two lines");
}

#[test]
fn html_br_splits_paragraph_rows() {
    let doc = parse("This example<br/>\nWill span two lines\n");
    assert_eq!(doc.rows.len(), 2);
    assert_eq!(doc.rows[0].text.as_ref(), "This example");
    assert_eq!(doc.rows[1].text.as_ref(), "Will span two lines");
}

#[test]
fn whitespace_normalization_preserves_inline_span_offsets() {
    let doc = parse("Prefix  **bold**\nnext line\n");
    assert_eq!(doc.rows.len(), 1);
    assert_eq!(doc.rows[0].text.as_ref(), "Prefix bold next line");

    let bold_span = doc.rows[0]
        .inline_spans
        .iter()
        .find(|span| span.style == MarkdownInlineStyle::Bold)
        .expect("expected bold span");
    assert_eq!(
        &doc.rows[0].text.as_ref()[bold_span.byte_range.clone()],
        "bold"
    );
}

// ── List tests ──────────────────────────────────────────────────────

#[test]
fn unordered_list_items_become_rows() {
    let doc = parse("- alpha\n- beta\n- gamma\n");
    assert_eq!(doc.rows.len(), 3);
    for row in &doc.rows {
        assert_eq!(row.kind, MarkdownPreviewRowKind::ListItem { number: None });
    }
    assert_eq!(row_texts(&doc), vec!["alpha", "beta", "gamma"]);
}

#[test]
fn ordered_list_items_preserve_numbers() {
    let doc = parse("3. first\n4. second\n5. third\n");
    assert_eq!(doc.rows.len(), 3);
    assert_eq!(
        doc.rows[0].kind,
        MarkdownPreviewRowKind::ListItem { number: Some(3) }
    );
    assert_eq!(
        doc.rows[1].kind,
        MarkdownPreviewRowKind::ListItem { number: Some(4) }
    );
    assert_eq!(
        doc.rows[2].kind,
        MarkdownPreviewRowKind::ListItem { number: Some(5) }
    );
}

#[test]
fn loose_list_items_still_render_as_list_rows() {
    let doc = parse("- first\n\n- second\n");
    assert_eq!(doc.rows.len(), 2);
    for row in &doc.rows {
        assert_eq!(row.kind, MarkdownPreviewRowKind::ListItem { number: None });
    }
}

#[test]
fn nested_list_increases_indent() {
    let doc = parse("- outer\n  - inner\n");
    assert_eq!(doc.rows.len(), 2);
    assert_eq!(doc.rows[0].indent_level, 1);
    assert_eq!(doc.rows[1].indent_level, 2);
}

// ── Blockquote tests ────────────────────────────────────────────────

#[test]
fn blockquote_produces_blockquote_row() {
    let doc = parse("> quoted text\n");
    assert_eq!(doc.rows.len(), 1);
    assert_eq!(doc.rows[0].kind, MarkdownPreviewRowKind::BlockquoteLine);
    assert_eq!(doc.rows[0].text.as_ref(), "quoted text");
}

#[test]
fn multiline_blockquote_produces_one_row_per_logical_quote_line() {
    let doc = parse("> first line\n> second line\n");
    assert_eq!(doc.rows.len(), 2);
    assert_eq!(doc.rows[0].kind, MarkdownPreviewRowKind::BlockquoteLine);
    assert_eq!(doc.rows[0].text.as_ref(), "first line");
    assert_eq!(doc.rows[0].source_line_range, 0..1);
    assert_eq!(doc.rows[0].blockquote_level, 1);
    assert_eq!(doc.rows[1].kind, MarkdownPreviewRowKind::BlockquoteLine);
    assert_eq!(doc.rows[1].text.as_ref(), "second line");
    assert_eq!(doc.rows[1].source_line_range, 1..2);
    assert_eq!(doc.rows[1].blockquote_level, 1);
}

#[test]
fn nested_blockquotes_preserve_quote_depth_per_row() {
    let doc = parse("> outer\n>> inner\n>>> deepest\n");
    assert_eq!(doc.rows.len(), 3);
    assert_eq!(doc.rows[0].blockquote_level, 1);
    assert_eq!(doc.rows[1].blockquote_level, 2);
    assert_eq!(doc.rows[2].blockquote_level, 3);
}

#[test]
fn list_items_inside_blockquotes_keep_quote_depth() {
    let doc = parse("> - first\n>> 3. second\n");
    assert_eq!(doc.rows.len(), 2);
    assert_eq!(
        doc.rows[0].kind,
        MarkdownPreviewRowKind::ListItem { number: None }
    );
    assert_eq!(doc.rows[0].blockquote_level, 1);
    assert_eq!(
        doc.rows[1].kind,
        MarkdownPreviewRowKind::ListItem { number: Some(3) }
    );
    assert_eq!(doc.rows[1].blockquote_level, 2);
}

#[test]
fn code_block_inside_blockquote_keeps_quote_depth() {
    let doc = parse("> ```\n> code\n> ```\n");
    let cr = code_rows(&doc);
    assert_eq!(cr.len(), 1);
    assert_eq!(cr[0].text.as_ref(), "code");
    assert_eq!(cr[0].blockquote_level, 1);
}

#[test]
fn gfm_alert_blockquotes_capture_alert_kind_and_hide_marker_line() {
    let doc = parse("> [!NOTE]\n> Line 1.\n> Line 2.\n");
    assert_eq!(doc.rows.len(), 2);
    assert_eq!(doc.rows[0].kind, MarkdownPreviewRowKind::BlockquoteLine);
    assert_eq!(doc.rows[0].text.as_ref(), "Line 1.");
    assert_eq!(doc.rows[0].alert_kind, Some(MarkdownAlertKind::Note));
    assert!(doc.rows[0].starts_alert);
    assert_eq!(doc.rows[1].text.as_ref(), "Line 2.");
    assert_eq!(doc.rows[1].alert_kind, Some(MarkdownAlertKind::Note));
    assert!(!doc.rows[1].starts_alert);
}

#[test]
fn nested_alert_blockquotes_stay_scoped_to_inner_quote_rows() {
    let doc = parse("> outer\n>\n> > [!WARNING]\n> > inner\n>\n> outer again\n");
    assert_eq!(doc.rows.len(), 3);

    assert_eq!(doc.rows[0].text.as_ref(), "outer");
    assert_eq!(doc.rows[0].blockquote_level, 1);
    assert_eq!(doc.rows[0].alert_kind, None);
    assert!(!doc.rows[0].starts_alert);

    assert_eq!(doc.rows[1].text.as_ref(), "inner");
    assert_eq!(doc.rows[1].blockquote_level, 2);
    assert_eq!(doc.rows[1].alert_kind, Some(MarkdownAlertKind::Warning));
    assert!(doc.rows[1].starts_alert);

    assert_eq!(doc.rows[2].text.as_ref(), "outer again");
    assert_eq!(doc.rows[2].blockquote_level, 1);
    assert_eq!(doc.rows[2].alert_kind, None);
    assert!(!doc.rows[2].starts_alert);
}

// ── Code block tests ────────────────────────────────────────────────

#[test]
fn fenced_code_block_one_row_per_line() {
    let doc = parse("```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n");
    let code_rows = code_rows(&doc);
    assert_eq!(code_rows.len(), 3);
    assert_eq!(code_rows[0].text.as_ref(), "fn main() {");
    assert_eq!(code_rows[1].text.as_ref(), "    println!(\"hi\");");
    assert_eq!(code_rows[2].text.as_ref(), "}");
    assert_eq!(
        code_rows[0].code_language,
        Some(crate::view::rows::DiffSyntaxLanguage::Rust)
    );
}

#[test]
fn code_block_first_last_flags() {
    let doc = parse("```\na\nb\nc\n```\n");
    let code_rows = code_rows(&doc);
    assert_eq!(code_rows.len(), 3);
    assert!(matches!(
        code_rows[0].kind,
        MarkdownPreviewRowKind::CodeLine {
            is_first: true,
            is_last: false
        }
    ));
    assert!(matches!(
        code_rows[1].kind,
        MarkdownPreviewRowKind::CodeLine {
            is_first: false,
            is_last: false
        }
    ));
    assert!(matches!(
        code_rows[2].kind,
        MarkdownPreviewRowKind::CodeLine {
            is_first: false,
            is_last: true
        }
    ));
}

#[test]
fn single_line_code_block_is_both_first_and_last() {
    let doc = parse("```\nonly\n```\n");
    let code_rows = code_rows(&doc);
    assert_eq!(code_rows.len(), 1);
    assert_eq!(code_rows[0].text.as_ref(), "only");
    assert!(matches!(
        code_rows[0].kind,
        MarkdownPreviewRowKind::CodeLine {
            is_first: true,
            is_last: true
        }
    ));
}

#[test]
fn indented_code_block_rows_keep_actual_source_line_ranges() {
    let doc = parse("    old\n    keep\n");
    let code_rows = code_rows(&doc);
    assert_eq!(code_rows.len(), 2);
    assert_eq!(code_rows[0].text.as_ref(), "old");
    assert_eq!(code_rows[0].source_line_range, 0..1);
    assert_eq!(code_rows[1].text.as_ref(), "keep");
    assert_eq!(code_rows[1].source_line_range, 1..2);
}

#[test]
fn fenced_code_block_preserves_trailing_blank_line() {
    let doc = parse("```\na\n\n```\n");
    let code_rows = code_rows(&doc);
    assert_eq!(code_rows.len(), 2);
    assert_eq!(code_rows[0].text.as_ref(), "a");
    assert_eq!(code_rows[0].source_line_range, 1..2);
    assert_eq!(code_rows[1].text.as_ref(), "");
    assert_eq!(code_rows[1].source_line_range, 2..3);
    assert!(matches!(
        code_rows[1].kind,
        MarkdownPreviewRowKind::CodeLine {
            is_first: false,
            is_last: true
        }
    ));
}

#[test]
fn empty_fenced_code_block_produces_single_empty_row() {
    let doc = parse("```\n```\n");
    let code_rows = code_rows(&doc);
    assert_eq!(code_rows.len(), 1);
    assert_eq!(code_rows[0].text.as_ref(), "");
    assert!(matches!(
        code_rows[0].kind,
        MarkdownPreviewRowKind::CodeLine {
            is_first: true,
            is_last: true
        }
    ));
    assert_eq!(code_rows[0].code_language, None);
}

#[test]
fn fenced_code_block_language_aliases_are_resolved() {
    let doc = parse("```language-typescript\nconst x = 1;\n```\n");
    let code_rows = code_rows(&doc);
    assert_eq!(code_rows.len(), 1);
    assert_eq!(
        code_rows[0].code_language,
        Some(crate::view::rows::DiffSyntaxLanguage::TypeScript)
    );
}

#[test]
fn wide_fenced_code_blocks_set_horizontal_scroll_hints() {
    let long_line = "scroll_hint_token_".repeat(6);
    let doc = parse(&format!("```text\n{long_line}\nshort\n```\n"));
    let code_rows = code_rows(&doc);

    assert_eq!(code_rows.len(), 2);
    assert!(
        code_rows
            .iter()
            .all(|row| row.code_block_horizontal_scroll_hint)
    );
}

// ── Thematic break ──────────────────────────────────────────────────

#[test]
fn thematic_break_produces_row() {
    let doc = parse("---\n");
    assert_eq!(doc.rows.len(), 1);
    assert_eq!(doc.rows[0].kind, MarkdownPreviewRowKind::ThematicBreak);
}

// ── Task list ───────────────────────────────────────────────────────

#[test]
fn task_list_markers_are_prepended() {
    let doc = parse("- [x] done\n- [ ] todo\n");
    assert_eq!(doc.rows.len(), 2);
    assert_eq!(doc.rows[0].text.as_ref(), "[x] done");
    assert_eq!(doc.rows[1].text.as_ref(), "[ ] todo");
}

#[test]
fn footnote_references_and_definitions_are_preserved() {
    let doc = parse("Here is a simple footnote[^1].\n\n[^1]: My reference.\n");
    assert_eq!(doc.rows.len(), 2);
    assert_eq!(doc.rows[0].text.as_ref(), "Here is a simple footnote[1].");
    let links = spans_with_style(&doc.rows[0], MarkdownInlineStyle::Link);
    assert_eq!(links.len(), 1);
    assert_eq!(
        &doc.rows[0].text.as_ref()[links[0].byte_range.clone()],
        "[1]"
    );

    assert_eq!(doc.rows[1].kind, MarkdownPreviewRowKind::Paragraph);
    assert_eq!(doc.rows[1].text.as_ref(), "My reference.");
    assert_eq!(
        doc.rows[1]
            .footnote_label
            .as_ref()
            .map(SharedString::as_ref),
        Some("1")
    );
    assert_eq!(doc.rows[1].indent_level, 1);
}

#[test]
fn footnote_definition_emits_label_only_for_first_rendered_row() {
    let doc = parse("Reference[^1].\n\n[^1]: First paragraph.\n\n    Second paragraph.\n");
    assert_eq!(doc.rows.len(), 3);

    assert_eq!(doc.rows[1].text.as_ref(), "First paragraph.");
    assert_eq!(
        doc.rows[1]
            .footnote_label
            .as_ref()
            .map(SharedString::as_ref),
        Some("1")
    );
    assert_eq!(doc.rows[1].indent_level, 1);

    assert_eq!(doc.rows[2].text.as_ref(), "Second paragraph.");
    assert_eq!(doc.rows[2].footnote_label, None);
    assert_eq!(doc.rows[2].indent_level, 1);
}

// ── Source line range tests ──────────────────────────────────────────

#[test]
fn source_line_ranges_are_plausible() {
    let doc = parse("# Heading\n\nParagraph\n");
    assert!(!doc.rows[0].source_line_range.is_empty());
    assert!(doc.rows[0].source_line_range.start < 5);
}

// ── Limit tests ─────────────────────────────────────────────────────

#[test]
fn parse_returns_none_for_oversized_source() {
    let huge = "x".repeat(MAX_PREVIEW_SOURCE_BYTES + 1);
    assert!(parse_markdown(&huge).is_none());
}

#[test]
fn parse_returns_none_when_rendered_rows_exceed_limit() {
    let too_many_rows = thematic_break_rows(MAX_PREVIEW_ROWS + 1);
    assert!(too_many_rows.len() < MAX_PREVIEW_SOURCE_BYTES);
    assert!(parse_markdown(&too_many_rows).is_none());
}

#[test]
fn parse_diff_returns_none_for_oversized_combined() {
    let big = "x".repeat(MAX_DIFF_PREVIEW_SOURCE_BYTES / 2 + 1);
    assert!(parse_markdown_diff(&big, &big).is_none());
}

#[test]
fn parse_diff_returns_none_when_one_side_exceeds_rendered_row_limit() {
    let too_many_rows = thematic_break_rows(MAX_PREVIEW_ROWS + 1);
    assert!(too_many_rows.len() < MAX_DIFF_PREVIEW_SOURCE_BYTES);
    assert!(parse_markdown_diff(&too_many_rows, "# ok\n").is_none());
}

#[test]
fn parse_diff_allows_single_side_over_single_preview_limit_within_combined_cap() {
    let old = "x".repeat(MAX_PREVIEW_SOURCE_BYTES + 1);
    let new = "y".repeat(MAX_DIFF_PREVIEW_SOURCE_BYTES - old.len());

    assert!(parse_markdown(&old).is_none());

    let (old_doc, new_doc) =
        parse_markdown_diff(&old, &new).expect("combined diff under 2 MiB should parse");
    assert_eq!(old_doc.rows.len(), 1);
    assert_eq!(new_doc.rows.len(), 1);
}

// ── Empty input ─────────────────────────────────────────────────────

#[test]
fn empty_source_produces_empty_document() {
    let doc = parse("");
    assert!(doc.rows.is_empty());
}

// ── Mixed document ──────────────────────────────────────────────────

#[test]
fn mixed_document_produces_correct_row_sequence() {
    let src = "\
# Title

A paragraph with **bold** text.

- item one
- item two

```
code line
```

---
";
    let doc = parse(src);

    // Should have: Heading, Spacer, Paragraph, ListItem, ListItem, CodeLine, ThematicBreak
    assert!(
        doc.rows.len() >= 7,
        "expected at least 7 rows, got {}",
        doc.rows.len()
    );
    assert!(matches!(
        doc.rows[0].kind,
        MarkdownPreviewRowKind::Heading { level: 1 }
    ));
    assert_eq!(doc.rows[1].kind, MarkdownPreviewRowKind::Spacer);
    assert_eq!(doc.rows[2].kind, MarkdownPreviewRowKind::Paragraph);
}

#[test]
fn build_line_starts_correct() {
    let src = "abc\ndef\nghi";
    let starts = build_line_starts(src);
    assert_eq!(starts, vec![0, 4, 8]);
}

#[test]
fn byte_offset_to_line_maps_correctly() {
    let starts = vec![0, 4, 8];
    assert_eq!(byte_offset_to_line(0, &starts), 0);
    assert_eq!(byte_offset_to_line(3, &starts), 0);
    assert_eq!(byte_offset_to_line(4, &starts), 1);
    assert_eq!(byte_offset_to_line(7, &starts), 1);
    assert_eq!(byte_offset_to_line(8, &starts), 2);
}

// ── Code span inside code block is not styled ────────────────────────

#[test]
fn code_block_lines_have_no_inline_spans() {
    let doc = parse("```\n**not bold** `not code`\n```\n");
    let code_rows = code_rows(&doc);
    assert_eq!(code_rows.len(), 1);
    assert!(
        code_rows[0].inline_spans.is_empty(),
        "inline spans inside code blocks should be empty"
    );
}

// ── Deeply nested list preserves indent levels ───────────────────────

#[test]
fn deeply_nested_lists_increment_indent() {
    let doc = parse("- a\n  - b\n    - c\n");
    assert!(doc.rows.len() >= 3);
    assert!(
        doc.rows[0].indent_level < doc.rows[1].indent_level,
        "second level should be more indented"
    );
    assert!(
        doc.rows[1].indent_level < doc.rows[2].indent_level,
        "third level should be more indented"
    );
}

// ── source_line_range helper ────────────────────────────────────────

#[test]
fn source_line_range_computes_correct_range() {
    let starts = build_line_starts("abc\ndef\nghi\n");
    // "abc\n" starts at 0 (line 0), "def\n" starts at 4 (line 1),
    // "ghi\n" starts at 8 (line 2)
    assert_eq!(source_line_range(0, 4, &starts), 0..1);
    assert_eq!(source_line_range(0, 8, &starts), 0..2);
    assert_eq!(source_line_range(4, 12, &starts), 1..3);
}

#[test]
fn source_line_range_handles_empty_range() {
    let starts = build_line_starts("abc\n");
    assert_eq!(source_line_range(0, 0, &starts), 0..1);
}

// ── Error message helpers ───────────────────────────────────────────

#[test]
fn single_preview_unavailable_reason_reports_size_for_oversized() {
    let reason = single_preview_unavailable_reason(MAX_PREVIEW_SOURCE_BYTES + 1);
    assert!(
        reason.contains("1 MiB"),
        "should mention size limit: {reason}"
    );
}

#[test]
fn single_preview_unavailable_reason_reports_rows_for_normal_size() {
    let reason = single_preview_unavailable_reason(100);
    assert!(
        reason.contains("row limit"),
        "should mention row limit: {reason}"
    );
}

#[test]
fn diff_preview_unavailable_reason_reports_size_for_oversized() {
    let reason = diff_preview_unavailable_reason(MAX_DIFF_PREVIEW_SOURCE_BYTES + 1);
    assert!(
        reason.contains("2 MiB"),
        "should mention size limit: {reason}"
    );
}

#[test]
fn diff_preview_unavailable_reason_reports_rows_for_normal_size() {
    let reason = diff_preview_unavailable_reason(100);
    assert!(
        reason.contains("row limit"),
        "should mention row limit: {reason}"
    );
}

#[test]
fn source_lines_are_found_for_byte_offsets() {
    // "a\nbb\n\nc" — line starts at 0, 2, 5, 6.
    let line_starts = [0usize, 2, 5, 6];

    assert_eq!(source_line_for_byte(0, &line_starts), 0);
    assert_eq!(source_line_for_byte(1, &line_starts), 0);
    assert_eq!(source_line_for_byte(2, &line_starts), 1);
    assert_eq!(source_line_for_byte(4, &line_starts), 1);
    assert_eq!(source_line_for_byte(5, &line_starts), 2);
    assert_eq!(source_line_for_byte(6, &line_starts), 3);
    // Past the end still resolves to the last line rather than panicking.
    assert_eq!(source_line_for_byte(9_999, &line_starts), 3);
    // And an empty table cannot underflow.
    assert_eq!(source_line_for_byte(3, &[]), 0);
}
