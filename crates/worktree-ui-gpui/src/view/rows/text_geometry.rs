use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(in crate::view) enum DiffTextRegion {
    Inline,
    SplitLeft,
    SplitRight,
}

impl DiffTextRegion {
    pub(in crate::view) fn order(self) -> u8 {
        match self {
            DiffTextRegion::Inline | DiffTextRegion::SplitLeft => 0,
            DiffTextRegion::SplitRight => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) struct DiffTextPos {
    pub(in crate::view) source_visible_ix: usize,
    pub(in crate::view) region: DiffTextRegion,
    pub(in crate::view) offset: usize,
}

impl DiffTextPos {
    pub(in crate::view) fn cmp_key(self) -> (usize, u8, usize) {
        (self.source_visible_ix, self.region.order(), self.offset)
    }
}

pub(in crate::view) struct DiffTextHitbox {
    pub(in crate::view) bounds: Bounds<Pixels>,
    pub(in crate::view) layout_key: u64,
    pub(in crate::view) source_visible_ix: usize,
    pub(in crate::view) text_start_offset: usize,
    pub(in crate::view) text_len: usize,
    pub(in crate::view) offset_map: Option<DiffTextOffsetMap>,
    /// Exactly the text this row painted, tabs expanded and whitespace revealed
    /// as they were on screen.
    ///
    /// Offsets into it are the display offsets `x_for_index` wants, which is why
    /// the search reveal measures against this rather than re-deriving the row's
    /// text: the two do not always agree, and a row whose text cannot be found
    /// again reveals nothing.
    pub(in crate::view) painted_text: SharedString,
    pub(in crate::view) streamed_ascii_monospace_cell_width: Option<Pixels>,
    /// Set by rows that painted their text with wrapping. Those rows cover
    /// several visual lines, so a click resolves through the layout they were
    /// painted with rather than through an x offset along one shaped line.
    pub(in crate::view) wrapped: Option<DiffTextWrappedHit>,
}

/// Where one merge-tool column row painted its text, and the line it shaped.
///
/// The conflict columns are their own canvases and register nothing in
/// [`DiffTextHitbox`], so quick search's sideways reveal measures against this
/// instead. `layout.text` is exactly what was painted, so offsets into it are
/// the display offsets `x_for_index` wants.
pub(in crate::view) struct ConflictTextHitbox {
    pub(in crate::view) bounds: Bounds<Pixels>,
    pub(in crate::view) layout: gpui::ShapedLine,
}

/// The wrapped layout a row painted, plus what it takes to read offsets back
/// in row coordinates.
pub(in crate::view) struct DiffTextWrappedHit {
    pub(in crate::view) layout: gpui::TextLayout,
    /// The row's raw text, when tabs were expanded for painting.
    pub(in crate::view) untabbed: Option<SharedString>,
}

impl DiffTextWrappedHit {
    /// Offset in row coordinates for an offset in the painted text.
    pub(in crate::view) fn row_offset(&self, painted_offset: usize) -> usize {
        match &self.untabbed {
            Some(raw) => markdown_flow_row_offset(raw, painted_offset),
            None => painted_offset,
        }
    }

    /// Offset in the painted text for an offset in row coordinates — the
    /// inverse of [`Self::row_offset`].
    pub(in crate::view) fn painted_offset(&self, row_offset: usize) -> usize {
        match &self.untabbed {
            Some(raw) => markdown_flow_painted_offset(raw, row_offset),
            None => row_offset,
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::view) struct DiffTextOffsetMap {
    pub(in crate::view) display_to_source: Arc<[usize]>,
    pub(in crate::view) source_to_display: Arc<[usize]>,
}

impl DiffTextOffsetMap {
    pub(in crate::view) fn display_len(&self) -> usize {
        self.display_to_source.len().saturating_sub(1)
    }

    pub(in crate::view) fn source_len(&self) -> usize {
        self.source_to_display.len().saturating_sub(1)
    }

    pub(in crate::view) fn source_offset_for_display(&self, offset: usize) -> usize {
        self.display_to_source
            .get(offset.min(self.display_len()))
            .copied()
            .unwrap_or_else(|| self.source_len())
    }

    pub(in crate::view) fn display_offset_for_source(&self, offset: usize) -> usize {
        self.source_to_display
            .get(offset.min(self.source_len()))
            .copied()
            .unwrap_or_else(|| self.display_len())
    }
}
