//! Inline and flow images: source resolution, placeholders, skeletons and layout.

use super::metrics::{
    MARKDOWN_PREVIEW_BASE_FONT_PX, MARKDOWN_PREVIEW_INLINE_IMAGE_LOADING_WIDTH_PX,
    MARKDOWN_PREVIEW_INLINE_IMAGE_MAX_HEIGHT_PX, markdown_preview_row_height,
    markdown_preview_scaled_px,
};
use super::render::MarkdownPreviewRenderContext;
use super::{
    AnyElement, AppTheme, Arc, FxHashMap, MarkdownInlineImage, MarkdownPreviewRow,
    MarkdownPreviewRowKind, Pixels, SharedString, components, div, px,
};
use gpui::InteractiveElement;
use gpui::IntoElement;
use gpui::ParentElement;
use gpui::Styled;
use gpui::StyledImage;

/// Pixel sizes read from picture headers, keyed by the source the document
/// wrote. Empty for anything that could not be measured without decoding.
pub(in crate::view) type MarkdownPreviewPictureSizes = Arc<FxHashMap<SharedString, (u32, u32)>>;

/// Shared stand-in for a preview that measured nothing. The diff preview draws
/// its pictures into fixed-height bands, so it has no use for their real sizes.
pub(in crate::view::rows) fn markdown_preview_no_picture_sizes()
-> &'static MarkdownPreviewPictureSizes {
    static EMPTY: std::sync::OnceLock<MarkdownPreviewPictureSizes> = std::sync::OnceLock::new();
    EMPTY.get_or_init(Default::default)
}

/// Where a markdown image source resolves to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) enum MarkdownPreviewImageSource {
    /// A file inside the previewed document's own directory tree.
    File(std::path::PathBuf),
    /// An `http(s)` URL, fetched and cached by `gpui`'s image loader.
    Remote(SharedString),
}

impl MarkdownPreviewImageSource {
    /// The key `gpui` stores this picture's decoded frames under.
    ///
    /// Everything that wants to know whether a picture is ready — the element
    /// that draws it and the pane waiting to be told it finished decoding —
    /// has to name it the same way, or they would be asking about two
    /// different entries in the asset cache.
    pub(in crate::view) fn to_resource(&self) -> gpui::Resource {
        match self {
            Self::File(path) => gpui::Resource::from(path.clone()),
            Self::Remote(url) => gpui::Resource::Uri(gpui::SharedUri::from(url.to_string())),
        }
    }
}

/// Resolve a markdown image source to something the preview can draw.
///
/// A local path must stay inside the previewed document's own directory tree,
/// so document content cannot aim the preview at arbitrary files on disk.
/// Anything else — `data:` payloads, other schemes, paths that climb out of
/// the tree — resolves to nothing and falls back to the alt text.
pub(in crate::view) fn markdown_preview_image_source(
    base_dir: Option<&std::path::Path>,
    source: &str,
) -> Option<MarkdownPreviewImageSource> {
    let source = source.trim();
    if source.is_empty() {
        return None;
    }
    if let Some(remote) = markdown_preview_remote_image_url(source) {
        return Some(MarkdownPreviewImageSource::Remote(remote));
    }
    if source.contains("://") || source.starts_with("data:") {
        return None;
    }

    // Query and fragment suffixes are common on image sources and are not part
    // of the file name.
    let path = source.split(['#', '?']).next().unwrap_or(source);
    let relative = std::path::Path::new(path);
    if relative.is_absolute() {
        return None;
    }
    let mut resolved = base_dir?.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(part) => resolved.push(part),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }

    resolved
        .is_file()
        .then_some(MarkdownPreviewImageSource::File(resolved))
}

/// The `http(s)` URL an image source names, if it names one.
///
/// Only these two schemes are followed; anything else a document might carry
/// (`file:`, `javascript:`, and so on) is not something a preview should
/// dereference.
fn markdown_preview_remote_image_url(source: &str) -> Option<SharedString> {
    let scheme_end = source.find("://")?;
    let scheme = &source[..scheme_end];
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        .then(|| SharedString::from(source.to_owned()))
}

/// An image sized the way the document asked, for the flowing renderer.
///
/// Unlike the diff preview's banded block, this is one element that keeps its
/// aspect ratio and never reserves rows it does not need.
pub(in crate::view) fn markdown_preview_flow_image(
    row: &MarkdownPreviewRow,
    row_ix: usize,
    theme: AppTheme,
    ui_scale_percent: u32,
    image_base_dir: Option<&std::path::Path>,
    picture_sizes: &MarkdownPreviewPictureSizes,
) -> AnyElement {
    let label_color = theme.colors.foreground.secondary;
    let font_size = markdown_preview_scaled_px(MARKDOWN_PREVIEW_BASE_FONT_PX, ui_scale_percent);

    let picture = row.image.as_ref().and_then(|image| {
        markdown_preview_resolved_picture(
            image.source.as_ref(),
            ("markdown_preview_block_image", row_ix).into(),
            image_base_dir,
        )
    });
    let Some(image) = picture else {
        return markdown_preview_image_placeholder_element(
            markdown_preview_image_label(
                row,
                crate::i18n::tr_str("misc.markdown.image_unavailable"),
            ),
            font_size,
            label_color,
        )
        .into_any_element();
    };

    let declared = row.image.as_ref().and_then(|image| image.width_px);
    let failed_label =
        markdown_preview_image_label(row, crate::i18n::tr_str("misc.markdown.failed_to_load"));
    let skeleton = markdown_preview_picture_skeleton(row, ui_scale_percent, picture_sizes);
    let image = match declared {
        Some(width) => image.w(markdown_preview_scaled_px(width as f32, ui_scale_percent)),
        // Without a declared size the picture keeps its own, up to the width
        // of the document.
        None => image.max_w_full(),
    };

    div()
        .w_full()
        .min_w(px(0.0))
        .child(
            image
                .debug_selector(move || format!("markdown_preview_block_image_{row_ix}"))
                .with_fallback(move || {
                    markdown_preview_image_placeholder_element(
                        failed_label.clone(),
                        font_size,
                        label_color,
                    )
                    .into_any_element()
                })
                .with_loading(move || skeleton.render(theme)),
        )
        .into_any_element()
}

/// The box a picture will occupy, worked out before it has been decoded.
///
/// `gpui` reads every frame of an animated picture before it reports a size, so
/// a block that waited for that would leave a hole in the document and then
/// shove everything down when the picture arrived. What the document declared
/// comes first; the picture's own header fills in the rest.
#[derive(Clone, Copy)]
pub(in crate::view::rows) struct MarkdownPreviewPictureSkeleton {
    /// Widest the picture will draw, or `None` to fill the document.
    pub(in crate::view::rows) width: Option<Pixels>,
    /// Width over height, or `None` when only a height is known.
    pub(in crate::view::rows) aspect_ratio: Option<f32>,
    /// Used when the aspect ratio is unknown: the rows the parser set aside.
    pub(in crate::view::rows) reserved_height: Pixels,
}

impl MarkdownPreviewPictureSkeleton {
    fn render(self, theme: AppTheme) -> AnyElement {
        let mut block = components::skeleton(theme)
            .debug_selector(|| "markdown_preview_picture_skeleton".to_string());
        block = match self.width {
            Some(width) => block.w(width).max_w_full(),
            None => block.w_full(),
        };
        block = match self.aspect_ratio {
            Some(ratio) => block.aspect_ratio(ratio),
            None => block.h(self.reserved_height),
        };
        block.into_any_element()
    }
}

pub(in crate::view::rows) fn markdown_preview_picture_skeleton(
    row: &MarkdownPreviewRow,
    ui_scale_percent: u32,
    picture_sizes: &MarkdownPreviewPictureSizes,
) -> MarkdownPreviewPictureSkeleton {
    let image = row.image.as_ref();
    let declared_width = image.and_then(|image| image.width_px).filter(|w| *w > 0);
    let declared_height = image.and_then(|image| image.height_px).filter(|h| *h > 0);
    // A declared size is in design pixels and scales with the UI; a size read
    // from the file is in the picture's own pixels, which is what `gpui` lays
    // an undeclared picture out at.
    let measured = image
        .and_then(|image| picture_sizes.get(&image.source))
        .copied();

    let width = match (declared_width, measured) {
        (Some(width), _) => Some(markdown_preview_scaled_px(width as f32, ui_scale_percent)),
        (None, Some((width, _))) => Some(px(width as f32)),
        (None, None) => None,
    };
    let aspect_ratio = match (declared_width, declared_height, measured) {
        (Some(width), Some(height), _) => Some(width as f32 / height as f32),
        (_, _, Some((width, height))) => Some(width as f32 / height as f32),
        _ => None,
    };

    MarkdownPreviewPictureSkeleton {
        width,
        aspect_ratio,
        reserved_height: markdown_preview_row_height(ui_scale_percent)
            * f32::from(markdown_preview_image_block_rows(row).max(1)),
    }
}

/// Rows an image block was given, which is the height it reserved.
fn markdown_preview_image_block_rows(row: &MarkdownPreviewRow) -> u8 {
    match row.kind {
        MarkdownPreviewRowKind::Image { slice_count, .. } => slice_count,
        _ => 1,
    }
}

/// One picture drawn on the same line as the text around it.
///
/// Badges, shields, and a logo beside a heading are all written inline, so they
/// are sized to the line rather than to the document: a declared width wins,
/// and anything else keeps its own size up to the inline height cap.
pub(in crate::view) fn markdown_preview_inline_image(
    inline: &MarkdownInlineImage,
    theme: AppTheme,
    ui_scale_percent: u32,
    image_base_dir: Option<&std::path::Path>,
    picture_sizes: &MarkdownPreviewPictureSizes,
) -> AnyElement {
    let source_byte = inline.source_byte;
    let label_color = theme.colors.foreground.secondary;
    let font_size = markdown_preview_scaled_px(MARKDOWN_PREVIEW_BASE_FONT_PX, ui_scale_percent);
    let described = if inline.alt.is_empty() {
        inline.image.source.clone()
    } else {
        inline.alt.clone()
    };
    let measured_aspect_ratio = picture_sizes
        .get(&inline.image.source)
        .filter(|(width, height)| *width > 0 && *height > 0)
        .map(|(width, height)| *width as f32 / *height as f32);

    let picture = markdown_preview_resolved_picture(
        inline.image.source.as_ref(),
        ("markdown_preview_inline_image", source_byte).into(),
        image_base_dir,
    );
    let Some(image) = picture else {
        return markdown_preview_inline_image_placeholder(
            markdown_preview_image_reason(
                crate::i18n::tr_str("misc.markdown.image_unavailable"),
                &described,
            ),
            source_byte,
            font_size,
            label_color,
        );
    };

    let failed_label = markdown_preview_image_reason(
        crate::i18n::tr_str("misc.markdown.failed_to_load"),
        &described,
    );
    let image =
        image.debug_selector(move || format!("markdown_preview_inline_image_{source_byte}"));
    let image = match inline.image.width_px {
        Some(width) => image.w(markdown_preview_scaled_px(width as f32, ui_scale_percent)),
        None => image.max_h(markdown_preview_scaled_px(
            MARKDOWN_PREVIEW_INLINE_IMAGE_MAX_HEIGHT_PX,
            ui_scale_percent,
        )),
    }
    // The height cap leaves a wide, short banner unbounded, and a declared
    // width can be larger than the pane; either would push the document into
    // horizontal overflow.
    .max_w_full();

    // A badge that has not arrived yet still holds its slot, so the line it
    // shares does not reflow the moment it does. Inline pictures are sized to
    // the line rather than to their own pixels, so the cap is the height and a
    // measured picture only decides how wide the slot is.
    let loading_height = markdown_preview_scaled_px(
        MARKDOWN_PREVIEW_INLINE_IMAGE_MAX_HEIGHT_PX,
        ui_scale_percent,
    );
    let loading_width = match (inline.image.width_px, measured_aspect_ratio) {
        (Some(width), _) => markdown_preview_scaled_px(width as f32, ui_scale_percent),
        (None, Some(ratio)) => loading_height * ratio,
        (None, None) => markdown_preview_scaled_px(
            MARKDOWN_PREVIEW_INLINE_IMAGE_LOADING_WIDTH_PX,
            ui_scale_percent,
        ),
    };

    image
        .with_fallback(move || {
            markdown_preview_inline_image_placeholder(
                failed_label.clone(),
                source_byte,
                font_size,
                label_color,
            )
        })
        .with_loading(move || {
            components::skeleton(theme)
                .debug_selector(move || format!("markdown_preview_inline_image_{source_byte}"))
                .flex_none()
                .w(loading_width)
                .h(loading_height)
                .max_w_full()
                .into_any_element()
        })
        .into_any_element()
}

/// A picture element that keeps per-frame state.
///
/// The id matters: `gpui` only remembers which frame an animated image is
/// showing for elements that have one, so an `img` without an id freezes on the
/// first frame of a GIF.
fn markdown_preview_image_element(
    source: MarkdownPreviewImageSource,
    id: gpui::ElementId,
) -> gpui::Stateful<gpui::Img> {
    gpui::img(gpui::ImageSource::Resource(source.to_resource())).id(id)
}

/// Stand-in for a picture that cannot be drawn.
///
/// It carries the picture's selector too: the slot has to hold its place
/// whether or not the source loaded, and a test asking whether the picture was
/// drawn is really asking whether that slot exists.
fn markdown_preview_inline_image_placeholder(
    label: SharedString,
    source_byte: usize,
    font_size: Pixels,
    color: gpui::Rgba,
) -> AnyElement {
    div()
        .debug_selector(move || format!("markdown_preview_inline_image_{source_byte}"))
        .flex_none()
        .text_size(font_size)
        .text_color(color)
        .child(label)
        .into_any_element()
}

/// Label for a picture that is not on screen: the reason, plus the alt text or
/// the source so the reader can tell which image is missing.
fn markdown_preview_image_label(row: &MarkdownPreviewRow, reason: &str) -> SharedString {
    let described = if row.text.is_empty() {
        row.image
            .as_ref()
            .map(|image| image.source.clone())
            .unwrap_or_default()
    } else {
        row.text.clone()
    };
    markdown_preview_image_reason(reason, &described)
}

/// "reason: what the picture was", or just the reason when nothing describes it.
fn markdown_preview_image_reason(reason: &str, described: &SharedString) -> SharedString {
    if described.is_empty() {
        SharedString::from(reason.to_owned())
    } else {
        crate::i18n::t!(
            "misc.markdown.image_reason_with_target",
            reason = reason,
            target = described
        )
        .into()
    }
}

/// The picture element for `source`, or `None` when the source does not resolve
/// to something drawable at all.
///
/// Both previews take the same two steps — resolve the source against the
/// document's directory, then build an element that keeps per-frame state — and
/// differ only in how they size the result and what they show in its place.
fn markdown_preview_resolved_picture(
    source: &str,
    id: gpui::ElementId,
    image_base_dir: Option<&std::path::Path>,
) -> Option<gpui::Stateful<gpui::Img>> {
    markdown_preview_image_source(image_base_dir, source)
        .map(|source| markdown_preview_image_element(source, id))
}

/// Stand-in shown in place of a picture, so the row is never silently blank.
fn markdown_preview_image_placeholder_element(
    label: SharedString,
    font_size: Pixels,
    color: gpui::Rgba,
) -> gpui::Div {
    div()
        .w_full()
        .h_full()
        .flex()
        .items_center()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_size(font_size)
        .text_color(color)
        .child(label)
}

/// Stand-in for a source that could not be resolved at all.
fn markdown_preview_image_placeholder(
    row: &MarkdownPreviewRow,
    context: &MarkdownPreviewRenderContext<'_>,
    reason: &str,
) -> gpui::Div {
    markdown_preview_image_placeholder_element(
        markdown_preview_image_label(row, reason),
        markdown_preview_scaled_px(MARKDOWN_PREVIEW_BASE_FONT_PX, context.ui_scale_percent),
        context.theme.colors.foreground.secondary,
    )
}

/// One horizontal band of an image block.
pub(super) fn markdown_preview_image_row(
    row: &MarkdownPreviewRow,
    row_ix: usize,
    slice_ix: u8,
    slice_count: u8,
    context: &MarkdownPreviewRenderContext<'_>,
) -> AnyElement {
    let ui_scale_percent = context.ui_scale_percent;
    let row_height = markdown_preview_row_height(ui_scale_percent);
    let block_height = row_height * f32::from(slice_count.max(1));
    let picture = row.image.as_ref().and_then(|image| {
        markdown_preview_resolved_picture(
            image.source.as_ref(),
            ("markdown_preview_image_band", row_ix).into(),
            context.image_base_dir.as_deref(),
        )
    });
    // A declared width is the size the document asked for; without one the
    // picture fills the block.
    let declared_width = row
        .image
        .as_ref()
        .and_then(|image| image.width_px)
        .map(|width| markdown_preview_scaled_px(width as f32, ui_scale_percent));

    let band = div().relative().w_full().h(row_height).overflow_hidden();
    let Some(image) = picture else {
        // Nothing to draw: the first band describes the picture instead, and
        // the rest stay blank so the block keeps its shape.
        if slice_ix != 0 {
            return band.into_any_element();
        }
        return band
            .child(markdown_preview_image_placeholder(
                row,
                context,
                crate::i18n::tr_str("misc.markdown.image_unavailable"),
            ))
            .into_any_element();
    };

    // `with_fallback` is called on demand, so the placeholder is rebuilt from
    // owned pieces rather than cloning a built element.
    let failed_label =
        markdown_preview_image_label(row, crate::i18n::tr_str("misc.markdown.failed_to_load"));
    let failed_font_size =
        markdown_preview_scaled_px(MARKDOWN_PREVIEW_BASE_FONT_PX, ui_scale_percent);
    let failed_color = context.theme.colors.foreground.secondary;
    // `Contain` keeps the aspect ratio inside whichever box the document asked
    // for, so a declared width never stretches the picture across the row.
    let image = match declared_width {
        Some(width) => image.w(width).max_w(width),
        None => image.w_full(),
    };
    band.child(
        div()
            .absolute()
            .left_0()
            .right_0()
            // Every band draws the whole picture and clips to its own slice, so
            // a block that is half scrolled off screen still renders correctly.
            .top(-(row_height * f32::from(slice_ix)))
            .h(block_height)
            .child(
                image
                    .h(block_height)
                    .object_fit(gpui::ObjectFit::Contain)
                    // A source that resolved but would not load — a 404 badge,
                    // an unreachable host, an undecodable file — says so rather
                    // than leaving a blank band.
                    .with_fallback(move || {
                        markdown_preview_image_placeholder_element(
                            failed_label.clone(),
                            failed_font_size,
                            failed_color,
                        )
                        .into_any_element()
                    }),
            ),
    )
    .into_any_element()
}
