//! Core model of the markdown preview.
//!
//! Rows, inline spans, images, the parse contexts rows are built from, and the
//! per-row measurement caches the renderers consult — everything downstream
//! domains ([`super::flatten`], [`super::diff`], [`super::wrap`]) build on.

use crate::view::CachedDiffStyledText;
use gpui::SharedString;
use std::ops::Range;
use std::sync::{Arc, Mutex, OnceLock};

/// Maximum source size (bytes) for a single markdown preview document.
pub(in crate::view) const MAX_PREVIEW_SOURCE_BYTES: usize = 1_024 * 1_024; // 1 MiB

/// Maximum combined source size (bytes) for a two-sided diff preview.
pub(in crate::view) const MAX_DIFF_PREVIEW_SOURCE_BYTES: usize = 2 * 1_024 * 1_024; // 2 MiB

/// Maximum number of preview rows per document.
pub(in crate::view) const MAX_PREVIEW_ROWS: usize = 20_000;

/// Maximum number of rows the single-document preview renders.
///
/// That preview lays its whole document out at once so text can wrap and
/// pictures can sit inline, which means every row costs layout on every frame —
/// unlike the diff preview, which paints a virtualized window of a fixed row
/// grid and is bounded by [`MAX_PREVIEW_ROWS`] instead. A document past this
/// budget falls back to source mode rather than making the pane crawl.
pub(in crate::view) const MAX_FLOWING_PREVIEW_ROWS: usize = 4_000;

/// Maximum number of inline spans per row before degrading to plain text.
pub(super) const MAX_INLINE_SPANS_PER_ROW: usize = 512;

// ── Core types ──────────────────────────────────────────────────────────

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct MarkdownPreviewDocument {
    pub(in crate::view) rows: Vec<MarkdownPreviewRow>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct MarkdownPreviewDiff {
    pub(in crate::view) old: MarkdownPreviewDocument,
    pub(in crate::view) new: MarkdownPreviewDocument,
    pub(in crate::view) inline: MarkdownPreviewDocument,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct MarkdownPreviewRow {
    pub(in crate::view) kind: MarkdownPreviewRowKind,
    pub(in crate::view) text: SharedString,
    pub(in crate::view) inline_spans: Arc<Vec<MarkdownInlineSpan>>,
    pub(in crate::view) code_language: Option<crate::view::rows::DiffSyntaxLanguage>,
    pub(in crate::view) code_block_horizontal_scroll_hint: bool,
    pub(in crate::view) source_line_range: Range<usize>,
    pub(in crate::view) change_hint: MarkdownChangeHint,
    pub(in crate::view) indent_level: u8,
    pub(in crate::view) blockquote_level: u8,
    pub(in crate::view) footnote_label: Option<SharedString>,
    pub(in crate::view) alert_kind: Option<MarkdownAlertKind>,
    pub(in crate::view) starts_alert: bool,
    /// The image an [`MarkdownPreviewRowKind::Image`] row paints.
    pub(in crate::view) image: Option<Arc<MarkdownImage>>,
    /// Pictures that share this row's line with its text, in document order.
    pub(in crate::view) inline_images: Arc<[MarkdownInlineImage]>,
    pub(in crate::view) styled_text_cache: MarkdownPreviewRowStyledTextCache,
    pub(in crate::view) measured_width_px: MarkdownPreviewRowWidthCache,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) enum MarkdownPreviewRowKind {
    Heading {
        level: u8,
    },
    Paragraph,
    DetailsSummary,
    ListItem {
        number: Option<u64>,
    },
    BlockquoteLine,
    CodeLine {
        is_first: bool,
        is_last: bool,
    },
    ThematicBreak,
    TableRow {
        is_header: bool,
    },
    /// One horizontal band of an image block.
    ///
    /// The preview paints into a uniform (fixed row height) list, so an image
    /// occupies `slice_count` consecutive rows and each row shows the band of
    /// the picture at its own `slice_ix`. Slicing it this way — rather than
    /// letting one tall row overflow its neighbours — keeps the image correct
    /// when it is scrolled half out of view, because every row draws itself.
    Image {
        slice_ix: u8,
        slice_count: u8,
    },
    PlainFallback,
    Spacer,
}

impl MarkdownPreviewRowKind {
    pub(super) fn is_image(&self) -> bool {
        matches!(self, Self::Image { .. })
    }
}

impl MarkdownPreviewRow {
    /// Whether this row only continues a picture an earlier row already
    /// carries, and so is not a line of the document in its own right.
    pub(in crate::view) fn continues_a_picture(&self) -> bool {
        matches!(self.kind, MarkdownPreviewRowKind::Image { slice_ix, .. } if slice_ix > 0)
    }
}

/// Rows an image block occupies when the document says nothing about the
/// picture's size.
pub(super) const MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS: u8 = 8;

/// Design row height, used to turn a declared image size into a row count at
/// parse time. Must track `MARKDOWN_PREVIEW_ROW_HEIGHT_PX`; both scale with the
/// UI together, so the row count is scale-independent.
const MARKDOWN_PREVIEW_IMAGE_ROW_HEIGHT_PX: u32 = 28;

/// A picture that shares a line with the text around it.
///
/// Markdown draws no distinction between a picture on a line of its own and one
/// written mid-sentence — badges, shields, and a logo beside a heading are all
/// ordinary inline content. A row therefore carries its pictures alongside its
/// text instead of displacing it, and only a picture that is alone on its line
/// becomes a block of its own.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct MarkdownInlineImage {
    /// Byte offset in the row's text where the picture belongs.
    pub(in crate::view) byte_offset: usize,
    /// Byte offset in the *source document* where the picture was written.
    ///
    /// Unique across the document, which makes it the element id a renderer
    /// can key on without allocating one, and the only thing left to tie a
    /// picture back to the line it came from once its row is built.
    pub(in crate::view) source_byte: usize,
    pub(in crate::view) image: Arc<MarkdownImage>,
    /// Description shown when the picture cannot be drawn.
    pub(in crate::view) alt: SharedString,
    /// The link the picture stands in for, when it is wrapped in one.
    pub(in crate::view) link_url: Option<SharedString>,
}

/// An image a preview row draws, with whatever size the document declared.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct MarkdownImage {
    /// The source exactly as written in the document.
    pub(in crate::view) source: SharedString,
    pub(in crate::view) width_px: Option<u32>,
    pub(in crate::view) height_px: Option<u32>,
}

impl MarkdownImage {
    /// Rows this image's block occupies.
    ///
    /// A declared height is authoritative. With only a width — the common
    /// `<img width="26">` used for an inline logo — the picture is assumed no
    /// taller than it is wide, which keeps small images from reserving a
    /// screenful of blank rows. `object_fit: contain` letterboxes anything
    /// that turns out to be taller.
    pub(in crate::view) fn block_rows(&self) -> u8 {
        // A declared size of zero says nothing about how tall the picture is,
        // so it is treated as undeclared rather than collapsing the block to a
        // single row — and each dimension is judged on its own, so `height="0"`
        // falls through to a usable width instead of discarding it.
        let Some(declared) = self
            .height_px
            .filter(|declared| *declared > 0)
            .or(self.width_px.filter(|declared| *declared > 0))
        else {
            return MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS;
        };
        let rows = declared
            .div_ceil(MARKDOWN_PREVIEW_IMAGE_ROW_HEIGHT_PX)
            .max(1);
        u8::try_from(rows)
            .unwrap_or(MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS)
            .min(MARKDOWN_PREVIEW_IMAGE_BLOCK_ROWS)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct MarkdownInlineSpan {
    pub(in crate::view) byte_range: Range<usize>,
    pub(in crate::view) style: MarkdownInlineStyle,
    /// Destination of the link this span sits inside.
    ///
    /// Carried on the span rather than in a parallel list so it survives the
    /// byte remapping that whitespace normalisation and table alignment apply,
    /// and independently of `style` because a bold or code span inside a link
    /// resolves to that style while still being clickable.
    pub(in crate::view) link_url: Option<SharedString>,
}

impl MarkdownInlineSpan {
    pub(super) fn restyled(&self, byte_range: Range<usize>) -> Self {
        Self {
            byte_range,
            style: self.style,
            link_url: self.link_url.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) enum MarkdownInlineStyle {
    Normal,
    Bold,
    Italic,
    BoldItalic,
    Code,
    Strikethrough,
    Link,
    Underline,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::view) enum MarkdownChangeHint {
    #[default]
    None,
    Added,
    Removed,
    Modified,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::view) enum MarkdownAlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MarkdownBlockQuoteContext {
    pub(super) alert_kind: Option<MarkdownAlertKind>,
    pub(super) emitted_row: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MarkdownFootnoteContext {
    pub(super) label: SharedString,
    pub(super) emitted_label: bool,
}

/// Parse state every row flush consults.
///
/// Both parts are answers to "what does the row being closed inherit?": the
/// blockquote stack decides its alert, and `pending_images` holds the pictures
/// read since the last flush, which belong to the line they were written on.
#[derive(Default)]
pub(super) struct MarkdownRowContext {
    pub(super) blockquote_stack: Vec<MarkdownBlockQuoteContext>,
    pub(super) pending_images: Vec<MarkdownInlineImage>,
}

impl MarkdownRowContext {
    /// True when a row has to be emitted even though its text is empty.
    ///
    /// Pictures only reach the document through the row that closes over them,
    /// so a construct that would otherwise skip an empty row — a list item
    /// holding nothing but a badge — has to emit one anyway or the picture is
    /// carried onto an unrelated row later, or dropped at the end of the parse.
    pub(super) fn has_pending_images(&self) -> bool {
        !self.pending_images.is_empty()
    }
}

pub(super) struct MarkdownPreviewRowInput<'a> {
    pub(super) kind: MarkdownPreviewRowKind,
    pub(super) text: &'a str,
    pub(super) inline_spans: &'a [MarkdownInlineSpan],
    pub(super) code_language: Option<crate::view::rows::DiffSyntaxLanguage>,
    pub(super) code_block_horizontal_scroll_hint: bool,
    pub(super) source_line_range: Range<usize>,
    pub(super) indent_level: u8,
    pub(super) blockquote_level: u8,
    pub(super) image: Option<Arc<MarkdownImage>>,
    pub(super) inline_images: Arc<[MarkdownInlineImage]>,
}

impl<'a> MarkdownPreviewRowInput<'a> {
    pub(super) fn plain(
        kind: MarkdownPreviewRowKind,
        text: &'a str,
        inline_spans: &'a [MarkdownInlineSpan],
        source_line_range: Range<usize>,
        indent_level: u8,
        blockquote_level: u8,
    ) -> Self {
        Self {
            kind,
            text,
            inline_spans,
            code_language: None,
            code_block_horizontal_scroll_hint: false,
            source_line_range,
            indent_level,
            blockquote_level,
            image: None,
            inline_images: Arc::from(Vec::new()),
        }
    }

    pub(super) fn code(
        kind: MarkdownPreviewRowKind,
        text: &'a str,
        source_line_range: Range<usize>,
        code_language: Option<crate::view::rows::DiffSyntaxLanguage>,
        code_block_horizontal_scroll_hint: bool,
        indent_level: u8,
        blockquote_level: u8,
    ) -> Self {
        Self {
            kind,
            text,
            inline_spans: &[],
            code_language,
            code_block_horizontal_scroll_hint,
            source_line_range,
            indent_level,
            blockquote_level,
            image: None,
            inline_images: Arc::from(Vec::new()),
        }
    }

    pub(super) fn image(
        slice_ix: u8,
        slice_count: u8,
        alt: &'a str,
        image: Arc<MarkdownImage>,
        source_line_range: Range<usize>,
        indent_level: u8,
        blockquote_level: u8,
    ) -> Self {
        Self {
            kind: MarkdownPreviewRowKind::Image {
                slice_ix,
                slice_count,
            },
            // The alt text stays the row text so selection and copy still see
            // something meaningful, and so a picture that cannot be loaded can
            // fall back to describing itself.
            text: alt,
            inline_spans: &[],
            code_language: None,
            code_block_horizontal_scroll_hint: false,
            source_line_range,
            indent_level,
            blockquote_level,
            image: Some(image),
            inline_images: Arc::from(Vec::new()),
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct MarkdownPreviewRowDecoration {
    pub(super) footnote_label: Option<SharedString>,
    pub(super) alert_kind: Option<MarkdownAlertKind>,
    pub(super) starts_alert: bool,
}

#[derive(Debug, Default)]
pub(in crate::view) struct MarkdownPreviewRowWidthCache(Mutex<Option<(u64, u32)>>);

impl Clone for MarkdownPreviewRowWidthCache {
    fn clone(&self) -> Self {
        let cached = match self.0.lock() {
            Ok(guard) => *guard,
            Err(poisoned) => *poisoned.into_inner(),
        };

        Self(Mutex::new(cached))
    }
}

impl MarkdownPreviewRowWidthCache {
    pub(in crate::view) fn get_or_init(&self, key: u64, compute: impl FnOnce() -> u32) -> u32 {
        let mut cached = match self.0.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some((cached_key, value)) = *cached
            && cached_key == key
        {
            return value;
        }

        let value = compute();
        *cached = Some((key, value));
        value
    }
}

impl PartialEq for MarkdownPreviewRowWidthCache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for MarkdownPreviewRowWidthCache {}

#[derive(Clone, Debug, Default)]
pub(in crate::view) struct MarkdownPreviewRowStyledTextCache {
    dark: OnceLock<CachedDiffStyledText>,
    light: OnceLock<CachedDiffStyledText>,
}

impl MarkdownPreviewRowStyledTextCache {
    pub(in crate::view) fn get_or_init(
        &self,
        is_dark: bool,
        compute: impl FnOnce() -> CachedDiffStyledText,
    ) -> &CachedDiffStyledText {
        if is_dark {
            self.dark.get_or_init(compute)
        } else {
            self.light.get_or_init(compute)
        }
    }
}

impl PartialEq for MarkdownPreviewRowStyledTextCache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for MarkdownPreviewRowStyledTextCache {}

// ── Flowing document blocks ─────────────────────────────────────────────

/// A run of rows that renders as one element in the flowing preview.
///
/// The row model is shaped for the diff preview, which paints into a uniform
/// (fixed row height) list and therefore needs one row per painted line. The
/// single-document preview lays out naturally instead, so consecutive rows
/// belonging to the same construct — the lines of a code block, the bands of
/// an image, the rows of a table — are grouped back into the block they came
/// from. Both previews stay on one parsed model this way.
/// Blocks address rows by index rather than by reference: selection, copy, and
/// hit testing are all keyed by row index, so the flowing renderer hands the
/// same indices to the same machinery the row preview used.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) enum MarkdownBlock {
    Heading {
        level: u8,
        row_ix: usize,
    },
    Paragraph(usize),
    /// The bands of one image; only the first carries the source.
    Image(Range<usize>),
    ThematicBreak(usize),
    List(Range<usize>),
    Blockquote(Range<usize>),
    Code(Range<usize>),
    Table(Range<usize>),
}

impl MarkdownBlock {
    /// Rows this block paints, in document order.
    pub(in crate::view) fn row_range(&self) -> Range<usize> {
        match self {
            Self::Heading { row_ix, .. }
            | Self::Paragraph(row_ix)
            | Self::ThematicBreak(row_ix) => *row_ix..*row_ix + 1,
            Self::Image(range)
            | Self::List(range)
            | Self::Blockquote(range)
            | Self::Code(range)
            | Self::Table(range) => range.clone(),
        }
    }
}

/// Group a document's rows into the blocks the flowing preview renders.
///
/// Spacer rows are dropped: they exist to open a gap in a fixed row grid, and
/// the flowing layout expresses the same gap as a margin.
pub(in crate::view) fn markdown_document_blocks(
    document: &MarkdownPreviewDocument,
) -> Vec<MarkdownBlock> {
    let mut blocks: Vec<MarkdownBlock> = Vec::new();
    let mut ix = 0usize;

    while ix < document.rows.len() {
        let row = &document.rows[ix];
        match row.kind {
            MarkdownPreviewRowKind::Spacer => ix += 1,
            MarkdownPreviewRowKind::ThematicBreak => {
                blocks.push(MarkdownBlock::ThematicBreak(ix));
                ix += 1;
            }
            MarkdownPreviewRowKind::Heading { level } => {
                blocks.push(MarkdownBlock::Heading { level, row_ix: ix });
                ix += 1;
            }
            MarkdownPreviewRowKind::Image { .. } => {
                // Every band of one image repeats the same source; the block
                // needs it once.
                let source = row.image.as_ref().map(|image| image.source.clone());
                let start = ix;
                ix += 1;
                while ix < document.rows.len()
                    && document.rows[ix].kind.is_image()
                    && document.rows[ix]
                        .image
                        .as_ref()
                        .map(|image| image.source.clone())
                        == source
                    && !matches!(
                        document.rows[ix].kind,
                        MarkdownPreviewRowKind::Image { slice_ix: 0, .. }
                    )
                {
                    ix += 1;
                }
                blocks.push(MarkdownBlock::Image(start..ix));
            }
            MarkdownPreviewRowKind::ListItem { .. } => {
                blocks.push(MarkdownBlock::List(take_run(
                    document,
                    &mut ix,
                    |_, row| matches!(row.kind, MarkdownPreviewRowKind::ListItem { .. }),
                )));
            }
            MarkdownPreviewRowKind::BlockquoteLine => {
                // Two alerts that touch are two blocks: each carries its own
                // bar and badge, and folding them together would label the
                // second one with the first one's kind.
                blocks.push(MarkdownBlock::Blockquote(take_run(
                    document,
                    &mut ix,
                    |offset, row| {
                        matches!(row.kind, MarkdownPreviewRowKind::BlockquoteLine)
                            && (offset == 0 || !row.starts_alert)
                    },
                )));
            }
            MarkdownPreviewRowKind::CodeLine { .. } => {
                blocks.push(MarkdownBlock::Code(take_run(
                    document,
                    &mut ix,
                    |_, row| matches!(row.kind, MarkdownPreviewRowKind::CodeLine { .. }),
                )));
            }
            MarkdownPreviewRowKind::TableRow { .. } => {
                // A header row opens a table, so it ends the one before it for
                // the same reason an alert's first row ends the quote above.
                blocks.push(MarkdownBlock::Table(take_run(
                    document,
                    &mut ix,
                    |offset, row| match row.kind {
                        MarkdownPreviewRowKind::TableRow { is_header } => offset == 0 || !is_header,
                        _ => false,
                    },
                )));
            }
            MarkdownPreviewRowKind::Paragraph
            | MarkdownPreviewRowKind::DetailsSummary
            | MarkdownPreviewRowKind::PlainFallback => {
                blocks.push(MarkdownBlock::Paragraph(ix));
                ix += 1;
            }
        }
    }

    blocks
}

/// Consume the run of consecutive rows `belongs` accepts, which sees each row
/// together with its offset from the start of the run.
fn take_run(
    document: &MarkdownPreviewDocument,
    ix: &mut usize,
    belongs: impl Fn(usize, &MarkdownPreviewRow) -> bool,
) -> Range<usize> {
    let start = *ix;
    while let Some(row) = document.rows.get(*ix) {
        if !belongs(*ix - start, row) {
            break;
        }
        *ix += 1;
    }
    start..*ix
}

/// Return a user-facing reason why a single-document markdown preview is
/// unavailable for a source of `source_len` bytes.
pub(in crate::view) fn single_preview_unavailable_reason(source_len: usize) -> &'static str {
    if source_len > MAX_PREVIEW_SOURCE_BYTES {
        crate::i18n::tr_str("misc.markdown.preview_source_limit")
    } else {
        crate::i18n::tr_str("misc.markdown.preview_row_limit")
    }
}

/// Why a single-document preview could not be produced.
///
/// The two cases read the same to a user — no preview — but they are not the
/// same problem: one document cannot be parsed within the row cap at all, the
/// other parses fine and is only too big for a renderer that lays every row
/// out at once. Only the second has a good answer, which is to show the source.
pub(in crate::view) const TOO_MANY_ROWS_TO_RENDER_MESSAGE: &str =
    "Markdown preview unavailable: document is too large to render; showing source.";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) enum MarkdownPreviewRefusal {
    /// Unreadable, or past the source-size or parsed-row cap.
    Unavailable(String),
    /// Parsed, but past what the flowing renderer will lay out in a frame.
    TooManyRowsToRender,
}

impl From<String> for MarkdownPreviewRefusal {
    fn from(reason: String) -> Self {
        Self::Unavailable(reason)
    }
}

impl MarkdownPreviewRefusal {
    pub(in crate::view) fn into_message(self) -> String {
        match self {
            Self::Unavailable(reason) => reason,
            Self::TooManyRowsToRender => {
                crate::i18n::tr_en(TOO_MANY_ROWS_TO_RENDER_MESSAGE).to_string()
            }
        }
    }

    /// True when the reader is better served by the source than by an error.
    pub(in crate::view) fn prefers_source(&self) -> bool {
        matches!(self, Self::TooManyRowsToRender)
    }
}

/// Return a user-facing reason why a two-sided diff markdown preview is
/// unavailable for sources of `combined_len` bytes.
pub(in crate::view) fn diff_preview_unavailable_reason(combined_len: usize) -> &'static str {
    if combined_len > MAX_DIFF_PREVIEW_SOURCE_BYTES {
        crate::i18n::tr_str("misc.markdown.preview_diff_limit")
    } else {
        crate::i18n::tr_str("misc.markdown.preview_row_limit")
    }
}

pub(super) fn markdown_preview_spacer_row() -> MarkdownPreviewRow {
    markdown_preview_spacer_row_with_range(0..0)
}

pub(super) fn markdown_preview_spacer_row_with_range(
    source_line_range: Range<usize>,
) -> MarkdownPreviewRow {
    MarkdownPreviewRow {
        kind: MarkdownPreviewRowKind::Spacer,
        text: SharedString::from(""),
        inline_spans: Arc::new(Vec::new()),
        code_language: None,
        code_block_horizontal_scroll_hint: false,
        source_line_range,
        change_hint: MarkdownChangeHint::None,
        indent_level: 0,
        blockquote_level: 0,
        footnote_label: None,
        alert_kind: None,
        starts_alert: false,
        image: None,
        inline_images: Arc::from(Vec::new()),
        styled_text_cache: MarkdownPreviewRowStyledTextCache::default(),
        measured_width_px: MarkdownPreviewRowWidthCache::default(),
    }
}
