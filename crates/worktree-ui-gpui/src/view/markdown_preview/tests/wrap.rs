use super::*;

// ── Word wrap ───────────────────────────────────────────────────────

#[test]
fn wrap_plan_keeps_one_visual_row_per_unwrapped_source_row() {
    let doc = parse("# Title\n\nParagraph one.\n\nParagraph two.\n");
    let plan = build_markdown_preview_wrap_plan(&doc, |_| Vec::new()).expect("plan fits");

    assert_eq!(plan.len(), doc.rows.len());
    for (visual_ix, row) in doc.rows.iter().enumerate() {
        let visual = plan.get(visual_ix).expect("visual row");
        assert_eq!(visual.row_ix, visual_ix);
        assert_eq!(visual.wrap_ix, 0);
        assert_eq!(visual.byte_range, 0..row.text.len());
        assert!(!visual.is_continuation());
    }
}

#[test]
fn wrap_plan_expands_split_rows_and_maps_source_rows_to_their_first_visual_row() {
    let doc = parse("First paragraph.\n\nSecond paragraph.\n");
    // Split every row with text into two halves at a char boundary.
    let plan = build_markdown_preview_wrap_plan(&doc, |row| {
        let len = row.text.len();
        if len < 4 {
            return Vec::new();
        }
        let mut mid = len / 2;
        while mid > 0 && !row.text.is_char_boundary(mid) {
            mid -= 1;
        }
        vec![0..mid, mid..len]
    })
    .expect("plan fits");

    let split_rows = doc.rows.iter().filter(|row| row.text.len() >= 4).count();
    assert_eq!(plan.len(), doc.rows.len() + split_rows);

    for row_ix in 0..doc.rows.len() {
        let visual_ix = plan.visual_ix_for_row(row_ix);
        let visual = plan.get(visual_ix).expect("first visual row");
        assert_eq!(visual.row_ix, row_ix);
        assert_eq!(visual.wrap_ix, 0);
        assert!(!visual.is_continuation());
    }

    let continuations = (0..plan.len())
        .filter_map(|ix| plan.get(ix))
        .filter(|visual| visual.is_continuation())
        .count();
    assert_eq!(continuations, split_rows);
}

#[test]
fn wrap_plan_slices_cover_the_whole_row_text() {
    let doc = parse("A paragraph with several words in it.\n");
    let plan = build_markdown_preview_wrap_plan(&doc, |row| {
        let len = row.text.len();
        if len >= 8 {
            vec![0..4, 4..len]
        } else {
            Vec::new()
        }
    })
    .expect("plan fits");

    let mut covered: Vec<(usize, Range<usize>)> = Vec::new();
    for ix in 0..plan.len() {
        let visual = plan.get(ix).expect("visual row");
        covered.push((visual.row_ix, visual.byte_range.clone()));
    }
    for (row_ix, row) in doc.rows.iter().enumerate() {
        let mut cursor = 0usize;
        for (_, range) in covered.iter().filter(|(ix, _)| *ix == row_ix) {
            assert_eq!(range.start, cursor, "slices must be contiguous");
            cursor = range.end;
        }
        assert_eq!(cursor, row.text.len(), "slices must cover the row text");
    }
}

#[test]
fn wrap_plan_reports_overflow_instead_of_truncating_the_document() {
    // Wrapping every row into many visual rows blows past the cap. The
    // builder must report that rather than hand back a plan whose tail
    // rows are missing, which would make them unreachable in the list.
    let paragraph = "w".repeat(900);
    let source = format!("{paragraph}\n\n").repeat(200);
    let doc = parse(&source);
    // One visual row per byte overshoots MAX_PREVIEW_WRAPPED_ROWS, which
    // a pane only a few pixels wide would do for real.
    let plan = build_markdown_preview_wrap_plan(&doc, |row| {
        let len = row.text.len();
        (0..len).map(|ix| ix..ix + 1).collect()
    });
    assert!(
        plan.is_none(),
        "an oversized wrapped document must fall back to unwrapped rendering"
    );
}

#[test]
fn split_wrap_plans_keep_both_columns_row_aligned() {
    let old = "# Title\n\nlong old paragraph that wraps\n\nshared tail\n";
    let new = "# Title\n\nshort\n\nshared tail\n";
    let preview = build_markdown_diff_preview(old, new).expect("diff preview should build");

    // Wrap only rows longer than 10 bytes, into two halves.
    let (old_plan, new_plan) =
        build_markdown_preview_split_wrap_plans(&preview.old, &preview.new, |row| {
            let len = row.text.len();
            if len <= 10 {
                return Vec::new();
            }
            let mut mid = len / 2;
            while mid > 0 && !row.text.is_char_boundary(mid) {
                mid -= 1;
            }
            vec![0..mid, mid..len]
        })
        .expect("split plans should fit");

    assert_eq!(
        old_plan.len(),
        new_plan.len(),
        "split columns must render the same number of visual rows"
    );
    for visual_ix in 0..old_plan.len() {
        let old_visual = old_plan.get(visual_ix).expect("old visual row");
        let new_visual = new_plan.get(visual_ix).expect("new visual row");
        assert_eq!(
            (old_visual.row_ix, old_visual.wrap_ix),
            (new_visual.row_ix, new_visual.wrap_ix),
            "visual row {visual_ix} must show the same source row on both sides"
        );
    }
}

#[test]
fn split_wrap_plans_pad_the_short_side_with_empty_continuations() {
    // The narrow column has to hold a blank row opposite each extra
    // wrapped row on the wide side, or the two lists drift apart.
    let old = "# Title\n\nlong paragraph on the old side\n";
    let new = "# Title\n\nshort\n";
    let preview = build_markdown_diff_preview(old, new).expect("diff preview should build");

    let (old_plan, new_plan) =
        build_markdown_preview_split_wrap_plans(&preview.old, &preview.new, |row| {
            let len = row.text.len();
            if len <= 10 {
                return Vec::new();
            }
            vec![0..5, 5..len]
        })
        .expect("split plans should fit");

    assert_eq!(old_plan.len(), new_plan.len());
    let padded: Vec<_> = (0..new_plan.len())
        .filter_map(|ix| new_plan.get(ix))
        .filter(|visual| visual.is_continuation())
        .collect();
    assert!(
        !padded.is_empty(),
        "the short column should gain padding rows"
    );
    for visual in padded {
        assert!(
            visual.byte_range.is_empty(),
            "a padding row paints nothing: {visual:?}"
        );
    }
}

#[test]
fn visual_row_text_slice_returns_the_painted_portion() {
    let doc = parse("first second third\n");
    let row = doc
        .rows
        .iter()
        .find(|row| row.kind == MarkdownPreviewRowKind::Paragraph)
        .expect("paragraph row");

    let visual = |wrap_ix, byte_range| MarkdownPreviewVisualRow {
        row_ix: 0,
        wrap_ix,
        byte_range,
    };

    assert_eq!(
        visual(0, 0..row.text.len()).text_slice(row).as_ref(),
        "first second third"
    );
    assert_eq!(visual(1, 6..12).text_slice(row).as_ref(), "second");
    // A padding row and an out-of-range slice both paint nothing.
    assert_eq!(
        visual(2, row.text.len()..row.text.len())
            .text_slice(row)
            .as_ref(),
        ""
    );
    assert_eq!(visual(3, 1..2).text_slice(row).as_ref(), "i");
}
