use super::*;

// ── Inline spans ────────────────────────────────────────────────────

#[test]
fn bold_text_produces_bold_span() {
    let doc = parse("Some **bold** text\n");
    assert_eq!(doc.rows.len(), 1);
    let bold = spans_with_style(&doc.rows[0], MarkdownInlineStyle::Bold);
    assert_eq!(bold.len(), 1);
    assert_eq!(
        &doc.rows[0].text.as_ref()[bold[0].byte_range.clone()],
        "bold"
    );
}

#[test]
fn italic_text_produces_italic_span() {
    let doc = parse("Some *italic* text\n");
    assert_eq!(
        spans_with_style(&doc.rows[0], MarkdownInlineStyle::Italic).len(),
        1
    );
}

#[test]
fn inline_code_produces_code_span() {
    let doc = parse("Use `code` here\n");
    let code = spans_with_style(&doc.rows[0], MarkdownInlineStyle::Code);
    assert_eq!(code.len(), 1);
    assert_eq!(
        &doc.rows[0].text.as_ref()[code[0].byte_range.clone()],
        "code"
    );
}

#[test]
fn strikethrough_produces_span() {
    let doc = parse("Some ~~struck~~ text\n");
    assert_eq!(
        spans_with_style(&doc.rows[0], MarkdownInlineStyle::Strikethrough).len(),
        1
    );
}

#[test]
fn link_produces_link_span() {
    let doc = parse("[click](http://example.com)\n");
    let links = spans_with_style(&doc.rows[0], MarkdownInlineStyle::Link);
    assert_eq!(links.len(), 1);
    assert_eq!(
        &doc.rows[0].text.as_ref()[links[0].byte_range.clone()],
        "click"
    );
}

#[test]
fn inline_html_links_preserve_text_and_link_span() {
    let doc = parse("Built with <a href=\"https://pages.github.com/\">GitHub Pages</a>.\n");
    assert_eq!(doc.rows.len(), 1);
    assert_eq!(doc.rows[0].text.as_ref(), "Built with GitHub Pages.");
    let links = spans_with_style(&doc.rows[0], MarkdownInlineStyle::Link);
    assert_eq!(links.len(), 1);
    assert_eq!(
        &doc.rows[0].text.as_ref()[links[0].byte_range.clone()],
        "GitHub Pages"
    );
}

#[test]
fn bold_italic_produces_bold_italic_span() {
    let doc = parse("***both***\n");
    assert_eq!(
        spans_with_style(&doc.rows[0], MarkdownInlineStyle::BoldItalic).len(),
        1
    );
}

#[test]
fn underline_html_produces_underline_span() {
    let doc = parse("This is an <ins>underlined</ins> text\n");
    assert_eq!(doc.rows[0].text.as_ref(), "This is an underlined text");
    let underline = spans_with_style(&doc.rows[0], MarkdownInlineStyle::Underline);
    assert_eq!(underline.len(), 1);
    assert_eq!(
        &doc.rows[0].text.as_ref()[underline[0].byte_range.clone()],
        "underlined"
    );
}

#[test]
fn subscript_and_superscript_tags_are_stripped_from_preview_text() {
    let doc = parse("This is a <sub>subscript</sub> and <sup>superscript</sup> text\n");
    assert_eq!(
        doc.rows[0].text.as_ref(),
        "This is a subscript and superscript text"
    );
}

#[test]
fn details_summary_renders_as_structured_preview_rows() {
    let doc = parse(
        "<details open>\n<summary>**Quick start**</summary>\n\nInstall the package.\n</details>\n",
    );

    assert_eq!(doc.rows.len(), 2);
    assert_eq!(doc.rows[0].kind, MarkdownPreviewRowKind::DetailsSummary);
    assert_eq!(doc.rows[0].text.as_ref(), "Quick start");
    let summary_bold = spans_with_style(&doc.rows[0], MarkdownInlineStyle::Bold);
    assert_eq!(summary_bold.len(), 1);
    assert_eq!(
        &doc.rows[0].text.as_ref()[summary_bold[0].byte_range.clone()],
        "Quick start"
    );

    assert_eq!(doc.rows[1].kind, MarkdownPreviewRowKind::Paragraph);
    assert_eq!(doc.rows[1].text.as_ref(), "Install the package.");
}

#[test]
fn details_summary_on_same_html_line_ignores_wrapper_tags() {
    let doc = parse(
        "<details><summary><strong>Examples</strong> and `usage`</summary>\n\nBody text.\n</details>\n",
    );

    assert_eq!(doc.rows.len(), 2);
    assert_eq!(doc.rows[0].kind, MarkdownPreviewRowKind::DetailsSummary);
    assert_eq!(doc.rows[0].text.as_ref(), "Examples and usage");
    let summary_code = spans_with_style(&doc.rows[0], MarkdownInlineStyle::Code);
    assert_eq!(summary_code.len(), 1);
    assert_eq!(
        &doc.rows[0].text.as_ref()[summary_code[0].byte_range.clone()],
        "usage"
    );
    assert_eq!(doc.rows[1].text.as_ref(), "Body text.");
}

#[test]
fn escaped_markdown_characters_remain_literal() {
    let doc = parse("Let's rename \\*our-new-project\\* to \\*our-old-project\\*.\n");
    assert_eq!(
        doc.rows[0].text.as_ref(),
        "Let's rename *our-new-project* to *our-old-project*."
    );
    assert!(doc.rows[0].inline_spans.is_empty());
}

#[test]
fn excessive_inline_spans_degrade_to_plain_text() {
    // Build a paragraph with more than MAX_INLINE_SPANS_PER_ROW inline
    // code spans so the cap fires and all styling is dropped.
    let mut src = String::new();
    for i in 0..MAX_INLINE_SPANS_PER_ROW + 10 {
        if i > 0 {
            src.push(' ');
        }
        src.push_str(&format!("`s{i}`"));
    }
    src.push('\n');

    let doc = parse(&src);
    assert_eq!(doc.rows.len(), 1);
    assert!(
        doc.rows[0].inline_spans.is_empty(),
        "expected all spans to be dropped when exceeding MAX_INLINE_SPANS_PER_ROW, got {}",
        doc.rows[0].inline_spans.len()
    );
}

#[test]
fn normalize_whitespace_with_spans_handles_multibyte_utf8() {
    // Emoji and accented characters with inline bold around a non-ASCII word.
    let doc = parse("café  **résumé**\nnext\n");
    assert_eq!(doc.rows.len(), 1);
    // Whitespace should be collapsed and span should point at the bold text.
    assert_eq!(doc.rows[0].text.as_ref(), "café résumé next");
    let bold_span = doc.rows[0]
        .inline_spans
        .iter()
        .find(|s| s.style == MarkdownInlineStyle::Bold)
        .expect("expected bold span");
    assert_eq!(
        &doc.rows[0].text.as_ref()[bold_span.byte_range.clone()],
        "résumé"
    );
}

#[test]
fn normalize_whitespace_collapses_runs() {
    let plain = |s: &str| normalize_whitespace_with_spans(s, &[]).0;
    assert_eq!(plain("a  b\tc\n d"), "a b c d");
    assert_eq!(plain("  leading"), " leading");
    assert_eq!(plain(""), "");
}

#[test]
fn normalize_whitespace_without_spans_returns_no_spans() {
    // The spans-free fast path of the merged normaliser: same text policy
    // as the remapping pass, and no spans out.
    let cases = [
        ("a  b\tc\n d", "a b c d"),
        ("", ""),
        ("  leading", " leading"),
        ("trailing  ", "trailing "),
        ("カタカナ  ひらがな", "カタカナ ひらがな"),
        ("\n\n\t\n", " "),
    ];
    for (source, expected) in cases {
        let (text, spans) = normalize_whitespace_with_spans(source, &[]);
        assert_eq!(text, expected, "source {source:?}");
        assert!(spans.is_empty(), "source {source:?}");
    }
}

// ── Links ───────────────────────────────────────────────────────────

#[test]
fn inline_links_keep_their_destination() {
    let doc = parse("See [the docs](https://example.com/docs) for details.\n");
    let row = &doc.rows[0];

    assert_eq!(row.text.as_ref(), "See the docs for details.");
    assert_eq!(
        link_spans(row),
        vec![("the docs", "https://example.com/docs")]
    );
}

#[test]
fn autolinks_and_styled_link_text_stay_clickable() {
    // A bold span inside a link resolves to Bold, but it is still part of
    // the link and must carry the destination.
    let doc = parse("<https://example.com/bare> and [**bold**](https://example.com/b)\n");
    let row = &doc.rows[0];

    let spans = link_spans(row);
    assert!(
        spans
            .iter()
            .any(|(text, url)| *text == "https://example.com/bare"
                && *url == "https://example.com/bare"),
        "autolink should be clickable: {spans:?}"
    );
    assert!(
        spans
            .iter()
            .any(|(text, url)| *text == "bold" && *url == "https://example.com/b"),
        "bold link text should be clickable: {spans:?}"
    );
}

#[test]
fn only_web_destinations_are_offered() {
    // Relative paths, anchors, and non-web schemes still render as links
    // but have nothing to open in a browser.
    let doc = parse(
        "[rel](./other.md) [anchor](#section) [mail](mailto:a@b.c) [js](javascript:alert(1)) [ok](https://example.com)\n",
    );
    let row = &doc.rows[0];

    assert_eq!(link_spans(row), vec![("ok", "https://example.com")]);
}

#[test]
fn footnote_references_are_not_web_links() {
    let doc = parse("text[^1]\n\n[^1]: note\n");
    for row in &doc.rows {
        assert!(
            link_spans(row).is_empty(),
            "footnotes point inside the document: {:?}",
            row.text
        );
    }
}

#[test]
fn link_destinations_survive_whitespace_normalisation_and_table_alignment() {
    // Both rewrite row text and remap span offsets; the URL has to ride
    // along or the remapped span becomes unclickable.
    let doc = parse("a   [spaced   link](https://example.com/x)   b\n");
    let paragraph = &doc.rows[0];
    assert_eq!(paragraph.text.as_ref(), "a spaced link b");
    assert_eq!(
        link_spans(paragraph),
        vec![("spaced link", "https://example.com/x")]
    );

    let table = parse("| a | b |\n| --- | --- |\n| [x](https://example.com/y) | wide cell |\n");
    let body = table
        .rows
        .iter()
        .find(|row| row.text.contains('x'))
        .expect("table body row");
    assert_eq!(link_spans(body), vec![("x", "https://example.com/y")]);
}

// ── Inline span integrity ───────────────────────────────────────────

#[test]
fn inline_spans_stay_on_char_boundaries_for_multibyte_markdown() {
    const FRAGMENTS: &[&str] = &[
        "plain — text",
        "**bold — run**",
        "*em — run*",
        "~~strike —~~",
        "`code — span`",
        "[link — text](https://example.com)",
        "text with é中😀 mix",
        "# Heading — one",
        "## Heading **—** two",
        "- list — item",
        "- [ ] task — item",
        "- [x] done **—** item",
        "1. ordered — item",
        "> quote — line",
        "> [!NOTE]\n> alert — body",
        "| a — | b |\n| --- | --- |\n| **c—** | d |",
        "| ——— | short |\n| --- | --- |\n| x | *y—z* |",
        "```rust\nlet x = \"—\";\n```",
        "---",
        "<b>html — bold</b>",
        "<details><summary>sum — **mary**</summary></details>",
        "<img alt=\"alt — text\" src=\"x.png\">",
        "text[^1] — ref\n\n[^1]: note — body",
        "line one —  \nline two —",
        "a—b   c—d",
        "  —indented — paragraph",
        "—",
        "**—**",
        "*—*text—",
        "| — |\n| --- |\n| **—** |",
    ];

    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    for _ in 0..20_000 {
        let count = 1 + (next() % 5) as usize;
        let mut source = String::new();
        for _ in 0..count {
            let fragment = FRAGMENTS[(next() % FRAGMENTS.len() as u64) as usize];
            source.push_str(fragment);
            source.push_str(if next() % 2 == 0 { "\n\n" } else { "\n" });
        }
        let Some(doc) = parse_markdown(&source) else {
            continue;
        };
        assert_rows_span_aligned(&source, &doc);
    }
}

#[test]
fn inline_spans_stay_on_char_boundaries_for_random_markdown_soup() {
    const ALPHABET: &[&str] = &[
        "—",
        "é",
        "中",
        "😀",
        "…",
        "\u{a0}",
        "a",
        "b",
        "x",
        " ",
        "  ",
        "\t",
        "\n",
        "\n\n",
        "#",
        "##",
        "*",
        "**",
        "_",
        "__",
        "~~",
        "`",
        "```",
        "[",
        "]",
        "(",
        ")",
        "<",
        ">",
        "|",
        "-",
        "- ",
        "1. ",
        "!",
        "\\",
        "&amp;",
        "<b>",
        "</b>",
        "<i>",
        "</i>",
        "<br>",
        "<summary>",
        "</summary>",
        "<details>",
        "</details>",
        "[^1]",
        "[^1]: ",
        "---",
        "> ",
        "[!NOTE]",
        "[ ] ",
        "[x] ",
        ":",
        "\"",
        "'",
        "/",
        "=",
        "img ",
        "alt=",
        "http://x",
    ];

    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    for _ in 0..60_000 {
        let token_count = 4 + (next() % 60) as usize;
        let mut source = String::with_capacity(token_count * 3);
        for _ in 0..token_count {
            source.push_str(ALPHABET[(next() % ALPHABET.len() as u64) as usize]);
        }
        let Some(doc) = parse_markdown(&source) else {
            continue;
        };
        assert_rows_span_aligned(&source, &doc);
    }
}
