//! Inline span machinery: styles, links, fragment parsing, and the whitespace
//! normalisation that keeps spans pointed at the bytes they styled.

use super::html::{HtmlHandling, classify_supported_html};
use super::model::{MarkdownInlineSpan, MarkdownInlineStyle};
use super::parse::markdown_parser_options;
use gpui::SharedString;

/// Destination of the innermost link currently open, if it is a web URL.
pub(super) fn current_link_url(link_stack: &[Option<SharedString>]) -> Option<SharedString> {
    link_stack.last().cloned().flatten()
}

/// Keep only destinations that open in a browser.
///
/// Relative links, in-document anchors, and `mailto:`/`javascript:` targets
/// have no meaning for a preview of a file at some commit, so they render as
/// links but are not offered as something to open.
pub(super) fn web_link_url(dest_url: &str) -> Option<SharedString> {
    let trimmed = dest_url.trim();
    let scheme_end = trimmed.find("://")?;
    let scheme = &trimmed[..scheme_end];
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        .then(|| SharedString::from(trimmed.to_owned()))
}

pub(super) fn pop_matching_inline_style(
    stack: &mut Vec<MarkdownInlineStyle>,
    style: MarkdownInlineStyle,
) {
    if let Some(ix) = stack.iter().rposition(|s| *s == style) {
        stack.remove(ix);
    }
}

fn strip_generic_html_tags(fragment: &str) -> String {
    let mut stripped = String::with_capacity(fragment.len());
    let mut chars = fragment.chars().peekable();
    let mut in_tag = false;

    while let Some(ch) = chars.next() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
            }
            continue;
        }

        if ch == '<'
            && chars
                .peek()
                .is_some_and(|next| next.is_ascii_alphabetic() || matches!(next, '/' | '!' | '?'))
        {
            in_tag = true;
            continue;
        }

        stripped.push(ch);
    }

    stripped
}

pub(super) fn parse_inline_markdown_fragment(source: &str) -> (String, Vec<MarkdownInlineSpan>) {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};

    let mut text_buf = String::new();
    let mut span_stack = Vec::new();
    let mut link_stack: Vec<Option<SharedString>> = Vec::new();
    let mut inline_spans = Vec::new();

    for event in Parser::new_ext(source, markdown_parser_options()) {
        match event {
            Event::Start(Tag::Strong) => span_stack.push(MarkdownInlineStyle::Bold),
            Event::Start(Tag::Emphasis) => span_stack.push(MarkdownInlineStyle::Italic),
            Event::Start(Tag::Strikethrough) => {
                span_stack.push(MarkdownInlineStyle::Strikethrough);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                span_stack.push(MarkdownInlineStyle::Link);
                link_stack.push(web_link_url(dest_url.as_ref()));
            }
            Event::End(TagEnd::Link) => {
                span_stack.pop();
                link_stack.pop();
            }
            Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough) => {
                span_stack.pop();
            }
            Event::Text(cow) => {
                let style = resolve_style_stack(&span_stack);
                let link_url = current_link_url(&link_stack);
                let start = text_buf.len();
                text_buf.push_str(&cow);
                let end = text_buf.len();
                if style != MarkdownInlineStyle::Normal || link_url.is_some() {
                    inline_spans.push(MarkdownInlineSpan {
                        byte_range: start..end,
                        style,
                        link_url,
                    });
                }
            }
            Event::Code(cow) => {
                let start = text_buf.len();
                text_buf.push_str(&cow);
                let end = text_buf.len();
                inline_spans.push(MarkdownInlineSpan {
                    byte_range: start..end,
                    style: MarkdownInlineStyle::Code,
                    link_url: current_link_url(&link_stack),
                });
            }
            Event::FootnoteReference(label) => {
                let start = text_buf.len();
                text_buf.push('[');
                text_buf.push_str(&label);
                text_buf.push(']');
                let end = text_buf.len();
                inline_spans.push(MarkdownInlineSpan {
                    byte_range: start..end,
                    style: MarkdownInlineStyle::Link,
                    link_url: None,
                });
            }
            Event::SoftBreak | Event::HardBreak if !text_buf.is_empty() => {
                text_buf.push(' ');
            }
            Event::Html(cow) | Event::InlineHtml(cow) => {
                match classify_supported_html(cow.as_ref()) {
                    HtmlHandling::Ignore => {}
                    HtmlHandling::HardBreak => {
                        if !text_buf.is_empty() {
                            text_buf.push(' ');
                        }
                    }
                    HtmlHandling::DetailsSummary(summary_source) => {
                        let summary_text = strip_generic_html_tags(&summary_source);
                        if !summary_text.is_empty() {
                            if !text_buf.is_empty() {
                                text_buf.push(' ');
                            }
                            text_buf.push_str(&summary_text);
                        }
                    }
                    HtmlHandling::StartInlineStyle(style) => span_stack.push(style),
                    HtmlHandling::EndInlineStyle(style) => {
                        pop_matching_inline_style(&mut span_stack, style);
                    }
                    // An inline fragment (a `<summary>` label) has nowhere to
                    // put a block, so an image there keeps describing itself.
                    HtmlHandling::Images(images) => {
                        for (_, _, alt) in images {
                            text_buf.push_str(&alt);
                        }
                    }
                    HtmlHandling::AppendText(text) => {
                        text_buf.push_str(&text);
                    }
                    HtmlHandling::AppendLiteral => {
                        text_buf.push_str(&strip_generic_html_tags(cow.as_ref()));
                    }
                }
            }
            _ => {}
        }
    }

    normalize_whitespace_with_spans(&text_buf, &inline_spans)
}

pub(super) fn normalize_whitespace(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                result.push(' ');
            }
            prev_ws = true;
        } else {
            result.push(ch);
            prev_ws = false;
        }
    }
    result
}

pub(super) fn normalize_whitespace_with_spans(
    text: &str,
    inline_spans: &[MarkdownInlineSpan],
) -> (String, Vec<MarkdownInlineSpan>) {
    if inline_spans.is_empty() {
        return (normalize_whitespace(text), Vec::new());
    }

    let mut normalized = String::with_capacity(text.len());
    let mut byte_map = vec![0usize; text.len() + 1];
    let mut prev_ws = false;
    let mut normalized_len = 0usize;

    for (byte_ix, ch) in text.char_indices() {
        byte_map[byte_ix] = normalized_len;
        if ch.is_whitespace() {
            if !prev_ws {
                normalized.push(' ');
                normalized_len += 1;
            }
            prev_ws = true;
        } else {
            normalized.push(ch);
            normalized_len += ch.len_utf8();
            prev_ws = false;
        }
        byte_map[byte_ix + ch.len_utf8()] = normalized_len;
    }

    let remapped_spans = inline_spans
        .iter()
        .filter_map(|span| {
            debug_assert!(text.is_char_boundary(span.byte_range.start));
            debug_assert!(text.is_char_boundary(span.byte_range.end));
            let start = *byte_map.get(span.byte_range.start)?;
            let end = *byte_map.get(span.byte_range.end)?;
            (start < end).then(|| span.restyled(start..end))
        })
        .collect();

    (normalized, remapped_spans)
}

/// Combine the inline style stack into a single effective style.
pub(super) fn resolve_style_stack(stack: &[MarkdownInlineStyle]) -> MarkdownInlineStyle {
    let mut has_bold = false;
    let mut has_italic = false;
    let mut has_strikethrough = false;
    let mut has_link = false;
    let mut has_code = false;
    let mut has_underline = false;

    for &s in stack {
        match s {
            MarkdownInlineStyle::Bold => has_bold = true,
            MarkdownInlineStyle::Italic => has_italic = true,
            MarkdownInlineStyle::Strikethrough => has_strikethrough = true,
            MarkdownInlineStyle::Link => has_link = true,
            MarkdownInlineStyle::Code => has_code = true,
            MarkdownInlineStyle::Underline => has_underline = true,
            _ => {}
        }
    }

    if has_code {
        MarkdownInlineStyle::Code
    } else if has_bold && has_italic {
        MarkdownInlineStyle::BoldItalic
    } else if has_bold {
        MarkdownInlineStyle::Bold
    } else if has_italic {
        MarkdownInlineStyle::Italic
    } else if has_strikethrough {
        MarkdownInlineStyle::Strikethrough
    } else if has_link {
        MarkdownInlineStyle::Link
    } else if has_underline {
        MarkdownInlineStyle::Underline
    } else {
        MarkdownInlineStyle::Normal
    }
}
