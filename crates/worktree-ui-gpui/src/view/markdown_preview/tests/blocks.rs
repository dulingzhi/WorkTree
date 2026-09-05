use super::*;

// ── Flowing document blocks ─────────────────────────────────────────

#[test]
fn blocks_group_the_lines_of_one_construct_together() {
    let doc = parse(
        "# Title\n\nA paragraph.\n\n- one\n- two\n\n```rust\nlet a = 1;\nlet b = 2;\n```\n\n> quoted\n> lines\n\n| a | b |\n| --- | --- |\n| c | d |\n\n---\n",
    );
    let blocks = markdown_document_blocks(&doc);

    let shapes: Vec<String> = blocks
        .iter()
        .map(|block| match block {
            MarkdownBlock::Heading { level, .. } => format!("h{level}"),
            MarkdownBlock::Paragraph(_) => "p".to_string(),
            MarkdownBlock::List(rows) => format!("list({})", rows.len()),
            MarkdownBlock::Blockquote(rows) => format!("quote({})", rows.len()),
            MarkdownBlock::Code(rows) => format!("code({})", rows.len()),
            MarkdownBlock::Table(rows) => format!("table({})", rows.len()),
            MarkdownBlock::Image(_) => "img".to_string(),
            MarkdownBlock::ThematicBreak(_) => "hr".to_string(),
        })
        .collect();

    // The table is header + body: the `| --- |` line is alignment
    // metadata and never becomes a row of its own.
    assert_eq!(
        shapes,
        vec![
            "h1", "p", "list(2)", "code(2)", "quote(2)", "table(2)", "hr"
        ]
    );
}

#[test]
fn spacer_rows_do_not_become_blocks() {
    // Spacers open a gap in the fixed row grid; the flowing layout uses a
    // margin instead, so carrying them through would double the gap.
    let doc = parse("Intro\n\n# Title\n\nBody\n");
    assert!(
        doc.rows
            .iter()
            .any(|row| row.kind == MarkdownPreviewRowKind::Spacer)
    );

    let blocks = markdown_document_blocks(&doc);
    assert_eq!(blocks.len(), 3, "paragraph, heading, paragraph: {blocks:?}");
}

#[test]
fn two_tables_that_touch_stay_separate() {
    // Folding them together padded both to the widest table's columns and
    // drew them as one grid.
    let doc = parse(
        "| a | b |\n| --- | --- |\n| c | d |\n\n| wiiiiiiiiiide | x |\n| --- | --- |\n| e | f |\n",
    );

    let tables: Vec<Range<usize>> = markdown_document_blocks(&doc)
        .into_iter()
        .filter_map(|block| match block {
            MarkdownBlock::Table(rows) => Some(rows),
            _ => None,
        })
        .collect();
    assert_eq!(tables.len(), 2, "rows: {:?}", row_texts(&doc));

    let width_of = |rows: &Range<usize>| doc.rows[rows.start].text.chars().count();
    assert_ne!(
        width_of(&tables[0]),
        width_of(&tables[1]),
        "each table is padded to its own columns, not the other's: {:?}",
        row_texts(&doc)
    );
}

#[test]
fn two_alerts_that_touch_stay_separate_blocks() {
    // Folding them together labelled the second alert with the first one's
    // kind and drew a single bar down both.
    let doc = parse("> [!NOTE]\n> first\n\n> [!WARNING]\n> second\n");
    let quotes: Vec<Range<usize>> = markdown_document_blocks(&doc)
        .into_iter()
        .filter_map(|block| match block {
            MarkdownBlock::Blockquote(rows) => Some(rows),
            _ => None,
        })
        .collect();

    assert_eq!(
        quotes.len(),
        2,
        "blocks: {:?}",
        markdown_document_blocks(&doc)
    );
    let kinds: Vec<Option<MarkdownAlertKind>> = quotes
        .iter()
        .map(|rows| doc.rows[rows.start].alert_kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            Some(MarkdownAlertKind::Note),
            Some(MarkdownAlertKind::Warning)
        ]
    );
    assert!(
        quotes.iter().all(|rows| doc.rows[rows.start].starts_alert),
        "each block must begin at the row that opens its alert"
    );
}

#[test]
fn an_image_block_collapses_its_bands_into_one() {
    let doc = parse("![shot](a.png)\n");
    assert_eq!(
        doc.rows.len(),
        usize::from(MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS),
        "the row model still carries one row per band"
    );

    let blocks = markdown_document_blocks(&doc);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(blocks[0], MarkdownBlock::Image(_)));
}

#[test]
fn every_row_that_paints_reaches_a_block() {
    // Nothing but spacers may be dropped, or the flowing preview would
    // silently lose content the diff preview still shows.
    let doc = parse(
        "# T\n\ntext\n\n- a\n\n```\ncode\n```\n\n> q\n\n| x |\n| --- |\n\n![i](a.png)\n\n---\n",
    );
    let painted = doc
        .rows
        .iter()
        .filter(|row| row.kind != MarkdownPreviewRowKind::Spacer)
        .count();
    let covered: usize = markdown_document_blocks(&doc)
        .iter()
        .map(|block| block.row_range().len())
        .sum();

    assert_eq!(
        covered,
        painted,
        "blocks: {:?}",
        markdown_document_blocks(&doc)
    );
}
