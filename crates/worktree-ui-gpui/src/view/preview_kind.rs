use super::*;

pub(super) fn is_svg_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
}

pub(super) fn should_bypass_text_file_preview_for_path(path: &std::path::Path) -> bool {
    image_format_for_path(path).is_some()
        || path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("ico"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RenderableConflictFile {
    Loading,
    Error(SharedString),
    Missing,
    File(worktree_state::model::ConflictFile),
}

pub(super) fn conflict_file_is_binary(file: &worktree_state::model::ConflictFile) -> bool {
    let has_non_text = |bytes: &Option<std::sync::Arc<[u8]>>,
                        text: &Option<std::sync::Arc<str>>| {
        bytes.is_some() && text.is_none()
    };
    has_non_text(&file.base_bytes, &file.base)
        || has_non_text(&file.ours_bytes, &file.ours)
        || has_non_text(&file.theirs_bytes, &file.theirs)
        || has_non_text(&file.current_bytes, &file.current)
}

pub(super) fn renderable_conflict_file(
    repo: &RepoState,
    conflict_resolver: &ConflictResolverUiState,
    target_path: &std::path::Path,
) -> RenderableConflictFile {
    match &repo.conflict_state.conflict_file {
        Loadable::Ready(Some(file)) if file.path == target_path => {
            RenderableConflictFile::File(file.clone())
        }
        Loadable::Ready(Some(_)) => RenderableConflictFile::Loading,
        Loadable::Loading | Loadable::NotLoaded => conflict_resolver
            .cached_loaded_file_for_target(repo.id, target_path)
            .cloned()
            .map(RenderableConflictFile::File)
            .unwrap_or(RenderableConflictFile::Loading),
        Loadable::Error(error) => RenderableConflictFile::Error(error.clone().into()),
        Loadable::Ready(None) => RenderableConflictFile::Missing,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DiffViewMode {
    Inline,
    Split,
}

impl DiffViewMode {
    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::Split => "split",
        }
    }

    pub(super) fn from_key(raw: &str) -> Option<Self> {
        match raw {
            "inline" => Some(Self::Inline),
            "split" => Some(Self::Split),
            _ => None,
        }
    }

    pub(super) fn settings_label(self) -> &'static str {
        match self {
            Self::Inline => crate::i18n::tr_str("ui.label.diff.view.inline"),
            Self::Split => crate::i18n::tr_str("ui.label.diff.view.split"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum RenderedPreviewKind {
    Svg,
    Markdown,
}

impl RenderedPreviewKind {
    pub(super) fn rendered_label(self) -> &'static str {
        match self {
            Self::Svg => crate::i18n::tr_str("ui.label.preview.image"),
            Self::Markdown => crate::i18n::tr_str("ui.label.preview.preview"),
        }
    }

    pub(super) fn source_label(self) -> &'static str {
        match self {
            Self::Svg => crate::i18n::tr_str("ui.label.preview.code"),
            Self::Markdown => crate::i18n::tr_str("ui.label.preview.text"),
        }
    }

    pub(super) fn rendered_button_id(self) -> &'static str {
        match self {
            Self::Svg => "svg_diff_view_image",
            Self::Markdown => "markdown_diff_view_preview",
        }
    }

    pub(super) fn toggle_id(self) -> &'static str {
        match self {
            Self::Svg => "svg_diff_view_toggle",
            Self::Markdown => "markdown_diff_view_toggle",
        }
    }

    pub(super) fn source_button_id(self) -> &'static str {
        match self {
            Self::Svg => "svg_diff_view_code",
            Self::Markdown => "markdown_diff_view_text",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RenderedPreviewMode {
    Rendered,
    Source,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RenderedPreviewModes {
    pub(super) svg: RenderedPreviewMode,
    pub(super) markdown: RenderedPreviewMode,
}

impl Default for RenderedPreviewModes {
    fn default() -> Self {
        Self {
            svg: RenderedPreviewMode::Rendered,
            markdown: RenderedPreviewMode::Rendered,
        }
    }
}

impl RenderedPreviewModes {
    pub(super) fn get(self, kind: RenderedPreviewKind) -> RenderedPreviewMode {
        match kind {
            RenderedPreviewKind::Svg => self.svg,
            RenderedPreviewKind::Markdown => self.markdown,
        }
    }

    pub(super) fn set(&mut self, kind: RenderedPreviewKind, mode: RenderedPreviewMode) {
        match kind {
            RenderedPreviewKind::Svg => self.svg = mode,
            RenderedPreviewKind::Markdown => self.markdown = mode,
        }
    }
}

/// Preview mode for the conflict resolver merge-input pane.
///
/// When the conflicted file supports a rendered preview (for example, SVG or
/// markdown), the user can toggle between the normal text diff view and a
/// rendered preview of each conflict side.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum ConflictResolverPreviewMode {
    /// Normal text/diff view with syntax highlighting.
    #[default]
    Text,
    /// Rendered preview (image for SVG files, rendered rows for markdown).
    Preview,
}

pub(super) fn is_markdown_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd" | "mkdn" | "mdwn"
            )
        })
}

pub(super) fn preview_path_rendered_kind(path: &std::path::Path) -> Option<RenderedPreviewKind> {
    if is_svg_path(path) {
        Some(RenderedPreviewKind::Svg)
    } else if is_markdown_path(path) {
        Some(RenderedPreviewKind::Markdown)
    } else {
        None
    }
}

pub(super) fn diff_target_rendered_preview_kind(
    target: Option<&DiffTarget>,
) -> Option<RenderedPreviewKind> {
    let path = match target? {
        DiffTarget::WorkingTree { path, .. } => path.as_path(),
        DiffTarget::Commit {
            path: Some(path), ..
        } => path.as_path(),
        _ => return None,
    };
    preview_path_rendered_kind(path)
}

pub(super) fn main_diff_rendered_preview_toggle_kind(
    wants_file_diff: bool,
    wants_collapsed_diff: bool,
    is_file_preview: bool,
    preview_kind: Option<RenderedPreviewKind>,
) -> Option<RenderedPreviewKind> {
    match preview_kind? {
        // Image/Code is orthogonal to the Full/Collapsed diff mode: the
        // rendered image is the whole file either way, and the source is a
        // normal text diff that both modes can show.
        // `is_file_preview` covers the content view an SVG gets when it is
        // opened from the file explorer: the picture is the whole file there
        // too, and Code is how you reach its source (and the editor).
        RenderedPreviewKind::Svg if wants_file_diff || wants_collapsed_diff || is_file_preview => {
            Some(RenderedPreviewKind::Svg)
        }
        RenderedPreviewKind::Markdown if wants_file_diff || is_file_preview => {
            Some(RenderedPreviewKind::Markdown)
        }
        _ => None,
    }
}
