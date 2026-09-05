use super::*;

// ── Change hint annotation tests ────────────────────────────────────

#[test]
fn change_hints_mark_changed_rows() {
    let old_src = "# Title\n\nOld paragraph\n";
    let new_src = "# Title\n\nNew paragraph\n";
    let (mut old_doc, mut new_doc) = parse_markdown_diff(old_src, new_src).unwrap();

    // Line 2 (0-based) is changed in both.
    let old_mask = vec![false, false, true];
    let new_mask = vec![false, false, true];
    annotate_change_hints(&mut old_doc, &mut new_doc, &old_mask, &new_mask);

    // Title row should be unchanged.
    assert_eq!(old_doc.rows[0].change_hint, MarkdownChangeHint::None);
    assert_eq!(new_doc.rows[0].change_hint, MarkdownChangeHint::None);

    // Paragraph row should be marked.
    let old_para = old_doc
        .rows
        .iter()
        .find(|r| r.text.as_ref() == "Old paragraph")
        .unwrap();
    assert_eq!(old_para.change_hint, MarkdownChangeHint::Removed);
    let new_para = new_doc
        .rows
        .iter()
        .find(|r| r.text.as_ref() == "New paragraph")
        .unwrap();
    assert_eq!(new_para.change_hint, MarkdownChangeHint::Added);
}

#[test]
fn heading_spacing_rows_do_not_receive_change_hints() {
    let preview =
        build_markdown_diff_preview("# Title\n\nOld paragraph\n", "# Title\n\nNew paragraph\n")
            .unwrap();

    let old_spacer = preview
        .old
        .rows
        .iter()
        .find(|row| matches!(row.kind, MarkdownPreviewRowKind::Spacer))
        .expect("expected heading spacer row on old side");
    let new_spacer = preview
        .new
        .rows
        .iter()
        .find(|row| matches!(row.kind, MarkdownPreviewRowKind::Spacer))
        .expect("expected heading spacer row on new side");

    assert_eq!(old_spacer.change_hint, MarkdownChangeHint::None);
    assert_eq!(new_spacer.change_hint, MarkdownChangeHint::None);
}

#[test]
fn partial_change_ranges_use_modified_hint() {
    let (mut old_doc, mut new_doc) =
        parse_markdown_diff("line one\nline two\n", "line one\nline two\n").unwrap();

    let old_mask = vec![false, true];
    let new_mask = vec![false, true];
    annotate_change_hints(&mut old_doc, &mut new_doc, &old_mask, &new_mask);

    assert_eq!(old_doc.rows[0].change_hint, MarkdownChangeHint::Modified);
    assert_eq!(new_doc.rows[0].change_hint, MarkdownChangeHint::Modified);
}

#[test]
fn list_item_change_hints_follow_changed_lines() {
    let old_src = "- keep\n- remove me\n";
    let new_src = "- keep\n- add me\n";
    let (mut old_doc, mut new_doc) = parse_markdown_diff(old_src, new_src).unwrap();

    let old_mask = vec![false, true];
    let new_mask = vec![false, true];
    annotate_change_hints(&mut old_doc, &mut new_doc, &old_mask, &new_mask);

    assert_eq!(old_doc.rows[0].change_hint, MarkdownChangeHint::None);
    assert_eq!(old_doc.rows[1].change_hint, MarkdownChangeHint::Removed);
    assert_eq!(new_doc.rows[1].change_hint, MarkdownChangeHint::Added);
}

#[test]
fn changed_code_lines_are_marked_individually() {
    let old_src = "```\nold\nkeep\n```\n";
    let new_src = "```\nnew\nkeep\n```\n";
    let (mut old_doc, mut new_doc) = parse_markdown_diff(old_src, new_src).unwrap();

    let old_mask = vec![false, true, false, false];
    let new_mask = vec![false, true, false, false];
    annotate_change_hints(&mut old_doc, &mut new_doc, &old_mask, &new_mask);

    let old_code_rows = code_rows(&old_doc);
    let new_code_rows = code_rows(&new_doc);
    assert_eq!(old_code_rows[0].change_hint, MarkdownChangeHint::Removed);
    assert_eq!(old_code_rows[1].change_hint, MarkdownChangeHint::None);
    assert_eq!(new_code_rows[0].change_hint, MarkdownChangeHint::Added);
    assert_eq!(new_code_rows[1].change_hint, MarkdownChangeHint::None);
}

#[test]
fn changed_indented_code_lines_are_marked_individually() {
    let preview =
        build_markdown_diff_preview("    old\n    keep\n", "    new\n    keep\n").unwrap();

    let old_code_rows = code_rows(&preview.old);
    let new_code_rows = code_rows(&preview.new);

    assert_eq!(old_code_rows[0].source_line_range, 0..1);
    assert_eq!(old_code_rows[1].source_line_range, 1..2);
    assert_eq!(new_code_rows[0].source_line_range, 0..1);
    assert_eq!(new_code_rows[1].source_line_range, 1..2);
    assert_eq!(old_code_rows[0].change_hint, MarkdownChangeHint::Removed);
    assert_eq!(old_code_rows[1].change_hint, MarkdownChangeHint::None);
    assert_eq!(new_code_rows[0].change_hint, MarkdownChangeHint::Added);
    assert_eq!(new_code_rows[1].change_hint, MarkdownChangeHint::None);
}

#[test]
fn changed_trailing_blank_code_line_is_marked_individually() {
    let preview = build_markdown_diff_preview("```\na\n\n```\n", "```\na\nb\n```\n").unwrap();

    let old_code_rows = code_rows(&preview.old);
    let new_code_rows = code_rows(&preview.new);

    assert_eq!(old_code_rows.len(), 2);
    assert_eq!(new_code_rows.len(), 2);
    assert_eq!(old_code_rows[1].text.as_ref(), "");
    assert_eq!(new_code_rows[1].text.as_ref(), "b");
    assert_eq!(old_code_rows[1].source_line_range, 2..3);
    assert_eq!(new_code_rows[1].source_line_range, 2..3);
    assert_eq!(old_code_rows[1].change_hint, MarkdownChangeHint::Removed);
    assert_eq!(new_code_rows[1].change_hint, MarkdownChangeHint::Added);
}

#[test]
fn build_markdown_diff_preview_applies_change_hints() {
    let preview = build_markdown_diff_preview("- old item\n", "- new item\n").unwrap();

    assert_eq!(preview.old.rows.len(), 1);
    assert_eq!(preview.new.rows.len(), 1);
    assert_eq!(preview.old.rows[0].change_hint, MarkdownChangeHint::Removed);
    assert_eq!(preview.new.rows[0].change_hint, MarkdownChangeHint::Added);
}

#[test]
fn diff_preview_inserts_spacer_rows_for_added_markdown_blocks() {
    let preview = build_markdown_diff_preview("- keep\n", "- keep\n- add me\n").unwrap();

    assert_eq!(preview.old.rows.len(), 2);
    assert_eq!(preview.new.rows.len(), 2);
    assert_eq!(preview.old.rows[0].text.as_ref(), "keep");
    assert_eq!(preview.new.rows[0].text.as_ref(), "keep");
    assert_eq!(preview.old.rows[1].kind, MarkdownPreviewRowKind::Spacer);
    assert_eq!(preview.old.rows[1].change_hint, MarkdownChangeHint::None);
    assert_eq!(
        preview.new.rows[1].kind,
        MarkdownPreviewRowKind::ListItem { number: None }
    );
    assert_eq!(preview.new.rows[1].text.as_ref(), "add me");
    assert_eq!(preview.new.rows[1].change_hint, MarkdownChangeHint::Added);
}

#[test]
fn diff_preview_inserts_spacer_rows_for_removed_markdown_blocks() {
    let preview = build_markdown_diff_preview("keep\n\nremove me\n", "keep\n").unwrap();

    assert_eq!(preview.old.rows.len(), 2);
    assert_eq!(preview.new.rows.len(), 2);
    assert_eq!(preview.old.rows[0].text.as_ref(), "keep");
    assert_eq!(preview.new.rows[0].text.as_ref(), "keep");
    assert_eq!(preview.old.rows[1].kind, MarkdownPreviewRowKind::Paragraph);
    assert_eq!(preview.old.rows[1].text.as_ref(), "remove me");
    assert_eq!(preview.old.rows[1].change_hint, MarkdownChangeHint::Removed);
    assert_eq!(preview.new.rows[1].kind, MarkdownPreviewRowKind::Spacer);
    assert_eq!(preview.new.rows[1].change_hint, MarkdownChangeHint::None);
}

#[test]
fn diff_preview_builds_inline_document_for_changed_rows() {
    let preview = build_markdown_diff_preview("keep\n\nremove me\n", "keep\n\nadd me\n").unwrap();

    assert_eq!(preview.inline.rows.len(), 3);
    assert_eq!(preview.inline.rows[0].text.as_ref(), "keep");
    assert_eq!(preview.inline.rows[0].change_hint, MarkdownChangeHint::None);
    assert_eq!(preview.inline.rows[1].text.as_ref(), "remove me");
    assert_eq!(
        preview.inline.rows[1].change_hint,
        MarkdownChangeHint::Removed
    );
    assert_eq!(preview.inline.rows[2].text.as_ref(), "add me");
    assert_eq!(
        preview.inline.rows[2].change_hint,
        MarkdownChangeHint::Added
    );
}

#[test]
fn diff_preview_inline_document_merges_unchanged_rows_after_insertions() {
    let preview = build_markdown_diff_preview("- keep\n", "- add\n- keep\n").unwrap();

    assert_eq!(preview.inline.rows.len(), 2);
    assert_eq!(preview.inline.rows[0].text.as_ref(), "add");
    assert_eq!(
        preview.inline.rows[0].change_hint,
        MarkdownChangeHint::Added
    );
    assert_eq!(preview.inline.rows[1].text.as_ref(), "keep");
    assert_eq!(preview.inline.rows[1].change_hint, MarkdownChangeHint::None);
}

#[test]
fn diff_preview_aligns_added_code_lines_with_spacer_rows() {
    let preview = build_markdown_diff_preview("```\nkeep\n```\n", "```\nkeep\nadd\n```\n").unwrap();

    assert_eq!(preview.old.rows.len(), 2);
    assert_eq!(preview.new.rows.len(), 2);
    assert!(matches!(
        preview.old.rows[0].kind,
        MarkdownPreviewRowKind::CodeLine {
            is_first: true,
            is_last: true
        }
    ));
    assert!(matches!(
        preview.new.rows[0].kind,
        MarkdownPreviewRowKind::CodeLine {
            is_first: true,
            is_last: false
        }
    ));
    assert_eq!(preview.old.rows[1].kind, MarkdownPreviewRowKind::Spacer);
    assert!(matches!(
        preview.new.rows[1].kind,
        MarkdownPreviewRowKind::CodeLine {
            is_first: false,
            is_last: true
        }
    ));
    assert_eq!(preview.new.rows[1].text.as_ref(), "add");
    assert_eq!(preview.new.rows[1].change_hint, MarkdownChangeHint::Added);
}

#[test]
fn diff_preview_marks_last_line_change_with_trailing_newline() {
    // The diff engine and mask sizing both use str::lines(), which strips
    // trailing newlines. Verify that a change on the very last line is still
    // detected and annotated correctly regardless of trailing newline.
    let preview =
        build_markdown_diff_preview("# Same\n\nold last\n", "# Same\n\nnew last\n").unwrap();

    let old_last = preview.old.rows.last().unwrap();
    let new_last = preview.new.rows.last().unwrap();
    assert_eq!(old_last.text.as_ref(), "old last");
    assert_eq!(new_last.text.as_ref(), "new last");
    assert_eq!(old_last.change_hint, MarkdownChangeHint::Removed);
    assert_eq!(new_last.change_hint, MarkdownChangeHint::Added);
}

#[test]
fn diff_preview_marks_last_line_change_without_trailing_newline() {
    let preview = build_markdown_diff_preview("# Same\n\nold last", "# Same\n\nnew last").unwrap();

    let old_last = preview.old.rows.last().unwrap();
    let new_last = preview.new.rows.last().unwrap();
    assert_eq!(old_last.text.as_ref(), "old last");
    assert_eq!(new_last.text.as_ref(), "new last");
    assert_eq!(old_last.change_hint, MarkdownChangeHint::Removed);
    assert_eq!(new_last.change_hint, MarkdownChangeHint::Added);
}

#[test]
fn multiline_blockquote_change_hints_follow_changed_quote_lines() {
    let preview =
        build_markdown_diff_preview("> keep\n> remove me\n", "> keep\n> add me\n").unwrap();

    assert_eq!(preview.old.rows.len(), 2);
    assert_eq!(preview.new.rows.len(), 2);
    assert_eq!(preview.old.rows[0].change_hint, MarkdownChangeHint::None);
    assert_eq!(preview.new.rows[0].change_hint, MarkdownChangeHint::None);
    assert_eq!(preview.old.rows[1].change_hint, MarkdownChangeHint::Removed);
    assert_eq!(preview.new.rows[1].change_hint, MarkdownChangeHint::Added);
}

#[test]
fn mixed_markdown_blocks_keep_change_hints_scoped_to_changed_rows() {
    let old_src = concat!(
        "# Title\n",
        "\n",
        "- keep\n",
        "- old item\n",
        "\n",
        "```rust\n",
        "let old_value = 1;\n",
        "let stable = 2;\n",
        "```\n",
        "\n",
        "| Name | Count |\n",
        "| --- | --- |\n",
        "| keep | 1 |\n",
        "| old | 2 |\n",
    );
    let new_src = concat!(
        "# Title\n",
        "\n",
        "- keep\n",
        "- new item\n",
        "\n",
        "```rust\n",
        "let new_value = 1;\n",
        "let stable = 2;\n",
        "```\n",
        "\n",
        "| Name | Count |\n",
        "| --- | --- |\n",
        "| keep | 1 |\n",
        "| new | 3 |\n",
    );

    let preview = build_markdown_diff_preview(old_src, new_src).unwrap();

    assert_eq!(
        preview.old.rows[0].kind,
        MarkdownPreviewRowKind::Heading { level: 1 }
    );
    assert_eq!(preview.old.rows[0].change_hint, MarkdownChangeHint::None);
    assert_eq!(preview.new.rows[0].change_hint, MarkdownChangeHint::None);

    let old_list_rows: Vec<_> = preview
        .old
        .rows
        .iter()
        .filter(|row| matches!(row.kind, MarkdownPreviewRowKind::ListItem { .. }))
        .collect();
    let new_list_rows: Vec<_> = preview
        .new
        .rows
        .iter()
        .filter(|row| matches!(row.kind, MarkdownPreviewRowKind::ListItem { .. }))
        .collect();
    assert_eq!(old_list_rows[0].change_hint, MarkdownChangeHint::None);
    assert_eq!(new_list_rows[0].change_hint, MarkdownChangeHint::None);
    assert_ne!(old_list_rows[1].change_hint, MarkdownChangeHint::None);
    assert_ne!(new_list_rows[1].change_hint, MarkdownChangeHint::None);

    let old_code_rows = code_rows(&preview.old);
    let new_code_rows = code_rows(&preview.new);
    assert_eq!(old_code_rows[0].change_hint, MarkdownChangeHint::Removed);
    assert_eq!(old_code_rows[1].change_hint, MarkdownChangeHint::None);
    assert_eq!(new_code_rows[0].change_hint, MarkdownChangeHint::Added);
    assert_eq!(new_code_rows[1].change_hint, MarkdownChangeHint::None);

    let old_table_rows: Vec<_> = preview
        .old
        .rows
        .iter()
        .filter(|row| matches!(row.kind, MarkdownPreviewRowKind::TableRow { .. }))
        .collect();
    let new_table_rows: Vec<_> = preview
        .new
        .rows
        .iter()
        .filter(|row| matches!(row.kind, MarkdownPreviewRowKind::TableRow { .. }))
        .collect();
    assert_eq!(old_table_rows.len(), 3);
    assert_eq!(new_table_rows.len(), 3);
    assert_eq!(old_table_rows[0].change_hint, MarkdownChangeHint::None);
    assert_eq!(new_table_rows[0].change_hint, MarkdownChangeHint::None);
    assert_eq!(old_table_rows[1].change_hint, MarkdownChangeHint::None);
    assert_eq!(new_table_rows[1].change_hint, MarkdownChangeHint::None);
    assert_ne!(old_table_rows[2].change_hint, MarkdownChangeHint::None);
    assert_ne!(new_table_rows[2].change_hint, MarkdownChangeHint::None);
}

// ── plan_changed_line_masks ──────────────────────────────────────────

#[test]
fn plan_changed_line_masks_from_plan_rows() {
    use worktree_core::file_diff::{FileDiffPlan, FileDiffPlanRun};

    let plan = FileDiffPlan {
        runs: vec![
            FileDiffPlanRun::Context {
                old_start: 0,
                new_start: 0,
                len: 1,
            },
            FileDiffPlanRun::Remove {
                old_start: 1,
                len: 1,
            },
            FileDiffPlanRun::Add {
                new_start: 1,
                len: 1,
            },
        ],
        row_count: 3,
        inline_row_count: 3,
        eof_newline: None,
    };

    let (old_mask, new_mask) = worktree_core::file_diff::plan_changed_line_masks(&plan, 3, 3);
    assert!(!old_mask[0]); // context line
    assert!(old_mask[1]); // removed line
    assert!(!new_mask[0]); // context line
    assert!(new_mask[1]); // added line
}

// ── Modify-kind mask coverage ────────────────────────────────────────

#[test]
fn plan_changed_line_masks_handles_modify_kind() {
    use worktree_core::file_diff::{FileDiffPlan, FileDiffPlanRun};

    let plan = FileDiffPlan {
        runs: vec![FileDiffPlanRun::Modify {
            old_start: 0,
            new_start: 0,
            len: 1,
        }],
        row_count: 1,
        inline_row_count: 2,
        eof_newline: None,
    };

    let (old_mask, new_mask) = worktree_core::file_diff::plan_changed_line_masks(&plan, 2, 2);
    assert!(old_mask[0]); // modify marks old side
    assert!(!old_mask[1]);
    assert!(new_mask[0]); // modify marks new side
    assert!(!new_mask[1]);
}

// ── Identical content diff produces no change hints ──────────────────

#[test]
fn identical_content_diff_produces_no_change_hints() {
    let src = "# Title\n\nSame paragraph\n\n- item one\n";
    let preview = build_markdown_diff_preview(src, src).unwrap();

    for row in &preview.old.rows {
        assert_eq!(
            row.change_hint,
            MarkdownChangeHint::None,
            "old row {:?} should be unchanged",
            row.text
        );
    }
    for row in &preview.new.rows {
        assert_eq!(
            row.change_hint,
            MarkdownChangeHint::None,
            "new row {:?} should be unchanged",
            row.text
        );
    }
}

#[test]
fn markdown_diff_scrollbar_markers_show_added_rows_for_one_sided_preview() {
    let preview = build_markdown_diff_preview("", "# New\n").unwrap();

    assert_eq!(
        scrollbar_markers_for_diff_preview(&preview),
        vec![crate::view::components::ScrollbarMarker {
            start: 0.0,
            end: 1.0,
            kind: crate::view::components::ScrollbarMarkerKind::Add,
        }]
    );
}

#[test]
fn markdown_diff_scrollbar_markers_show_removed_rows_for_one_sided_preview() {
    let preview = build_markdown_diff_preview("# Gone\n", "").unwrap();

    assert_eq!(
        scrollbar_markers_for_diff_preview(&preview),
        vec![crate::view::components::ScrollbarMarker {
            start: 0.0,
            end: 1.0,
            kind: crate::view::components::ScrollbarMarkerKind::Remove,
        }]
    );
}

#[test]
fn markdown_diff_scrollbar_markers_merge_replacements_into_modify_markers() {
    let preview = build_markdown_diff_preview("old\n", "new\n").unwrap();

    assert_eq!(
        scrollbar_markers_for_diff_preview(&preview),
        vec![crate::view::components::ScrollbarMarker {
            start: 0.0,
            end: 1.0,
            kind: crate::view::components::ScrollbarMarkerKind::Modify,
        }]
    );
}

#[test]
fn markdown_diff_scrollbar_markers_split_disjoint_change_regions() {
    let preview = build_markdown_diff_preview(
        "- old one\n- keep two\n- keep three\n- keep four\n- old five\n",
        "- new one\n- keep two\n- keep three\n- keep four\n- new five\n",
    )
    .unwrap();

    assert_eq!(
        scrollbar_markers_for_diff_preview(&preview),
        vec![
            crate::view::components::ScrollbarMarker {
                start: 0.0,
                end: 0.2,
                kind: crate::view::components::ScrollbarMarkerKind::Modify,
            },
            crate::view::components::ScrollbarMarker {
                start: 0.8,
                end: 1.0,
                kind: crate::view::components::ScrollbarMarkerKind::Modify,
            },
        ]
    );
}

// ── Edge case: line_range_change_hint with empty mask ────────────────

#[test]
fn line_range_change_hint_with_empty_mask_is_none() {
    assert_eq!(
        line_range_change_hint(&(0..3), &[], true),
        MarkdownChangeHint::None
    );
}

#[test]
fn line_range_change_hint_with_empty_range_is_none() {
    assert_eq!(
        line_range_change_hint(&(2..2), &[true, true, true], true),
        MarkdownChangeHint::None
    );
}
