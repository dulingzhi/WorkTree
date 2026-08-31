use super::super::*;
use gpui::SharedString;
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};
use tree_sitter::StreamingIterator;

const TS_DOCUMENT_CACHE_MAX_ENTRIES: usize = 8;
const TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS: usize = 64;
const TS_DOCUMENT_LINE_TOKEN_PREFETCH_GUARD_CHUNKS: usize = 1;
const DIFF_SYNTAX_FOREGROUND_PARSE_BUDGET_NON_TEST: Duration = Duration::from_millis(1);
const DIFF_SYNTAX_FOREGROUND_SKIP_TEXT_BYTES: usize = 128 * 1024;
const DIFF_SYNTAX_FOREGROUND_SKIP_LINE_COUNT: usize = 2_048;
const TS_QUERY_MATCH_LIMIT: u32 = 256;
const TS_MAX_BYTES_TO_QUERY: usize = 16 * 1024;
const TS_QUERY_MAX_LINES_PER_PASS: usize = 256;
const TS_DEFERRED_DROP_MIN_BYTES: usize = 256 * 1024;
const TS_INCREMENTAL_REPARSE_ENABLE_ENV: &str = "WORKTREE_DIFF_SYNTAX_INCREMENTAL_REPARSE";
const TS_INCREMENTAL_REPARSE_MAX_CHANGED_BYTES: usize = 64 * 1024;
const TS_INCREMENTAL_REPARSE_MAX_CHANGED_PERCENT: usize = 35;
const TS_INCREMENTAL_REPARSE_LATE_EDIT_MIN_PREFIX_BYTES: usize = 8 * 1024;
const TS_INCREMENTAL_REPARSE_LATE_EDIT_MAX_CHANGED_BYTES: usize = 384 * 1024;
const TS_INCREMENTAL_REPARSE_LATE_EDIT_MAX_CHANGED_PERCENT: usize = 80;
const TS_LINE_TOKEN_CACHE_MAX_ENTRIES: usize = 256;
// Extreme multi-megabyte documents are better served by the existing visible-line
// heuristic fallback than by building a full prepared tree-sitter document.
pub(in crate::view) const PREPARED_DIFF_SYNTAX_DOCUMENT_MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;
const TS_PREPARED_DOCUMENT_MAX_TEXT_BYTES: usize = PREPARED_DIFF_SYNTAX_DOCUMENT_MAX_TEXT_BYTES;
const TS_SHARED_DOCUMENT_SEED_MAX_ENTRIES: usize = 64;
const TS_PENDING_PARSE_REQUEST_MAX_ENTRIES: usize = 8;
const BASH_HIGHLIGHTS_QUERY: &str = include_str!("queries/bash_highlights.scm");
const C_HIGHLIGHTS_QUERY: &str = include_str!("queries/c_highlights.scm");
const C_INJECTIONS_QUERY: &str = include_str!("queries/c_injections.scm");
const CSHARP_HIGHLIGHTS_QUERY: &str = include_str!("queries/csharp_highlights.scm");
const CPP_HIGHLIGHTS_QUERY: &str = include_str!("queries/cpp_highlights.scm");
const GITCOMMIT_HIGHLIGHTS_QUERY: &str = include_str!("queries/gitcommit_highlights.scm");
const GOMOD_HIGHLIGHTS_QUERY: &str = include_str!("queries/gomod_highlights.scm");
const GOWORK_HIGHLIGHTS_QUERY: &str = include_str!("queries/gowork_highlights.scm");
const HTML_HIGHLIGHTS_QUERY: &str = include_str!("queries/html_highlights.scm");
const HTML_INJECTIONS_QUERY: &str = include_str!("queries/html_injections.scm");
const JINJA_HIGHLIGHTS_QUERY: &str = include_str!("queries/jinja_highlights.scm");
const JINJA_INJECTIONS_QUERY: &str = include_str!("queries/jinja_injections.scm");
const VUE_HIGHLIGHTS_QUERY: &str = include_str!("queries/vue_highlights.scm");
const VUE_INJECTIONS_QUERY: &str = include_str!("queries/vue_injections.scm");
const MARKDOWN_HIGHLIGHTS_QUERY: &str = tree_sitter_md::HIGHLIGHT_QUERY_BLOCK;
const MARKDOWN_INJECTIONS_QUERY: &str = tree_sitter_md::INJECTION_QUERY_BLOCK;
const MARKDOWN_INLINE_HIGHLIGHTS_QUERY: &str = tree_sitter_md::HIGHLIGHT_QUERY_INLINE;
const CSS_HIGHLIGHTS_QUERY: &str = include_str!("queries/css_highlights.scm");
const GO_HIGHLIGHTS_QUERY: &str = include_str!("queries/go_highlights.scm");
const GO_INJECTIONS_QUERY: &str = include_str!("queries/go_injections.scm");
const JAVASCRIPT_HIGHLIGHTS_QUERY: &str = include_str!("queries/javascript_highlights.scm");
const JAVASCRIPT_INJECTIONS_QUERY: &str = include_str!("queries/javascript_injections.scm");
const JSDOC_HIGHLIGHTS_QUERY: &str = include_str!("queries/jsdoc_highlights.scm");
const JSON_HIGHLIGHTS_QUERY: &str = include_str!("queries/json_highlights.scm");
const POWERSHELL_HIGHLIGHTS_QUERY: &str = tree_sitter_powershell::HIGHLIGHTS_QUERY;
const PYTHON_HIGHLIGHTS_QUERY: &str = include_str!("queries/python_highlights.scm");
const REGEX_HIGHLIGHTS_QUERY: &str = include_str!("queries/regex_highlights.scm");
const TYPESCRIPT_HIGHLIGHTS_QUERY: &str = include_str!("queries/typescript_highlights.scm");
const TYPESCRIPT_INJECTIONS_QUERY: &str = include_str!("queries/typescript_injections.scm");
const TSX_HIGHLIGHTS_QUERY: &str = include_str!("queries/tsx_highlights.scm");
const TSX_INJECTIONS_QUERY: &str = include_str!("queries/tsx_injections.scm");
const NIX_HIGHLIGHTS_QUERY: &str = include_str!("queries/nix_highlights.scm");
const NIX_INJECTIONS_QUERY: &str = include_str!("queries/nix_injections.scm");
const RUST_HIGHLIGHTS_QUERY: &str = include_str!("queries/rust_highlights.scm");
const RUST_INJECTIONS_QUERY: &str = include_str!("queries/rust_injections.scm");
const YAML_HIGHLIGHTS_QUERY: &str = include_str!("queries/yaml_highlights.scm");
const YAML_INJECTIONS_QUERY: &str = include_str!("queries/yaml_injections.scm");
const XML_HIGHLIGHTS_QUERY: &str = tree_sitter_xml::XML_HIGHLIGHT_QUERY;
const CPP_INJECTIONS_QUERY: &str = include_str!("queries/cpp_injections.scm");
const CLOJURE_HIGHLIGHTS_QUERY: &str = include_str!("queries/clojure_highlights.scm");
const JULIA_HIGHLIGHTS_QUERY: &str = include_str!("queries/julia_highlights.scm");
const OCAML_HIGHLIGHTS_QUERY: &str = include_str!("queries/ocaml_highlights.scm");
const SOLIDITY_HIGHLIGHTS_QUERY: &str = include_str!("queries/solidity_highlights.scm");
const SVELTE_HIGHLIGHTS_QUERY: &str = include_str!("queries/svelte_highlights.scm");
const SVELTE_INJECTIONS_QUERY: &str = include_str!("queries/svelte_injections.scm");

/// Maximum injection nesting depth. Root document = 0, first injection = 1.
/// This prevents infinite recursion if an injected language's highlight spec
/// itself contains an injection query.
const TS_MAX_INJECTION_DEPTH: usize = 1;
const TS_INJECTION_CACHE_MAX_ENTRIES: usize = 32;

/// Ceilings for one `(#set! injection.combined)` layer in the *prepared* path.
///
/// Hard limits rather than a time budget: the prepared path cannot repair a layer
/// dropped by a `ControlFlow::Break` -- that mechanism exists only in `live.rs` --
/// so a budgeted layer would appear and vanish with scroll timing.
///
/// Both are measured *after* the ranges are clipped by
/// [`combined_injection_clip_region`], and that ordering is load-bearing. A
/// template grammar hands out one `(text)` node per gap between tags, so an
/// unclipped node is document-sized: a 1900-line `.njk` is a single 138KB range,
/// which tripped the byte ceiling for every window and dropped HTML highlighting
/// from the whole file.
///
/// The byte ceiling is the one that bounds cost, since a combined parse lexes only
/// included bytes. The range ceiling is only an allocation guard on the
/// `Vec<tree_sitter::Range>`, sized so template density cannot reach it -- at 512 an
/// ordinary 8-column table row (~950 ranges in a clipped window) tripped it.
///
/// `live.rs` deliberately has no equivalent; see `parse_injection_layers`.
const TS_COMBINED_INJECTION_MAX_RANGES: usize = 16_384;
const TS_COMBINED_INJECTION_MAX_BYTES: usize = 128 * 1024;

/// Context on each side of the rendered window that a combined injection is still
/// parsed with.
///
/// Clipping to *exactly* the window is not output-preserving: a `<section` whose
/// attributes run onto the next lines is cut in half, and the surviving half loses
/// its captures to error recovery. Measured on that fixture: margin 0 differs on two
/// lines, margin 1024 is byte-identical to an unclipped parse. 4KB is well past any
/// realistic multi-line tag and still bounds the parse at window + 8KB.
const TS_COMBINED_INJECTION_CONTEXT_MARGIN_BYTES: usize = 4 * 1024;

thread_local! {
    static TS_PARSER: RefCell<tree_sitter::Parser> = RefCell::new(tree_sitter::Parser::new());
    static TS_PARSER_REQUIRES_LANGUAGE_RESET: Cell<bool> = const { Cell::new(false) };
    static TS_CURSOR: RefCell<tree_sitter::QueryCursor> = RefCell::new(tree_sitter::QueryCursor::new());
    static TS_INPUT: RefCell<String> = const { RefCell::new(String::new()) };
    static TS_DOCUMENT_CACHE: RefCell<TreesitterDocumentCache> = RefCell::new(TreesitterDocumentCache::new());
    static TS_LINE_TOKEN_CACHE: RefCell<SingleLineSyntaxTokenCache> = RefCell::new(SingleLineSyntaxTokenCache::new());
    static TS_INJECTION_CACHE: RefCell<FxHashMap<TreesitterInjectionMatch, CachedInjectionTokens>> = RefCell::new(FxHashMap::default());
    static TS_PENDING_PARSE_REQUESTS: RefCell<Vec<PendingParseRequest>> = const { RefCell::new(Vec::new()) };
    static TS_INJECTION_ACCESS_COUNTER: Cell<u64> = const { Cell::new(0) };
    static TS_INJECTION_DEPTH: Cell<usize> = const { Cell::new(0) };
    #[cfg(test)]
    static TS_PARSER_SET_LANGUAGE_CALL_COUNT: Cell<usize> = const { Cell::new(0) };
    #[cfg(test)]
    static TS_TREE_STATE_CLONE_COUNT: Cell<usize> = const { Cell::new(0) };
    #[cfg(test)]
    static TS_INCREMENTAL_PARSE_COUNT: Cell<usize> = const { Cell::new(0) };
    #[cfg(test)]
    static TS_INCREMENTAL_FALLBACK_COUNT: Cell<usize> = const { Cell::new(0) };
    #[cfg(test)]
    static TS_DOCUMENT_HASH_COUNT: Cell<usize> = const { Cell::new(0) };
}

fn invalidate_ts_parser_language_fast_path() {
    TS_PARSER_REQUIRES_LANGUAGE_RESET.with(|needs_reset| needs_reset.set(true));
}

fn catch_treesitter_query_panic<R>(f: impl FnOnce() -> R) -> Option<R> {
    // Upstream tree-sitter can panic during query predicate evaluation when a
    // recovered node reports a byte range that extends past the provided text.
    // Treat those as syntax-miss fallbacks instead of crashing the UI.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(result) => Some(result),
        Err(_) => {
            TS_CURSOR.with(|cursor| {
                *cursor.borrow_mut() = tree_sitter::QueryCursor::new();
            });
            invalidate_ts_parser_language_fast_path();
            None
        }
    }
}

fn ascii_lowercase_for_match(s: &str) -> Cow<'_, str> {
    if s.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(s.to_ascii_lowercase())
    } else {
        Cow::Borrowed(s)
    }
}

fn with_ts_parser<R>(
    ts_language: &tree_sitter::Language,
    f: impl FnOnce(&mut tree_sitter::Parser) -> R,
) -> Option<R> {
    TS_PARSER.with(|parser| {
        let mut parser = parser.borrow_mut();
        let needs_language_reset =
            TS_PARSER_REQUIRES_LANGUAGE_RESET.with(|needs_reset| needs_reset.replace(false));
        let parser_language_matches = parser
            .language()
            .as_deref()
            .is_some_and(|current| current == ts_language);

        if needs_language_reset || !parser_language_matches {
            #[cfg(test)]
            TS_PARSER_SET_LANGUAGE_CALL_COUNT.with(|count| count.set(count.get() + 1));
            if parser.set_language(ts_language).is_err() {
                invalidate_ts_parser_language_fast_path();
                return None;
            }
        }
        Some(f(&mut parser))
    })
}

fn with_ts_parser_parse_result<R>(
    ts_language: &tree_sitter::Language,
    f: impl FnOnce(&mut tree_sitter::Parser) -> Option<R>,
) -> Option<R> {
    let result = with_ts_parser(ts_language, f).flatten();
    if result.is_none() {
        invalidate_ts_parser_language_fast_path();
    }
    result
}

// `Ord` so injection layers can be sorted by (range, language) and deduped;
// the order itself carries no meaning beyond being total and stable.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(in crate::view) enum DiffSyntaxLanguage {
    Markdown,
    MarkdownInline,
    Html,
    Vue,
    Svelte,
    /// Nunjucks, Jinja2, Twig and Django templates. One grammar parses the union
    /// of all four dialects; the HTML around the tags arrives as a combined
    /// injection (see queries/jinja_injections.scm).
    Jinja,
    /// The same grammar with the HTML injection removed, for templates whose body is
    /// not markup: `values.yaml.j2`, `deploy.sh.j2`, `nginx.conf.j2`. Template tags
    /// still highlight; only the injection is dropped.
    JinjaText,
    Css,
    Hcl,
    Bicep,
    Lua,
    Makefile,
    Nix,
    Kotlin,
    Zig,
    Groovy,
    Clojure,
    Elixir,
    Erlang,
    Haskell,
    Julia,
    /// `.ml`. Parsed by `LANGUAGE_OCAML`; see [`DiffSyntaxLanguage::OCamlInterface`]
    /// for the other half of the pair.
    OCaml,
    /// `.mli`. A separate grammar rather than a mode of the one above: an
    /// interface file is a different language shape (`val f : int -> int` has no
    /// implementation counterpart), and upstream ships it as its own
    /// `LANGUAGE_OCAML_INTERFACE`. Both share queries/ocaml_highlights.scm.
    OCamlInterface,
    Solidity,
    /// Generic assembly -- GAS and Intel-flavoured alike. The grammar is
    /// dialect-agnostic, so it labels mnemonics and operands without knowing the
    /// target architecture.
    Assembly,
    Rust,
    Python,
    JavaScript,
    Jsdoc,
    TypeScript,
    Tsx,
    Regex,
    Go,
    GoMod,
    GoWork,
    C,
    Cpp,
    ObjectiveC,
    CSharp,
    FSharp,
    VisualBasic,
    Java,
    Php,
    Ruby,
    PowerShell,
    Swift,
    R,
    Dart,
    Scala,
    Perl,
    Json,
    Toml,
    Yaml,
    Sql,
    Diff,
    GitCommit,
    Bash,
    Xml,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::view) enum DiffSyntaxMode {
    Auto,
    HeuristicOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::view) struct DiffSyntaxEdit {
    pub old_range: Range<usize>,
    pub new_range: Range<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SyntaxToken {
    pub(super) range: Range<usize>,
    pub(super) kind: SyntaxTokenKind,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) struct PreparedSyntaxDocument {
    cache_key: PreparedSyntaxCacheKey,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct PreparedSyntaxCacheKey {
    language: DiffSyntaxLanguage,
    doc_hash: u64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct PreparedSyntaxSourceIdentity {
    language: DiffSyntaxLanguage,
    text_ptr: usize,
    text_len: usize,
    line_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct SingleLineSyntaxTokenCacheKey {
    language: DiffSyntaxLanguage,
    mode: DiffSyntaxMode,
    text_hash: u64,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TreesitterParseReuseMode {
    Full,
    Incremental,
}

#[derive(Clone, Debug)]
struct PreparedSyntaxTreeState {
    language: DiffSyntaxLanguage,
    text: SharedString,
    line_starts: Arc<[usize]>,
    source_hash: u64,
    source_version: u64,
    tree: tree_sitter::Tree,
    #[cfg(test)]
    parse_mode: TreesitterParseReuseMode,
}

#[derive(Clone, Debug)]
pub(super) struct PreparedSyntaxDocumentData {
    cache_key: PreparedSyntaxCacheKey,
    line_count: usize,
    line_token_chunks: FxHashMap<usize, Vec<Arc<[SyntaxToken]>>>,
    tree_state: Option<PreparedSyntaxTreeState>,
}

#[derive(Clone, Debug)]
pub(super) struct PreparedSyntaxReparseSeed {
    document: PreparedSyntaxDocument,
    tree_state: PreparedSyntaxTreeState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::view) struct DiffSyntaxBudget {
    pub foreground_parse: Duration,
}

impl Default for DiffSyntaxBudget {
    fn default() -> Self {
        Self {
            foreground_parse: crate::ui_runtime::current().diff_syntax_foreground_parse_budget(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PrepareTreesitterDocumentResult {
    Ready(PreparedSyntaxDocument),
    TimedOut,
    Unsupported,
}

mod heuristic;
mod language;
mod live;
mod prepared;

use heuristic::*;
use language::*;
use prepared::*;

pub(super) use heuristic::syntax_tokens_for_line_heuristic_into;
#[cfg(test)]
pub(super) use language::syntax_tokens_for_line;
pub(super) use language::syntax_tokens_for_line_shared;
pub(in crate::view) use language::{
    diff_syntax_language_for_code_fence_info, diff_syntax_language_for_path,
};
pub(in crate::view) use live::{
    LiveSyntaxDocument, LiveSyntaxSnapshot, LiveSyntaxSyncOutcome, live_syntax_document_supported,
    live_syntax_reparse,
};
#[cfg(any(test, feature = "benchmarks"))]
pub(super) use prepared::has_pending_prepared_syntax_chunk_builds_for_document;
#[cfg(test)]
pub(super) use prepared::syntax_tokens_for_prepared_document_line;
pub(super) use prepared::{
    PreparedSyntaxLineTokensRequest, drain_completed_prepared_syntax_chunk_builds,
    drain_completed_prepared_syntax_chunk_builds_for_document,
    has_pending_prepared_syntax_chunk_builds, inject_prepared_document_data,
    prepare_treesitter_document_in_background_text_with_reparse_seed,
    prepare_treesitter_document_with_budget_reuse_text, prepared_document_reparse_seed,
    request_syntax_tokens_for_prepared_document_line,
    request_syntax_tokens_for_prepared_document_line_range_into,
};
#[cfg(feature = "benchmarks")]
pub(super) use prepared::{
    benchmark_cache_replacement_drop_step, benchmark_drop_payload_timed_step,
    benchmark_flush_deferred_drop_queue, benchmark_prepared_syntax_cache_contains_document,
    benchmark_prepared_syntax_cache_metrics, benchmark_prepared_syntax_loaded_chunk_count,
    benchmark_reset_prepared_syntax_cache_metrics,
};
#[cfg(test)]
pub(super) use prepared::{prepared_document_parse_mode, prepared_document_source_version};

#[cfg(test)]
pub(super) fn reset_prepared_syntax_cache() {
    prepared::reset_prepared_syntax_cache();
}

pub(super) fn syntax_tokens_for_streamed_line_slice_heuristic(
    raw_text: &worktree_core::file_diff::FileDiffLineText,
    language: DiffSyntaxLanguage,
    requested_slice_range: Range<usize>,
    resolved_slice_range: Range<usize>,
) -> Option<Vec<SyntaxToken>> {
    heuristic::syntax_tokens_for_streamed_line_slice_heuristic(
        raw_text,
        language,
        requested_slice_range,
        resolved_slice_range,
    )
}

#[cfg(test)]
pub(super) fn reset_streamed_heuristic_line_cache() {
    heuristic::reset_streamed_heuristic_line_cache();
}

#[cfg(test)]
mod tests;
