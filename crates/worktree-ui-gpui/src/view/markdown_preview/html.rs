//! Classification of the HTML the markdown preview understands.

use super::model::{MarkdownImage, MarkdownInlineStyle};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum HtmlHandling {
    Ignore,
    HardBreak,
    DetailsSummary(String),
    StartInlineStyle(MarkdownInlineStyle),
    EndInlineStyle(MarkdownInlineStyle),
    AppendText(String),
    /// The `<img>` tags a fragment holds, each with the byte offset of its tag
    /// inside that fragment and the `alt` describing it if it cannot be drawn.
    Images(Vec<(usize, MarkdownImage, String)>),
    AppendLiteral,
}

pub(super) fn classify_supported_html(html: &str) -> HtmlHandling {
    let trimmed = html.trim();
    if trimmed.is_empty() {
        return HtmlHandling::Ignore;
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("<!--") {
        return HtmlHandling::Ignore;
    }
    if let Some(summary_source) = extract_html_summary_content(trimmed) {
        return HtmlHandling::DetailsSummary(summary_source);
    }
    let images = extract_html_images(trimmed);
    if !images.is_empty() {
        return HtmlHandling::Images(images);
    }
    if let Some(alt_text) = extract_html_image_alt(trimmed) {
        return HtmlHandling::AppendText(alt_text);
    }
    if matches!(lower.as_str(), "<br>" | "<br/>" | "<br />") {
        return HtmlHandling::HardBreak;
    }
    if matches!(lower.as_str(), "<ins>") {
        return HtmlHandling::StartInlineStyle(MarkdownInlineStyle::Underline);
    }
    if matches!(lower.as_str(), "</ins>") {
        return HtmlHandling::EndInlineStyle(MarkdownInlineStyle::Underline);
    }
    if matches!(lower.as_str(), "<sub>" | "</sub>" | "<sup>" | "</sup>") {
        return HtmlHandling::Ignore;
    }
    if lower.starts_with("<a ") && (lower.contains(" name=") || lower.contains(" id=")) {
        return HtmlHandling::Ignore;
    }
    if lower.starts_with("<a ") && lower.contains(" href=") {
        return HtmlHandling::StartInlineStyle(MarkdownInlineStyle::Link);
    }
    if lower == "</a>" {
        return HtmlHandling::EndInlineStyle(MarkdownInlineStyle::Link);
    }
    if lower.starts_with("<picture")
        || lower == "</picture>"
        || lower.starts_with("<source")
        || lower == "</source>"
    {
        return HtmlHandling::Ignore;
    }
    if is_html_open_tag(lower.as_str(), "details") || is_html_close_tag(lower.as_str(), "details") {
        return HtmlHandling::Ignore;
    }

    HtmlHandling::AppendLiteral
}

fn is_html_open_tag(lower_html: &str, tag_name: &str) -> bool {
    if !lower_html.starts_with('<') || lower_html.starts_with("</") {
        return false;
    }

    let Some(rest) = lower_html.strip_prefix('<') else {
        return false;
    };
    let Some(rest) = rest.strip_prefix(tag_name) else {
        return false;
    };

    rest.is_empty()
        || rest.starts_with('>')
        || rest.starts_with('/')
        || rest.starts_with(char::is_whitespace)
}

fn is_html_close_tag(lower_html: &str, tag_name: &str) -> bool {
    let Some(rest) = lower_html.strip_prefix("</") else {
        return false;
    };
    let Some(rest) = rest.strip_prefix(tag_name) else {
        return false;
    };

    rest.is_empty() || rest.starts_with('>') || rest.starts_with(char::is_whitespace)
}

fn extract_html_summary_content(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open_ix = lower.find("<summary")?;
    let start_tag_end_rel = html[open_ix..].find('>')?;
    let content_start = open_ix + start_tag_end_rel + 1;
    let close_rel = lower[content_start..].find("</summary>")?;
    Some(html[content_start..content_start + close_rel].to_owned())
}

fn extract_html_image_alt(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let img_ix = lower.find("<img")?;
    extract_html_attribute(&html[img_ix..], "alt")
}

/// The image an `<img>` tag describes, for the tags markdown documents use in
/// place of `![alt](src)` — typically a logo sized with `width`.
/// One fragment often holds several — a row of badges is written as a single
/// block of HTML — so every tag is collected, and each is bounded to its own
/// `>` before its attributes are read so it cannot borrow the next tag's.
fn extract_html_images(html: &str) -> Vec<(usize, MarkdownImage, String)> {
    let lower = html.to_ascii_lowercase();
    let mut images = Vec::new();
    let mut search_start = 0usize;

    while let Some(offset) = lower[search_start..].find("<img") {
        let tag_start = search_start + offset;
        let tag_end = lower[tag_start..]
            .find('>')
            .map_or(html.len(), |end| tag_start + end + 1);
        search_start = tag_end;

        let tag = &html[tag_start..tag_end];
        let Some(source) = extract_html_attribute(tag, "src") else {
            continue;
        };
        if source.trim().is_empty() {
            continue;
        }
        images.push((
            tag_start,
            MarkdownImage {
                source: source.into(),
                width_px: extract_html_pixel_attribute(tag, "width"),
                height_px: extract_html_pixel_attribute(tag, "height"),
            },
            extract_html_attribute(tag, "alt").unwrap_or_default(),
        ));
    }

    images
}

/// A `width`/`height` attribute in CSS pixels.
///
/// Percentages and other units describe a size relative to something the
/// preview's fixed row grid does not have, so they are ignored and the image
/// falls back to the default block.
fn extract_html_pixel_attribute(html: &str, name: &str) -> Option<u32> {
    let value = extract_html_attribute(html, name)?;
    let value = value.trim();
    let digits = value.strip_suffix("px").unwrap_or(value).trim();
    digits.parse::<u32>().ok().filter(|px| *px > 0)
}

fn extract_html_attribute(html: &str, name: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let needle = format!("{name}=");
    let mut search_start = 0;

    while let Some(rel_ix) = lower[search_start..].find(&needle) {
        let attr_ix = search_start + rel_ix;
        if attr_ix > 0 {
            let prev = lower.as_bytes()[attr_ix - 1];
            if !prev.is_ascii_whitespace() && prev != b'<' {
                search_start = attr_ix + needle.len();
                continue;
            }
        }

        let value_start = attr_ix + needle.len();
        if value_start >= html.len() {
            return None;
        }

        let value = &html[value_start..];
        let mut chars = value.chars();
        let first = chars.next()?;
        if first == '"' || first == '\'' {
            let end_rel = value[1..].find(first)?;
            return Some(value[1..1 + end_rel].to_owned());
        }

        let end = value
            .find(|c: char| c.is_ascii_whitespace() || matches!(c, '>' | '/'))
            .unwrap_or(value.len());
        return Some(value[..end].to_owned());
    }

    None
}
