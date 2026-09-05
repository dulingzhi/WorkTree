use super::*;

#[test]
fn markdown_images_preserve_alt_text() {
    // A remote image is still laid out as a block; it just cannot be
    // fetched, so every band keeps the alt text to describe itself.
    let doc = parse("![Octocat smiling](https://example.com/octocat.svg)\n");
    assert_eq!(
        doc.rows.len(),
        usize::from(MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS)
    );
    assert!(doc.rows.iter().all(|row| row.kind.is_image()));
    assert!(
        doc.rows
            .iter()
            .all(|row| row.text.as_ref() == "Octocat smiling")
    );
    assert_eq!(
        doc.rows[0]
            .image
            .as_ref()
            .map(|image| image.source.as_ref()),
        Some("https://example.com/octocat.svg")
    );
}

#[test]
fn html_img_tags_preserve_alt_text() {
    let doc = parse("<img alt=\"Octocat smiling\" src=\"https://example.com/octocat.svg\" />\n");
    assert_eq!(
        doc.rows.len(),
        usize::from(MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS)
    );
    assert!(doc.rows.iter().all(|row| row.kind.is_image()));
    assert!(
        doc.rows
            .iter()
            .all(|row| row.text.as_ref() == "Octocat smiling")
    );
}

#[test]
fn html_img_tags_without_a_source_still_fall_back_to_alt_text() {
    let doc = parse("<img alt=\"Octocat smiling\" />\n");
    assert_eq!(doc.rows.len(), 1);
    assert_eq!(doc.rows[0].kind, MarkdownPreviewRowKind::Paragraph);
    assert_eq!(doc.rows[0].text.as_ref(), "Octocat smiling");
}

#[test]
fn picture_elements_render_their_nested_img() {
    // A `<picture>` carries themed `<source>` alternatives around a plain
    // `<img>` fallback; the fallback is the one to draw, and its `src` must
    // not be confused with a `<source srcset=…>` beside it.
    let doc = parse(
        "<picture>\n  <source media=\"(prefers-color-scheme: dark)\" srcset=\"dark.svg\" />\n  <img alt=\"Octocat smiling\" src=\"light.svg\" />\n</picture>\n",
    );

    let images = image_rows(&doc);
    assert_eq!(images.len(), usize::from(MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS));
    assert_eq!(
        images[0].image.as_ref().map(|image| image.source.as_ref()),
        Some("light.svg")
    );
    assert_eq!(images[0].text.as_ref(), "Octocat smiling");
}

#[test]
fn adjacent_images_share_one_line() {
    // Two pictures written on consecutive lines are one paragraph, so they
    // belong on one line — the shape a row of badges takes.
    let doc = parse("![one](a.png)\n![two](b.png)\n");

    let sources: Vec<&str> = doc
        .rows
        .iter()
        .flat_map(|row| row.inline_images.iter())
        .map(|inline| inline.image.source.as_ref())
        .collect();
    assert_eq!(sources, vec!["a.png", "b.png"]);
    assert!(
        image_rows(&doc).is_empty(),
        "neither picture is alone on its line, so neither becomes a block"
    );
}

#[test]
fn a_list_item_holding_only_a_picture_keeps_it_on_that_item() {
    // A tight list item emits no paragraph, so the item closes with an
    // empty text buffer. Skipping the row there carried the badge onto the
    // next item, or dropped it when the list ended the document.
    let doc = parse("- ![one](a.png)\n- second item\n");

    let items: Vec<(&str, Vec<&str>)> = doc
        .rows
        .iter()
        .filter(|row| matches!(row.kind, MarkdownPreviewRowKind::ListItem { .. }))
        .map(|row| {
            (
                row.text.as_ref(),
                row.inline_images
                    .iter()
                    .map(|inline| inline.image.source.as_ref())
                    .collect(),
            )
        })
        .collect();

    assert_eq!(
        items,
        vec![("", vec!["a.png"]), ("second item", vec![])],
        "rows: {:?}",
        row_texts(&doc)
    );
}

#[test]
fn a_picture_in_the_last_list_item_is_not_dropped() {
    let doc = parse("- text item\n- ![only](b.png)\n");

    let sources: Vec<&str> = doc
        .rows
        .iter()
        .flat_map(|row| row.inline_images.iter())
        .map(|inline| inline.image.source.as_ref())
        .collect();
    assert_eq!(sources, vec!["b.png"], "rows: {:?}", row_texts(&doc));
}

#[test]
fn a_picture_before_a_nested_list_stays_with_its_parent_item() {
    let doc = parse("- ![parent](a.png)\n  - nested\n");

    let parent = doc
        .rows
        .iter()
        .find(|row| !row.inline_images.is_empty())
        .expect("the parent item keeps its picture");
    assert_eq!(parent.image, None);
    assert_eq!(parent.indent_level, 1, "rows: {:?}", row_texts(&doc));
    assert!(
        doc.rows
            .iter()
            .any(|row| row.text.as_ref() == "nested" && row.indent_level == 2),
        "the nested item is still its own row: {:?}",
        row_texts(&doc)
    );
}

#[test]
fn a_picture_in_a_table_cell_stays_in_its_column_as_text() {
    // A table row paints as one string whose columns are aligned by
    // padding, so a picture recorded against the row would draw at its
    // leading or trailing edge — out of its column.
    let doc = parse("| ![icon](i.png) | Enabled |\n| --- | --- |\n| b | c |\n");

    let header = doc
        .rows
        .iter()
        .find(|row| {
            matches!(
                row.kind,
                MarkdownPreviewRowKind::TableRow { is_header: true }
            )
        })
        .expect("the header row survives");
    assert!(
        header.text.contains("icon"),
        "the picture's description holds its cell: {:?}",
        header.text
    );
    assert!(
        doc.rows.iter().all(|row| row.inline_images.is_empty()),
        "no picture escapes the table to render beside it"
    );
    assert!(
        image_rows(&doc).is_empty(),
        "and none becomes a block either"
    );
}

#[test]
fn an_html_picture_in_a_table_cell_also_stays_in_its_column() {
    // The `<img>` producer records pictures separately from the markdown
    // one, so it needs the same guard.
    let doc = parse("| <img alt=\"icon\" src=\"i.png\" /> | Enabled |\n| --- | --- |\n| b | c |\n");

    let header = doc
        .rows
        .iter()
        .find(|row| {
            matches!(
                row.kind,
                MarkdownPreviewRowKind::TableRow { is_header: true }
            )
        })
        .expect("the header row survives");
    assert!(
        header.text.contains("icon"),
        "the tag's description holds its cell: {:?}",
        header.text
    );
    assert!(
        doc.rows.iter().all(|row| row.inline_images.is_empty()),
        "no picture escapes the table to render beside it"
    );
}

#[test]
fn a_picture_alone_on_its_line_becomes_a_block() {
    let doc = parse("![only](a.png)\n");

    assert_eq!(
        image_rows(&doc).len(),
        usize::from(MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS),
        "rows: {:?}",
        row_texts(&doc)
    );
    assert!(
        doc.rows.iter().all(|row| row.inline_images.is_empty()),
        "a block picture is not also inline"
    );
}

// ── Images ──────────────────────────────────────────────────────────

#[test]
fn an_image_becomes_a_block_of_rows_carrying_its_source_and_alt() {
    let doc = parse("![A screenshot](docs/shot.png)\n");
    let rows = image_rows(&doc);

    assert_eq!(rows.len(), usize::from(MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS));
    for (expected_ix, row) in rows.iter().enumerate() {
        assert_eq!(
            row.kind,
            MarkdownPreviewRowKind::Image {
                slice_ix: expected_ix as u8,
                slice_count: MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS,
            }
        );
        assert_eq!(
            row.image.as_ref().map(|image| image.source.as_ref()),
            Some("docs/shot.png")
        );
        // The alt stays as row text so copy and the unavailable-image
        // fallback have something to show.
        assert_eq!(row.text.as_ref(), "A screenshot");
    }
}

#[test]
fn a_picture_written_mid_sentence_stays_on_the_sentence_row() {
    let doc = parse("Before ![shot](a.png) after\n");
    let kinds = row_kinds(&doc);

    assert_eq!(kinds.first(), Some(&&MarkdownPreviewRowKind::Paragraph));
    assert_eq!(
        doc.rows[0].text.as_ref(),
        "Before after",
        "the sentence stays one row: {:?}",
        row_texts(&doc)
    );
    assert!(
        image_rows(&doc).is_empty(),
        "a picture sharing its line with text is not a block"
    );

    let inline = &doc.rows[0].inline_images;
    assert_eq!(inline.len(), 1);
    assert_eq!(inline[0].image.source.as_ref(), "a.png");
    assert_eq!(inline[0].alt.as_ref(), "shot");
    assert_eq!(
        inline[0].byte_offset,
        "Before ".len(),
        "the picture records where in the text it was written"
    );
}

#[test]
fn image_alt_text_is_not_also_rendered_as_paragraph_text() {
    let doc = parse("![only alt](a.png)\n");
    assert!(
        !doc.rows
            .iter()
            .any(|row| !row.kind.is_image() && row.text.contains("only alt")),
        "alt text belongs to the image block only: {:?}",
        row_texts(&doc)
    );
}

#[test]
fn a_logo_in_a_heading_shares_the_heading_line() {
    // A logo in a heading, written as HTML because markdown cannot size an
    // image — the shape WorkTree's own README uses. Putting it on a line of
    // its own left the heading text stranded underneath.
    let doc = parse(
        "## <img alt=\"WorkTree logo\" src=\"assets/worktree_logo.svg\" width=\"26\" /> WorkTree\n",
    );

    let heading = doc
        .rows
        .iter()
        .find(|row| matches!(row.kind, MarkdownPreviewRowKind::Heading { .. }))
        .expect("the heading survives");
    assert_eq!(heading.text.as_ref(), "WorkTree");
    assert_eq!(heading.inline_images.len(), 1);

    let inline = &heading.inline_images[0];
    assert_eq!(inline.image.source.as_ref(), "assets/worktree_logo.svg");
    assert_eq!(inline.alt.as_ref(), "WorkTree logo");
    assert_eq!(
        inline.byte_offset, 0,
        "the logo is written before the heading text"
    );
    // The tag declares `width="26"`, which is the size it is drawn at.
    assert_eq!(inline.image.width_px, Some(26));
    assert_eq!(inline.image.height_px, None);
    assert!(
        image_rows(&doc).is_empty(),
        "a logo beside a heading is not a block of its own"
    );
}

#[test]
fn a_block_html_image_stands_on_its_own_line() {
    // `<img>` at the top level has no paragraph around it, so it has to
    // close its own row — and being alone there, it is a block.
    let doc = parse("<img alt=\"demo\" src=\"assets/demo.gif\" />\n");

    let images = image_rows(&doc);
    assert_eq!(
        images.len(),
        usize::from(MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS),
        "rows: {:?}",
        row_texts(&doc)
    );
    assert_eq!(
        images[0].image.as_ref().map(|image| image.source.as_ref()),
        Some("assets/demo.gif")
    );
    assert_eq!(images[0].text.as_ref(), "demo");
}

#[test]
fn image_block_rows_follow_the_declared_size() {
    let sized = |width_px, height_px| {
        MarkdownImage {
            source: "a.png".into(),
            width_px,
            height_px,
        }
        .block_rows()
    };

    // Undeclared falls back to the default block.
    assert_eq!(sized(None, None), MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS);
    // A declared height is authoritative, even against a wide width.
    assert_eq!(sized(Some(400), Some(20)), 1);
    assert_eq!(sized(Some(400), Some(60)), 3);
    // Width alone bounds the height, so a small logo stays small.
    assert_eq!(sized(Some(26), None), 1);
    assert_eq!(sized(Some(28), None), 1);
    assert_eq!(sized(Some(29), None), 2);
    // Anything large is capped at the default block.
    assert_eq!(sized(Some(4000), None), MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS);
    // A zero or unparseable size is treated as undeclared.
    assert_eq!(sized(Some(0), None), MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS);
}

#[test]
fn non_pixel_size_attributes_are_ignored() {
    // A percentage is relative to a container the fixed row grid does not
    // have, so it falls back to the default block rather than guessing.
    let doc = parse("<img alt=\"wide\" src=\"a.png\" width=\"100%\" />\n");
    let images = image_rows(&doc);
    assert_eq!(images.len(), usize::from(MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS));
    assert_eq!(images[0].image.as_ref().expect("image").width_px, None);

    // An explicit `px` suffix is accepted.
    let doc = parse("<img alt=\"logo\" src=\"a.png\" width=\"26px\" />\n");
    assert_eq!(
        image_rows(&doc)[0].image.as_ref().expect("image").width_px,
        Some(26)
    );
}

#[test]
fn linked_badge_images_keep_both_the_picture_and_the_link() {
    // `[![alt](badge)](target)` — the standard badge shape. The image is a
    // block; the link it sits in is still recorded.
    let doc = parse(
        "[![Build Status](https://github.com/o/r/badge.svg?branch=main)](https://github.com/o/r/actions)\n",
    );

    let images = image_rows(&doc);
    assert_eq!(images.len(), usize::from(MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS));
    assert_eq!(
        images[0].image.as_ref().map(|image| image.source.as_ref()),
        Some("https://github.com/o/r/badge.svg?branch=main")
    );
    assert_eq!(images[0].text.as_ref(), "Build Status");
}

#[test]
fn a_row_of_badges_stays_on_one_line_and_keeps_its_links() {
    // Badges are written one per source line but form a single paragraph,
    // and each is a picture wrapped in a link.
    let doc = parse(
        "[![One](https://img.shields.io/badge/one.svg)](https://a.example)\n[![Two](https://img.shields.io/badge/two.svg)](https://b.example)\n",
    );

    let badges: Vec<&MarkdownInlineImage> = doc
        .rows
        .iter()
        .flat_map(|row| row.inline_images.iter())
        .collect();

    assert_eq!(
        badges
            .iter()
            .map(|badge| badge.image.source.as_ref())
            .collect::<Vec<_>>(),
        vec![
            "https://img.shields.io/badge/one.svg",
            "https://img.shields.io/badge/two.svg"
        ]
    );
    assert_eq!(
        badges
            .iter()
            .map(|badge| badge.link_url.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("https://a.example"), Some("https://b.example")],
        "clicking a badge has to reach the link it stands for"
    );
    assert_eq!(
        doc.rows
            .iter()
            .filter(|row| !row.inline_images.is_empty())
            .count(),
        1,
        "both badges belong to the same line: {:?}",
        row_texts(&doc)
    );
}
