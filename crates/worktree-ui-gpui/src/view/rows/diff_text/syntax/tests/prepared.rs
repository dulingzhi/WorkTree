use super::*;

struct TempFileBackedLineFixture {
    path: std::path::PathBuf,
    raw_text: worktree_core::file_diff::FileDiffLineText,
}

impl TempFileBackedLineFixture {
    fn new(name: &str, text: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "worktree_{name}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be monotonic enough for test temp path")
                .as_nanos()
        ));
        std::fs::write(&path, text.as_bytes()).expect("write streamed slice fixture");
        let raw_text = worktree_core::file_diff::FileDiffLineText::file_slice(
            Arc::new(path.clone()),
            0..text.len(),
            false,
            false,
        );
        Self { path, raw_text }
    }
}

impl Drop for TempFileBackedLineFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn wait_for_background_chunk_build_for_document(
    document: PreparedSyntaxDocument,
    timeout: Duration,
) -> usize {
    let started = Instant::now();
    loop {
        let applied = drain_completed_prepared_syntax_chunk_builds_for_document(document);
        if applied > 0 {
            return applied;
        }
        if started.elapsed() >= timeout {
            return 0;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_all_background_chunk_builds_for_document(
    document: PreparedSyntaxDocument,
    timeout: Duration,
) -> usize {
    let started = Instant::now();
    let mut total_applied = 0usize;
    loop {
        let applied = drain_completed_prepared_syntax_chunk_builds_for_document(document);
        total_applied = total_applied.saturating_add(applied);
        if !has_pending_prepared_syntax_chunk_builds_for_document(document) {
            return total_applied;
        }
        if started.elapsed() >= timeout {
            return total_applied;
        }
        if applied == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn prepare_test_document_from_shared_text(
    language: DiffSyntaxLanguage,
    text: &str,
) -> PreparedSyntaxDocument {
    let input = treesitter_document_input_from_text(text);
    let prepared = prepare_treesitter_document_in_background_text_with_reuse(
        language,
        DiffSyntaxMode::Auto,
        SharedString::from(text.to_owned()),
        input.line_starts,
        None,
        None,
    )
    .expect("shared-text test document should parse successfully");
    inject_prepared_document_data(prepared)
}

fn prepare_markdown_document(lines: &[&str]) -> PreparedSyntaxDocument {
    prepare_test_document(DiffSyntaxLanguage::Markdown, &lines.join("\n"))
}

const ELIXIR_FIXTURE: &[&str] = &[
    /*  0 */ "defmodule Demo.Worker do",
    /*  1 */ "  @moduledoc \"Runs jobs.\"",
    /*  2 */ "",
    /*  3 */ "  def run(%{id: id} = job) when is_integer(id) do",
    /*  4 */ "    :ok",
    /*  5 */ "  end",
    /*  6 */ "end",
];

const ERLANG_FIXTURE: &[&str] = &[
    /*  0 */ "-module(demo).",
    /*  1 */ "-export([run/1]).",
    /*  2 */ "",
    /*  3 */ "%% Adds one.",
    /*  4 */ "run(X) when is_integer(X) ->",
    /*  5 */ "    Y = X + 1,",
    /*  6 */ "    {ok, Y}.",
];

const HASKELL_FIXTURE: &[&str] = &[
    /*  0 */ "module Demo.Worker (run) where",
    /*  1 */ "",
    /*  2 */ "import Data.List (foldl')",
    /*  3 */ "",
    /*  4 */ "-- | Adds one.",
    /*  5 */ "run :: Int -> Int",
    /*  6 */ "run x = foldl' (+) 0 [x, 1]",
];

const JULIA_FIXTURE: &[&str] = &[
    /*  0 */ "module Demo",
    /*  1 */ "",
    /*  2 */ "# Adds one.",
    /*  3 */ "function run(x::Int)::Int",
    /*  4 */ "    y = x + 1",
    /*  5 */ "    return y",
    /*  6 */ "end",
    /*  7 */ "",
    /*  8 */ "end",
];

const OCAML_FIXTURE: &[&str] = &[
    /*  0 */ "(* Adds one. *)",
    /*  1 */ "let run (x : int) : int =",
    /*  2 */ "  let y = x + 1 in",
    /*  3 */ "  y",
];

const OCAML_INTERFACE_FIXTURE: &[&str] = &[
    /*  0 */ "(* Adds one. *)",
    /*  1 */ "val run : int -> int",
    /*  2 */ "",
    /*  3 */ "type t = { id : int }",
];

/// Both halves of the `.ml`/`.mli` pair, because they are separate grammars
/// sharing one query file. A change that compiles against the implementation
/// grammar can still fail against the interface one -- that is exactly why
/// `(shebang)` had to come out of the vendored copy.

const CLOJURE_FIXTURE: &[&str] = &[
    /*  0 */ "(ns demo.worker)",
    /*  1 */ "",
    /*  2 */ ";; Adds one.",
    /*  3 */ "(defn run [x]",
    /*  4 */ "  (let [y (+ x 1)]",
    /*  5 */ "    {:id y :name \"demo\"}))",
];

/// Every language the batch added, with a snippet that exercises its comment
/// form, its string form and one keyword.
///
/// Shared by the invariant sweeps below so a new language is added in one place
/// and picked up by all of them.
fn batch_language_samples() -> Vec<(DiffSyntaxLanguage, &'static str)> {
    Vec::from([
        (
            DiffSyntaxLanguage::Groovy,
            "class D { def s = 'x' } // note",
        ),
        (DiffSyntaxLanguage::Clojure, "(defn f [x] \"s\") ;; note"),
        (DiffSyntaxLanguage::Elixir, "def f(x), do: \"s\" # note"),
        (DiffSyntaxLanguage::Erlang, "f(X) -> \"s\". % note"),
        (DiffSyntaxLanguage::Haskell, "f x = \"s\" -- note"),
        (DiffSyntaxLanguage::Julia, "f(x) = \"s\" # note"),
        (DiffSyntaxLanguage::OCaml, "let f x = \"s\" (* note *)"),
        (
            DiffSyntaxLanguage::OCamlInterface,
            "val f : int -> int (* note *)",
        ),
        (
            DiffSyntaxLanguage::Solidity,
            "function f() { s = \"x\"; } // note",
        ),
        (DiffSyntaxLanguage::Assembly, "    mov eax, 1 ; note"),
        (
            DiffSyntaxLanguage::Svelte,
            "<p class=\"c\">x</p> <!-- note -->",
        ),
    ])
}

/// The `potential_open_state_lead` fast-skip decides which bytes are even worth
/// examining, and a language whose comment lead is missing from it has its
/// comments run past entirely on the streamed path. Haskell's `-` had to be
/// added there when `line_comment` became None for it.
///
/// The long body is not padding. Below the checkpoint threshold the streamed
/// entry point hands the visible region straight to the per-line tokenizer, so a
/// short-line version of this test exercises the scanner not at all and passes
/// with the fast-skip entry deleted.
///
/// Each case puts the comment opener *before* the slice, so the token can only
/// be right if the scanner resumed in the comment state.

#[test]
fn large_document_query_passes_are_chunked_to_bounded_windows() {
    let lines = vec!["let value = 1;"; 8_192];
    let input = treesitter_document_input_from_text(&lines.join("\n"));
    let passes = treesitter_document_query_passes_for_line_window(
        input.line_starts.as_ref(),
        input.text.len(),
        0,
        input.line_starts.len(),
    );
    assert!(
        passes.len() > 1,
        "large document should be processed in multiple query passes"
    );
    assert!(passes.iter().all(|pass| {
        pass.byte_range.end.saturating_sub(pass.byte_range.start) <= TS_MAX_BYTES_TO_QUERY
    }));
}

#[test]
fn pathological_long_line_uses_containing_ranges_for_subpasses() {
    let long_line = format!("let value = {};", "x".repeat(TS_MAX_BYTES_TO_QUERY * 4));
    let input = treesitter_document_input_from_text(&long_line);
    let passes = treesitter_document_query_passes_for_line_window(
        input.line_starts.as_ref(),
        input.text.len(),
        0,
        input.line_starts.len(),
    );

    assert!(
        passes.len() >= 4,
        "long line should be split into multiple bounded query passes"
    );
    assert!(
        passes
            .iter()
            .all(|pass| pass.containing_byte_range.is_some()),
        "pathological line subpasses should use containing byte ranges"
    );
}

#[test]
fn streamed_ascii_json_slice_keeps_string_state_after_checkpoint() {
    const CHECKPOINT_SPACING: usize = 32 * 1024;
    reset_streamed_heuristic_line_cache();

    let payload = "x".repeat(CHECKPOINT_SPACING * 2);
    let text = format!(r#"{{"payload":"{payload}","tail":true}}"#);
    let payload_start = text.find(&payload).expect("payload should be present");
    let slice_start = payload_start + CHECKPOINT_SPACING + 137;
    let slice_end = slice_start + 256;
    let raw_text = worktree_core::file_diff::FileDiffLineText::shared(Arc::from(text));
    let (slice_text, resolved_range) = raw_text
        .slice_text_resolved(slice_start..slice_end)
        .expect("ASCII streamed slice should resolve");

    let tokens = syntax_tokens_for_streamed_line_slice_heuristic(
        &raw_text,
        DiffSyntaxLanguage::Json,
        slice_start..slice_end,
        resolved_range,
    )
    .expect("ASCII streamed slice should be supported");
    assert_token_ranges_are_utf8_safe(slice_text.as_ref(), &tokens);

    assert!(
        tokens.iter().any(|token| {
            token.kind == SyntaxTokenKind::String && token.range.start == 0 && token.range.end > 64
        }),
        "slice that starts inside the payload string should keep string highlighting: {tokens:?}"
    );
}

#[test]
fn streamed_ascii_block_comment_slice_keeps_comment_state_and_tail_tokens() {
    const CHECKPOINT_SPACING: usize = 32 * 1024;
    reset_streamed_heuristic_line_cache();

    let comment = "x".repeat(CHECKPOINT_SPACING + 192);
    let text = format!("/*{comment}*/ let value = 1;");
    let comment_start = text.find(&comment).expect("comment body should be present");
    let comment_end = comment_start + comment.len();
    let slice_start = comment_start + CHECKPOINT_SPACING;
    let slice_end = text.len();
    let raw_text = worktree_core::file_diff::FileDiffLineText::shared(Arc::from(text));
    let (slice_text, resolved_range) = raw_text
        .slice_text_resolved(slice_start..slice_end)
        .expect("ASCII streamed slice should resolve");

    let tokens = syntax_tokens_for_streamed_line_slice_heuristic(
        &raw_text,
        DiffSyntaxLanguage::Rust,
        slice_start..slice_end,
        resolved_range,
    )
    .expect("ASCII streamed slice should be supported");
    assert_token_ranges_are_utf8_safe(slice_text.as_ref(), &tokens);

    let comment_tail_len = comment_end.saturating_add(2).saturating_sub(slice_start);
    assert!(
        tokens.iter().any(|token| {
            token.kind == SyntaxTokenKind::Comment
                && token.range.start == 0
                && token.range.end >= comment_tail_len
        }),
        "slice should preserve the continued block comment: {tokens:?}"
    );
    assert!(
        tokens
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Keyword),
        "tail after the closing comment should still tokenize normally: {tokens:?}"
    );
}

#[test]
fn streamed_utf8_file_backed_json_slice_keeps_string_state_after_checkpoint() {
    const CHECKPOINT_SPACING: usize = 32 * 1024;
    reset_streamed_heuristic_line_cache();

    let payload = "x".repeat(CHECKPOINT_SPACING * 2);
    let text = format!(r#"{{"title":"Ä","payload":"{payload}","tail":true}}"#);
    let payload_start = text.find(&payload).expect("payload should be present");
    let slice_start = payload_start + CHECKPOINT_SPACING + 137;
    let slice_end = slice_start + 256;
    let fixture = TempFileBackedLineFixture::new("streamed_utf8_json_slice.json", &text);
    let (slice_text, resolved_range) = fixture
        .raw_text
        .slice_text_resolved(slice_start..slice_end)
        .expect("UTF-8 streamed slice should resolve");

    let tokens = syntax_tokens_for_streamed_line_slice_heuristic(
        &fixture.raw_text,
        DiffSyntaxLanguage::Json,
        slice_start..slice_end,
        resolved_range,
    )
    .expect("UTF-8 streamed slice should be supported");

    assert_token_ranges_are_utf8_safe(slice_text.as_ref(), &tokens);
    assert!(
        tokens.iter().any(|token| {
            token.kind == SyntaxTokenKind::String && token.range.start == 0 && token.range.end > 64
        }),
        "UTF-8 file-backed slice that starts inside the payload string should keep string highlighting: {tokens:?}"
    );
}

#[test]
fn streamed_utf8_file_backed_block_comment_slice_keeps_comment_state_and_tail_tokens() {
    const CHECKPOINT_SPACING: usize = 32 * 1024;
    reset_streamed_heuristic_line_cache();

    let comment = "x".repeat(CHECKPOINT_SPACING + 192);
    let text = format!(r#"let title = "Ä"; /*{comment}*/ let value = 1;"#);
    let comment_start = text.find(&comment).expect("comment body should be present");
    let comment_end = comment_start + comment.len();
    let slice_start = comment_start + CHECKPOINT_SPACING;
    let slice_end = text.len();
    let fixture = TempFileBackedLineFixture::new("streamed_utf8_comment_slice.rs", &text);
    let (slice_text, resolved_range) = fixture
        .raw_text
        .slice_text_resolved(slice_start..slice_end)
        .expect("UTF-8 streamed slice should resolve");

    let tokens = syntax_tokens_for_streamed_line_slice_heuristic(
        &fixture.raw_text,
        DiffSyntaxLanguage::Rust,
        slice_start..slice_end,
        resolved_range.clone(),
    )
    .expect("UTF-8 streamed slice should be supported");

    assert_token_ranges_are_utf8_safe(slice_text.as_ref(), &tokens);

    let comment_tail_len = comment_end
        .saturating_add(2)
        .saturating_sub(resolved_range.start);
    assert!(
        tokens.iter().any(|token| {
            token.kind == SyntaxTokenKind::Comment
                && token.range.start == 0
                && token.range.end >= comment_tail_len
        }),
        "UTF-8 file-backed slice should preserve the continued block comment: {tokens:?}"
    );
    assert!(
        tokens
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Keyword),
        "tail after the closing comment should still tokenize normally: {tokens:?}"
    );
}

#[test]
fn extended_path_aliases_are_supported() {
    assert_eq!(
        diff_syntax_language_for_path(".bashrc"),
        Some(DiffSyntaxLanguage::Bash)
    );
    assert_eq!(
        diff_syntax_language_for_path("PKGBUILD"),
        Some(DiffSyntaxLanguage::Bash)
    );
    assert_eq!(
        diff_syntax_language_for_path("module.cppm"),
        Some(DiffSyntaxLanguage::Cpp)
    );
    assert_eq!(
        diff_syntax_language_for_path("legacy.C"),
        Some(DiffSyntaxLanguage::Cpp)
    );
    assert_eq!(
        diff_syntax_language_for_path("legacy.H"),
        Some(DiffSyntaxLanguage::Cpp)
    );
    assert_eq!(
        diff_syntax_language_for_path("plain.c"),
        Some(DiffSyntaxLanguage::C)
    );
    assert_eq!(
        diff_syntax_language_for_path("sketch.ino"),
        Some(DiffSyntaxLanguage::Cpp)
    );
    assert_eq!(
        diff_syntax_language_for_path("styles.pcss"),
        Some(DiffSyntaxLanguage::Css)
    );
    assert_eq!(
        diff_syntax_language_for_path("types.pyi"),
        Some(DiffSyntaxLanguage::Python)
    );
    assert_eq!(
        diff_syntax_language_for_path("config.jsonc"),
        Some(DiffSyntaxLanguage::Json)
    );
    assert_eq!(
        diff_syntax_language_for_path(".prettierrc"),
        Some(DiffSyntaxLanguage::Json)
    );
    assert_eq!(
        diff_syntax_language_for_path(".clang-format"),
        Some(DiffSyntaxLanguage::Yaml)
    );
    assert_eq!(
        diff_syntax_language_for_path("README.mdx"),
        Some(DiffSyntaxLanguage::Markdown)
    );
    assert_eq!(
        diff_syntax_language_for_path("script.ps1"),
        Some(DiffSyntaxLanguage::PowerShell)
    );
    assert_eq!(
        diff_syntax_language_for_path("main.swift"),
        Some(DiffSyntaxLanguage::Swift)
    );
    assert_eq!(
        diff_syntax_language_for_path("analysis.R"),
        Some(DiffSyntaxLanguage::R)
    );
    assert_eq!(
        diff_syntax_language_for_path("app.dart"),
        Some(DiffSyntaxLanguage::Dart)
    );
    assert_eq!(
        diff_syntax_language_for_path("build.sbt"),
        Some(DiffSyntaxLanguage::Scala)
    );
    assert_eq!(
        diff_syntax_language_for_path("module.pm"),
        Some(DiffSyntaxLanguage::Perl)
    );
    assert_eq!(
        diff_syntax_language_for_path("main.m"),
        Some(DiffSyntaxLanguage::ObjectiveC)
    );
    assert_eq!(
        diff_syntax_language_for_path("changes.patch"),
        Some(DiffSyntaxLanguage::Diff)
    );
    assert_eq!(
        diff_syntax_language_for_path("COMMIT_EDITMSG"),
        Some(DiffSyntaxLanguage::GitCommit)
    );
    assert_eq!(
        diff_syntax_language_for_path("go.mod"),
        Some(DiffSyntaxLanguage::GoMod)
    );
    assert_eq!(
        diff_syntax_language_for_path("go.work"),
        Some(DiffSyntaxLanguage::GoWork)
    );
}

#[test]
fn single_line_syntax_cache_isolated_by_mode_for_xml_markup() {
    reset_ts_parser_test_state();

    let text = r#"<item enabled="true">value</item>"#;
    let auto = syntax_tokens_for_line(text, DiffSyntaxLanguage::Xml, DiffSyntaxMode::Auto);
    assert!(
        auto.iter().any(|token| {
            matches!(
                token.kind,
                SyntaxTokenKind::Tag | SyntaxTokenKind::Attribute
            )
        }),
        "tree-sitter XML mode should classify markup tokens: {auto:?}"
    );

    let heuristic =
        syntax_tokens_for_line(text, DiffSyntaxLanguage::Xml, DiffSyntaxMode::HeuristicOnly);
    assert!(
        !heuristic.iter().any(|token| {
            matches!(
                token.kind,
                SyntaxTokenKind::Tag | SyntaxTokenKind::Attribute
            )
        }),
        "heuristic XML mode should not reuse tree-sitter markup tokens: {heuristic:?}"
    );

    let auto_again = syntax_tokens_for_line(text, DiffSyntaxLanguage::Xml, DiffSyntaxMode::Auto);
    assert_eq!(auto_again, auto);
}

// ---- Heuristic fallback: Nix and Jinja ------------------------------------

/// Treating `'` as a quote painted the rest of the line as a string from the
/// tick in `foldl'` onward. HeuristicOnly is a production path for large diffs,
/// not just a fallback.

/// The positional rule must not leak into languages where `'` really does open
/// a string anywhere -- Rust byte and char literals are the sharp case.
#[test]
fn non_markup_languages_keep_unconditional_single_quote_strings() {
    assert_eq!(
        heuristic_string_spans("let b = b'x';", DiffSyntaxLanguage::Rust),
        vec!["'x'"]
    );
    assert_eq!(
        heuristic_string_spans("s = 'it''s'", DiffSyntaxLanguage::Sql),
        vec!["'it'", "'s'"]
    );
}

/// Pins a deliberate limitation rather than an achievement.
///
/// The heuristic tokenizer is per-line and has no notion of which SFC
/// section a line belongs to, so Vue has to pick one comment/keyword
/// dialect for the whole file. It is grouped with Html/Xml, which is right
/// for the template -- `<img src="//cdn/x">` must not grey out as a line
/// comment, and attributes named `class`/`for` must not render as keywords
/// -- but it means `<script>` bodies get no `//` comments and no JS
/// keywords when tree-sitter is unavailable.
///
/// This only bites the fallback paths: files over
/// TS_PREPARED_DOCUMENT_MAX_TEXT_BYTES, over-long lines, and builds without
/// `syntax-web`. If that ever stops being acceptable, the fix is section
/// tracking in the streamed heuristic state, not flipping the dialect --
/// flipping it just moves the damage into the template.

#[test]
fn single_line_syntax_cache_isolated_by_language_for_same_markup_text() {
    reset_ts_parser_test_state();

    let text = r#"<div class="demo">ok</div>"#;
    let html = syntax_tokens_for_line(text, DiffSyntaxLanguage::Html, DiffSyntaxMode::Auto);
    assert!(
        html.iter().any(|token| {
            matches!(
                token.kind,
                SyntaxTokenKind::Tag | SyntaxTokenKind::Attribute
            )
        }),
        "HTML mode should classify markup tokens: {html:?}"
    );

    let json = syntax_tokens_for_line(text, DiffSyntaxLanguage::Json, DiffSyntaxMode::Auto);
    assert!(
        !json.iter().any(|token| {
            matches!(
                token.kind,
                SyntaxTokenKind::Tag | SyntaxTokenKind::Attribute
            )
        }),
        "JSON mode should not reuse HTML markup tokens: {json:?}"
    );
    assert_ne!(json, html);

    let html_again = syntax_tokens_for_line(text, DiffSyntaxLanguage::Html, DiffSyntaxMode::Auto);
    assert_eq!(html_again, html);
}

#[test]
fn prepared_document_cache_isolated_by_language_for_same_script_markup() {
    reset_ts_parser_test_state();
    reset_prepared_syntax_cache();

    let text = "<script>\nconst value = 1;\n</script>";
    let html = prepare_test_document(DiffSyntaxLanguage::Html, text);
    let xml = prepare_test_document(DiffSyntaxLanguage::Xml, text);

    let html_tokens = syntax_tokens_for_prepared_document_line(html, 1)
        .expect("HTML script line tokens should be available");
    assert!(
        html_tokens
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Keyword),
        "HTML document should inject JavaScript keyword highlighting: {html_tokens:?}"
    );
    assert!(
        html_tokens
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Number),
        "HTML document should inject JavaScript number highlighting: {html_tokens:?}"
    );

    let xml_tokens = syntax_tokens_for_prepared_document_line(xml, 1)
        .expect("XML script line tokens should be available");
    assert!(
        !xml_tokens.iter().any(|token| {
            matches!(
                token.kind,
                SyntaxTokenKind::Keyword | SyntaxTokenKind::Number
            )
        }),
        "XML document should not reuse HTML script injection tokens: {xml_tokens:?}"
    );
    assert_ne!(xml_tokens, html_tokens);
}

#[test]
fn single_line_syntax_cache_drops_text_hash_collisions_on_text_mismatch() {
    let mut cache = SingleLineSyntaxTokenCache::new();
    let key = SingleLineSyntaxTokenCacheKey {
        language: DiffSyntaxLanguage::Html,
        mode: DiffSyntaxMode::Auto,
        text_hash: 7,
    };
    let tokens: Arc<[SyntaxToken]> = vec![SyntaxToken {
        range: 0..5,
        kind: SyntaxTokenKind::Tag,
    }]
    .into();

    cache.insert(key, "<div>", Arc::clone(&tokens));

    assert!(cache.get(key, "<span>").is_none());
    assert!(cache.by_key.is_empty());
    assert!(cache.lru_order.is_empty());
}

#[test]
fn prepared_document_preserves_multiline_treesitter_context() {
    let lines = ["/* open comment", "still comment */ let x = 1;"];
    let doc = prepare_test_document(DiffSyntaxLanguage::Rust, &lines.join("\n"));

    let first = syntax_tokens_for_prepared_document_line(doc, 0)
        .expect("prepared tokens should be available for line 0");
    let second = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("prepared tokens should be available for line 1");

    assert!(
        first.iter().any(|t| t.kind == SyntaxTokenKind::Comment),
        "first line should include comment tokens"
    );
    assert!(
        second.iter().any(|t| t.kind == SyntaxTokenKind::Comment),
        "second line should include comment tokens from multiline context"
    );
    assert!(
        second
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Comment && t.range.start == 0),
        "second line should start with comment highlighting from multiline context, got: {second:?}"
    );
}

#[test]
fn prepared_document_request_line_tokens_preserves_multiline_context() {
    let lines = ["/* open comment", "still comment */ let x = 1;"];
    let doc = prepare_test_document(DiffSyntaxLanguage::Rust, &lines.join("\n"));

    let expected = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("sync line-token lookup should materialize the continuation line chunk");

    match request_syntax_tokens_for_prepared_document_line(doc, 1) {
        Some(PreparedSyntaxLineTokensRequest::Ready(tokens)) => {
            assert!(
                tokens
                    .iter()
                    .any(|t| t.kind == SyntaxTokenKind::Comment && t.range.start == 0),
                "requested second line should start with comment highlighting from multiline context, got: {tokens:?}"
            );
            assert_eq!(
                tokens.as_ref(),
                expected.as_slice(),
                "requested prepared continuation line should match the synchronously materialized tokens"
            );
        }
        other => panic!("expected ready prepared second line, got {other:?}"),
    }
}

#[test]
fn prepared_rust_document_highlights_macro_token_tree_via_injection() {
    let text = "test_macro!(value.field::<Vec<u32>>());";
    let document = prepare_test_document(DiffSyntaxLanguage::Rust, text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("Rust macro line tokens should be available");
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::FunctionMethod, "field"),
        "Rust macro token trees should inject nested method calls: {tokens:?}"
    );
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::TypeBuiltin, "u32"),
        "Rust macro token trees should inject nested builtin types: {tokens:?}"
    );
}

#[test]
fn prepared_markdown_document_highlights_fenced_rust_block_via_injection() {
    let lines = ["```rust", "fn main() { let value = 42; }", "```"];
    let doc = prepare_markdown_document(&lines);

    let tokens = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("markdown fenced code line tokens should be available");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Keyword),
        "embedded Rust should highlight keywords inside fenced markdown, got: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Number),
        "embedded Rust should highlight numbers inside fenced markdown, got: {tokens:?}"
    );
}

#[test]
fn prepared_markdown_document_highlights_fenced_html_block_via_injection() {
    let doc = prepare_markdown_document(&["```html", "<div class=\"note\">ok</div>", "```"]);

    let tokens = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("markdown fenced HTML line tokens should be available");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Tag),
        "embedded HTML should highlight tags inside fenced markdown, got: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Attribute),
        "embedded HTML should highlight attributes inside fenced markdown, got: {tokens:?}"
    );
}

#[test]
fn prepared_markdown_document_highlights_fenced_ruby_block_via_path_alias() {
    let doc = prepare_markdown_document(&["```foo/bar/baz.rb", "if @user", "end", "```"]);

    let tokens = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("markdown fenced Ruby line tokens should be available");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Keyword),
        "Ruby path aliases in fenced markdown should highlight keywords, got: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Property),
        "Ruby path aliases in fenced markdown should highlight instance variables, got: {tokens:?}"
    );
}

#[test]
fn prepared_markdown_document_highlights_fenced_tsx_block_via_path_alias() {
    let doc = prepare_markdown_document(&[
        "```src/components/button.tsx",
        "const node = <button disabled />;",
        "```",
    ]);

    let tokens = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("markdown fenced TSX line tokens should be available");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Tag),
        "TSX path aliases in fenced markdown should highlight JSX tags, got: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Attribute),
        "TSX path aliases in fenced markdown should highlight JSX attributes, got: {tokens:?}"
    );
}

#[test]
fn prepared_markdown_document_highlights_fenced_gomod_block_via_filename_alias() {
    let line = "module example.com/project";
    let doc = prepare_markdown_document(&["```go.mod", line, "```"]);

    let tokens = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("markdown fenced go.mod line tokens should be available");
    assert!(
        has_token_kind_and_text(line, &tokens, SyntaxTokenKind::Keyword, "module"),
        "go.mod filename aliases in fenced markdown should highlight keywords, got: {tokens:?}"
    );
}

#[test]
fn prepared_markdown_document_unknown_fence_does_not_reuse_previous_language_tokens() {
    let rust_doc = prepare_markdown_document(&["```rs", "fn main() { let value = 42; }", "```"]);
    let rust_tokens = syntax_tokens_for_prepared_document_line(rust_doc, 1)
        .expect("markdown fenced Rust line tokens should be available");
    assert!(
        rust_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Keyword),
        "supported fenced Rust should highlight keywords, got: {rust_tokens:?}"
    );
    assert!(
        rust_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Number),
        "supported fenced Rust should highlight numbers, got: {rust_tokens:?}"
    );

    let unknown_doc = prepare_markdown_document(&[
        "```foo/bar/baz.unknown",
        "fn main() { let value = 42; }",
        "```",
    ]);
    let unknown_tokens = syntax_tokens_for_prepared_document_line(unknown_doc, 1)
        .expect("markdown fenced unknown-language line tokens should be available");
    assert!(
        !unknown_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Keyword),
        "unsupported fenced languages should not reuse stale Rust keyword tokens, got: {unknown_tokens:?}"
    );
    assert!(
        !unknown_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Number),
        "unsupported fenced languages should not reuse stale Rust number tokens, got: {unknown_tokens:?}"
    );
}

#[test]
fn prepared_markdown_document_highlights_inline_code_and_html_block() {
    let doc = prepare_markdown_document(&["Use `git status` here", "<div class=\"note\">ok</div>"]);

    let inline_tokens = syntax_tokens_for_prepared_document_line(doc, 0)
        .expect("markdown inline line tokens should be available");
    assert!(
        inline_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::PunctuationDelimiter),
        "markdown inline code should at least preserve delimiter highlighting, got: {inline_tokens:?}"
    );

    let html_tokens = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("markdown HTML block line tokens should be available");
    assert!(
        html_tokens.iter().any(|t| t.kind == SyntaxTokenKind::Tag),
        "markdown HTML blocks should inject HTML tag highlighting, got: {html_tokens:?}"
    );
    assert!(
        html_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Attribute),
        "markdown HTML blocks should inject HTML attribute highlighting, got: {html_tokens:?}"
    );
}

#[test]
fn prepared_html_document_highlights_style_element_contents_via_css_injection() {
    let lines = ["<style>", "body { color: red; }", "</style>"];
    let doc = prepare_html_document(&lines);

    let style_tokens = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("style line tokens should be available");
    assert!(
        style_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Property),
        "embedded CSS should highlight properties inside <style>, got: {style_tokens:?}"
    );
}

#[test]
fn prepared_html_document_highlights_script_element_contents_via_javascript_injection() {
    let lines = ["<script>", "const value = 1;", "</script>"];
    let doc = prepare_html_document(&lines);

    let script_tokens = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("script line tokens should be available");
    assert!(
        script_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Keyword),
        "embedded JavaScript should highlight keywords inside <script>, got: {script_tokens:?}"
    );
    assert!(
        script_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Number),
        "embedded JavaScript should highlight numbers inside <script>, got: {script_tokens:?}"
    );
}

#[test]
fn prepared_html_document_highlights_onclick_attribute_via_javascript_injection() {
    let lines = [r#"<button onclick="const value = 1;">go</button>"#];
    let doc = prepare_html_document(&lines);

    let tokens = syntax_tokens_for_prepared_document_line(doc, 0)
        .expect("button line tokens should be available");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Attribute),
        "root HTML tokens should still include the onclick attribute, got: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Keyword),
        "embedded JavaScript should highlight keywords inside onclick, got: {tokens:?}"
    );
}

#[test]
fn prepared_html_document_highlights_style_attribute_via_css_injection() {
    let lines = [r#"<div style="color: red; display: block">ok</div>"#];
    let doc = prepare_html_document(&lines);

    let tokens = syntax_tokens_for_prepared_document_line(doc, 0)
        .expect("div line tokens should be available");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Attribute),
        "root HTML tokens should still include the style attribute, got: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Property),
        "embedded CSS should highlight properties inside style=, got: {tokens:?}"
    );
}

#[test]
fn prepared_vue_document_highlights_template_natively() {
    // The Vue grammar inherits html, so <template> is parsed by the root
    // grammar rather than through an injection. That matters because the
    // injection engine is depth-1 only.
    let doc = prepare_vue_document(VUE_SFC_FIXTURE);

    let tokens = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("template line tokens should be available");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Tag),
        "template markup should highlight tag names, got: {tokens:?}"
    );
}

#[test]
fn prepared_vue_document_highlights_script_setup_via_typescript_injection() {
    let doc = prepare_vue_document(VUE_SFC_FIXTURE);

    let tokens = syntax_tokens_for_prepared_document_line(doc, 7)
        .expect("script line tokens should be available");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Keyword),
        "<script setup lang=\"ts\"> body should highlight keywords, got: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Number),
        "<script setup lang=\"ts\"> body should highlight numbers, got: {tokens:?}"
    );
}

#[test]
fn prepared_vue_document_highlights_scss_style_block_via_css_injection() {
    // "scss" resolves to DiffSyntaxLanguage::Css through the shared alias table.
    let doc = prepare_vue_document(VUE_SFC_FIXTURE);

    let tokens = syntax_tokens_for_prepared_document_line(doc, 11)
        .expect("style line tokens should be available");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Property),
        "<style lang=\"scss\"> body should highlight properties, got: {tokens:?}"
    );
}

#[test]
fn prepared_vue_document_highlights_interpolation_via_typescript_injection() {
    let doc = prepare_vue_document(VUE_SFC_FIXTURE);
    let kinds = token_kinds_for_line_fragment(doc, 2, VUE_SFC_FIXTURE[2], "count + 1");

    assert!(
        kinds.contains(&SyntaxTokenKind::Operator),
        "{{{{ }}}} interpolation should highlight operators, got: {kinds:?}"
    );
    assert!(
        kinds.contains(&SyntaxTokenKind::Number),
        "{{{{ }}}} interpolation should highlight numbers, got: {kinds:?}"
    );
}

#[test]
fn prepared_vue_document_highlights_directive_value_as_expression_not_string() {
    // The html base rule `(attribute_value) @string` would otherwise colour the
    // whole directive expression as a string; vue_highlights.scm overrides it
    // with @variable so the TypeScript injection shows through.
    let doc = prepare_vue_document(VUE_SFC_FIXTURE);
    let kinds = token_kinds_for_line_fragment(doc, 2, VUE_SFC_FIXTURE[2], "count > 10");

    assert!(
        kinds.contains(&SyntaxTokenKind::Operator),
        "v-if expression should highlight operators, got: {kinds:?}"
    );
    assert!(
        kinds.contains(&SyntaxTokenKind::Number),
        "v-if expression should highlight numbers, got: {kinds:?}"
    );
    assert!(
        !kinds.contains(&SyntaxTokenKind::String),
        "v-if expression must not fall back to the html string rule, got: {kinds:?}"
    );
}

#[test]
fn prepared_vue_document_highlights_directive_name_as_attribute() {
    // `@tag.attribute` has to map to Attribute; without an explicit arm the
    // dotted-suffix trimming would silently resolve it to Tag.
    let doc = prepare_vue_document(VUE_SFC_FIXTURE);
    let kinds = token_kinds_for_line_fragment(doc, 2, VUE_SFC_FIXTURE[2], "v-if");

    assert!(
        kinds.contains(&SyntaxTokenKind::Attribute),
        "directive names should highlight as attributes, got: {kinds:?}"
    );
}

#[test]
fn prepared_vue_document_highlights_plain_script_via_javascript_injection() {
    // No `lang` attribute: this falls through to the inherited html_tags rule,
    // which is guarded by `#not-match? "\\slang\\s*="` so it cannot also fire
    // for the `lang="ts"` case above.
    let lines = ["<script>", "const value = 1;", "</script>"];
    let doc = prepare_vue_document(&lines);

    let tokens = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("script line tokens should be available");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Keyword),
        "plain <script> body should highlight keywords, got: {tokens:?}"
    );
}

#[test]
fn prepared_vue_document_highlights_slot_shorthand_sigil() {
    // `#` is the v-slot shorthand. Upstream captures `:`, `.` and `@` but not
    // `#`, which leaves it as the one unstyled sigil on the tag.
    let line = r#"  <MyComp #footer="{ row }">"#;
    let doc = prepare_vue_document(&["<template>", line, "</template>"]);
    let kinds = token_kinds_for_line_fragment(doc, 1, line, "#");

    assert!(
        kinds.contains(&SyntaxTokenKind::PunctuationSpecial),
        "the v-slot `#` shorthand should highlight like `:` and `@`, got: {kinds:?}"
    );
}

/// Regression guard for the injection-per-directive blowup. Without the
/// `#not-match?` guards in vue_injections.scm every directive and every
/// interpolation became its own injected layer: ~5 per line here, which
/// overruns TS_INJECTION_CACHE_MAX_ENTRIES (32) and evicts half the cache
/// mid-render, so scrolling re-parses everything.

#[test]
fn prepared_document_cache_keeps_multiple_documents_available() {
    let first_doc = prepare_test_document(DiffSyntaxLanguage::Rust, "/* one */ let a = 1;");
    let second_doc = prepare_test_document(DiffSyntaxLanguage::Rust, "/* two */ let b = 2;");

    let first_tokens = syntax_tokens_for_prepared_document_line(first_doc, 0)
        .expect("first prepared document should remain in cache");
    let second_tokens = syntax_tokens_for_prepared_document_line(second_doc, 0)
        .expect("second prepared document should be in cache");

    assert!(
        first_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Comment),
        "first document should keep its tokens available"
    );
    assert!(
        second_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Comment),
        "second document should keep its tokens available"
    );
}

#[test]
fn prepared_document_tokens_are_chunked_and_materialized_lazily() {
    // The prepared-document cache is thread-local and persists across tests on the same worker
    // thread, so clear it before asserting exact miss/hit behavior.
    reset_prepared_syntax_cache();
    let lines = (0..(TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS * 3))
        .map(|ix| format!("let value_{ix} = {ix};"))
        .collect::<Vec<_>>();
    let document = prepare_test_document(DiffSyntaxLanguage::Rust, &lines.join("\n"));

    assert_eq!(
        prepared_syntax_loaded_chunk_count(document),
        0,
        "prepared document should start with no chunk materialization"
    );

    let _ = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("first line tokens should resolve");
    assert_eq!(
        prepared_syntax_loaded_chunk_count(document),
        1,
        "first lookup should materialize one chunk"
    );
    let after_first_lookup = prepared_syntax_cache_metrics();
    assert_eq!(after_first_lookup.miss, 1);
    assert_eq!(after_first_lookup.hit, 0);

    let _ = syntax_tokens_for_prepared_document_line(document, 1)
        .expect("same-chunk lookup should resolve");
    assert_eq!(
        prepared_syntax_loaded_chunk_count(document),
        1,
        "same chunk lookup should reuse cached chunk"
    );
    let after_second_lookup = prepared_syntax_cache_metrics();
    assert_eq!(after_second_lookup.miss, 1);
    assert_eq!(after_second_lookup.hit, 1);

    let _ = syntax_tokens_for_prepared_document_line(document, TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS)
        .expect("next-chunk lookup should resolve");
    assert_eq!(
        prepared_syntax_loaded_chunk_count(document),
        2,
        "lookup on next chunk boundary should build one additional chunk"
    );
    let after_third_lookup = prepared_syntax_cache_metrics();
    assert_eq!(after_third_lookup.miss, 2);
    assert_eq!(after_third_lookup.hit, 1);
    assert!(
        after_third_lookup.chunk_build_ms >= after_first_lookup.chunk_build_ms,
        "chunk build metric should accumulate monotonically"
    );
}

#[test]
fn prepared_document_chunk_request_builds_in_background() {
    let lines = (0..(TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS * 2))
        .map(|ix| format!("let value_{ix} = {ix};"))
        .collect::<Vec<_>>();
    let document = prepare_test_document(DiffSyntaxLanguage::Rust, &lines.join("\n"));

    assert_eq!(
        prepared_syntax_loaded_chunk_count(document),
        0,
        "prepared document should start with no chunk materialization"
    );
    assert_eq!(
        request_syntax_tokens_for_prepared_document_line(document, 0),
        Some(PreparedSyntaxLineTokensRequest::Pending),
        "first request should enqueue a background chunk build"
    );
    assert_eq!(
        prepared_syntax_loaded_chunk_count(document),
        0,
        "pending request should not materialize the chunk synchronously"
    );
    assert!(
        has_pending_prepared_syntax_chunk_builds(),
        "background chunk request should remain pending until drained"
    );

    assert!(
        wait_for_all_background_chunk_builds_for_document(document, Duration::from_secs(2)) > 0,
        "background chunk builds should complete within timeout"
    );
    assert_eq!(
        prepared_syntax_loaded_chunk_count(document),
        2,
        "first visible miss should also prefetch the adjacent chunk"
    );

    let ready = request_syntax_tokens_for_prepared_document_line(document, 0);
    match ready {
        Some(PreparedSyntaxLineTokensRequest::Ready(tokens)) => {
            assert!(
                tokens
                    .iter()
                    .any(|token| token.kind == SyntaxTokenKind::Keyword),
                "ready chunk should expose syntax tokens"
            );
        }
        other => panic!("expected ready tokens after background chunk build, got {other:?}"),
    }
    let prefetched = request_syntax_tokens_for_prepared_document_line(
        document,
        TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS,
    );
    match prefetched {
        Some(PreparedSyntaxLineTokensRequest::Ready(tokens)) => {
            assert!(
                tokens
                    .iter()
                    .any(|token| token.kind == SyntaxTokenKind::Keyword),
                "adjacent prefetched chunk should already be ready"
            );
        }
        other => panic!("expected prefetched adjacent chunk to be ready, got {other:?}"),
    }
    assert!(
        !has_pending_prepared_syntax_chunk_builds(),
        "drained chunk request should clear pending state"
    );
}

#[test]
fn prepared_document_chunk_prefetch_shares_one_tree_state_clone() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();
    reset_prepared_syntax_cache();
    let lines = (0..(TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS * 2))
        .map(|ix| format!("let value_{ix} = {ix};"))
        .collect::<Vec<_>>();
    let document = prepare_test_document(DiffSyntaxLanguage::Rust, &lines.join("\n"));

    let clones_before_request = tree_state_clone_count();
    assert_eq!(
        request_syntax_tokens_for_prepared_document_line(document, 0),
        Some(PreparedSyntaxLineTokensRequest::Pending),
        "first request should enqueue the visible chunk and its prefetched neighbor"
    );
    assert_eq!(
        tree_state_clone_count(),
        clones_before_request.saturating_add(1),
        "the queued chunk burst should share one cloned tree state"
    );
}

#[test]
fn document_scoped_chunk_drain_preserves_other_documents() {
    let lines_a = (0..TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS)
        .map(|ix| format!("let alpha_{ix} = {ix};"))
        .collect::<Vec<_>>();
    let lines_b = (0..TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS)
        .map(|ix| format!("let beta_{ix} = {ix};"))
        .collect::<Vec<_>>();
    let document_a = prepare_test_document(DiffSyntaxLanguage::Rust, &lines_a.join("\n"));
    let document_b = prepare_test_document(DiffSyntaxLanguage::Rust, &lines_b.join("\n"));

    assert_eq!(
        request_syntax_tokens_for_prepared_document_line(document_a, 0),
        Some(PreparedSyntaxLineTokensRequest::Pending)
    );
    assert_eq!(
        request_syntax_tokens_for_prepared_document_line(document_b, 0),
        Some(PreparedSyntaxLineTokensRequest::Pending)
    );
    assert!(has_pending_prepared_syntax_chunk_builds_for_document(
        document_a
    ));
    assert!(has_pending_prepared_syntax_chunk_builds_for_document(
        document_b
    ));

    assert!(
        wait_for_background_chunk_build_for_document(document_a, Duration::from_secs(2)) > 0,
        "document-scoped drain should eventually apply the requested chunk"
    );
    assert_eq!(prepared_syntax_loaded_chunk_count(document_a), 1);
    assert_eq!(
        prepared_syntax_loaded_chunk_count(document_b),
        0,
        "draining document_a should not materialize document_b"
    );
    assert!(!has_pending_prepared_syntax_chunk_builds_for_document(
        document_a
    ));
    assert!(
        has_pending_prepared_syntax_chunk_builds_for_document(document_b),
        "other document work should remain pending"
    );

    assert!(
        wait_for_background_chunk_build_for_document(document_b, Duration::from_secs(2)) > 0,
        "remaining document chunk should still be drainable afterward"
    );
    assert_eq!(prepared_syntax_loaded_chunk_count(document_b), 1);
    assert!(!has_pending_prepared_syntax_chunk_builds_for_document(
        document_b
    ));
}

#[test]
fn prepared_document_chunk_hit_does_not_clone_tree_state() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();
    reset_prepared_syntax_cache();
    let lines = (0..(TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS * 2))
        .map(|ix| format!("let chunk_clone_probe_{ix} = {ix};"))
        .collect::<Vec<_>>();
    let document = prepare_test_document(DiffSyntaxLanguage::Rust, &lines.join("\n"));

    let _ = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("first miss should resolve and build first chunk");
    let clones_after_miss = tree_state_clone_count();
    assert!(
        clones_after_miss >= 1,
        "chunk miss should clone tree state for chunk build"
    );

    let _ = syntax_tokens_for_prepared_document_line(document, 1)
        .expect("same-chunk hit should resolve");
    assert_eq!(
        tree_state_clone_count(),
        clones_after_miss,
        "chunk-hit lookup should not clone tree state"
    );
}

#[test]
fn prepared_tree_state_clones_share_source_buffers() {
    let lines = (0..128usize)
        .map(|ix| format!("let value_{ix} = {ix};"))
        .collect::<Vec<_>>();
    let document = prepare_test_document(DiffSyntaxLanguage::Rust, &lines.join("\n"));

    let (first, second) = TS_DOCUMENT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let first = cache
            .tree_state(document.cache_key)
            .expect("first tree state clone should exist");
        let second = cache
            .tree_state(document.cache_key)
            .expect("second tree state clone should exist");
        (first, second)
    });

    assert!(
        first.text.as_ptr() == second.text.as_ptr() && first.text.len() == second.text.len(),
        "tree state clones should share source text storage"
    );
    assert!(
        Arc::ptr_eq(&first.line_starts, &second.line_starts),
        "tree state clones should share line start storage"
    );
}

#[test]
fn shared_text_input_reuses_snapshot_line_start_storage() {
    let snapshot = crate::kit::text_model::TextModel::from("alpha\nbeta\ngamma").snapshot();
    let shared_line_starts = snapshot.shared_line_starts();
    let input = treesitter_document_input_from_shared_text(
        snapshot.as_shared_string(),
        shared_line_starts.clone(),
    );

    assert!(
        Arc::ptr_eq(&input.line_starts, &shared_line_starts),
        "full-text tree-sitter input should reuse snapshot line-start storage"
    );
    assert_eq!(input.line_starts.as_ref(), snapshot.line_starts());
}

#[test]
fn collected_input_last_line_content_excludes_trailing_newline() {
    let input = treesitter_document_input_from_text("alpha\nbeta");

    assert_eq!(
        line_content_end_byte(input.line_starts.as_ref(), input.text.as_bytes(), 0),
        5
    );
    assert_eq!(
        line_content_end_byte(input.line_starts.as_ref(), input.text.as_bytes(), 1),
        input.text.len(),
        "text-built input should not include trailing content beyond the last line"
    );
}

#[test]
fn shared_text_input_last_line_content_excludes_trailing_newline() {
    let snapshot = crate::kit::text_model::TextModel::from("alpha\nbeta\n").snapshot();
    let text_input = treesitter_document_input_from_text("alpha\nbeta\n");
    let input = treesitter_document_input_from_shared_text(
        snapshot.as_shared_string(),
        snapshot.shared_line_starts(),
    );

    assert_eq!(
        input.line_starts.as_ref(),
        text_input.line_starts.as_ref(),
        "shared full-text input should normalize trailing-newline line starts to the same shape as collected text input"
    );
    assert_eq!(
        line_content_end_byte(input.line_starts.as_ref(), input.text.as_bytes(), 1),
        input.text.len() - 1,
        "shared full-text input should trim the real trailing newline from the last line"
    );
}

#[test]
fn shared_text_input_preserves_real_empty_last_line_while_trimming_phantom_entry() {
    let source = "alpha\n\n";
    let snapshot = crate::kit::text_model::TextModel::from(source).snapshot();
    let input = treesitter_document_input_from_shared_text(
        snapshot.as_shared_string(),
        snapshot.shared_line_starts(),
    );

    assert_eq!(
        snapshot.line_starts(),
        &[0, 6, source.len()],
        "snapshot line starts should still include the text-model phantom trailing entry"
    );
    assert_eq!(
        input.line_starts.as_ref(),
        &[0, 6],
        "tree-sitter input should keep the real empty last line but drop the phantom trailing entry"
    );
    assert_eq!(
        line_content_end_byte(input.line_starts.as_ref(), input.text.as_bytes(), 1),
        source.len() - 1,
        "the empty last line should end before the terminal newline byte"
    );
}

#[test]
fn warm_shared_text_prepare_reuses_source_identity_without_rehashing() {
    let _lock = lock_global_counter_tests();
    reset_prepared_syntax_cache();
    reset_deferred_drop_counters();

    let source = vec!["fn warm_identity() { let value = Some(42); }"; 512].join("\n");
    let text: SharedString = source.clone().into();
    let line_starts = treesitter_document_input_from_text(&source).line_starts;
    let budget = DiffSyntaxBudget {
        foreground_parse: Duration::from_secs(1),
    };

    let first = match prepare_treesitter_document_with_budget_reuse_text(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        text.clone(),
        Arc::clone(&line_starts),
        budget,
        None,
        None,
    ) {
        PrepareTreesitterDocumentResult::Ready(document) => document,
        other => panic!("expected prepared document, got {other:?}"),
    };
    let first_hash_count = document_hash_count();
    assert!(
        first_hash_count > 0,
        "initial prepare should still hash the source at least once"
    );

    let second = match prepare_treesitter_document_with_budget_reuse_text(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        text,
        line_starts,
        budget,
        None,
        None,
    ) {
        PrepareTreesitterDocumentResult::Ready(document) => document,
        other => panic!("expected warm prepared document, got {other:?}"),
    };

    assert_eq!(second, first);
    assert_eq!(
        document_hash_count(),
        first_hash_count,
        "warm prepare should reuse the source-identity cache hit without rehashing the full text"
    );
}

#[test]
fn cold_prepare_hashes_the_source_only_once_on_cache_miss() {
    let _lock = lock_global_counter_tests();
    reset_prepared_syntax_cache();
    reset_deferred_drop_counters();

    let source = vec!["fn cold_hash_miss() { let value = Some(42); }"; 512].join("\n");
    let text: SharedString = source.clone().into();
    let line_starts = treesitter_document_input_from_text(&source).line_starts;
    let budget = DiffSyntaxBudget {
        foreground_parse: Duration::from_secs(1),
    };

    let document = match prepare_treesitter_document_with_budget_reuse_text(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        text,
        line_starts,
        budget,
        None,
        None,
    ) {
        PrepareTreesitterDocumentResult::Ready(document) => document,
        other => panic!("expected prepared document, got {other:?}"),
    };

    assert_eq!(document_hash_count(), 1);
    assert_eq!(prepared_syntax_loaded_chunk_count(document), 0);
}

#[test]
fn timed_out_prepare_reuses_pending_parse_request_in_background_without_rehashing() {
    let _lock = lock_global_counter_tests();
    reset_prepared_syntax_cache();
    reset_deferred_drop_counters();

    let source = vec!["fn background_reuse() { let value = Some(42); }"; 4_096].join("\n");
    let text: SharedString = source.clone().into();
    let line_starts = treesitter_document_input_from_text(&source).line_starts;

    let timed_out = prepare_treesitter_document_with_budget_reuse_text(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        text.clone(),
        Arc::clone(&line_starts),
        DiffSyntaxBudget {
            foreground_parse: Duration::from_millis(1),
        },
        None,
        None,
    );
    assert_eq!(timed_out, PrepareTreesitterDocumentResult::TimedOut);
    assert_eq!(
        document_hash_count(),
        1,
        "timed-out foreground prepare should hash once while storing the pending request"
    );

    let background = prepare_treesitter_document_in_background_text_with_reuse(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        text,
        line_starts,
        None,
        None,
    )
    .expect("background parse should still succeed after foreground timeout");

    assert_eq!(
        document_hash_count(),
        1,
        "background parse should reuse the pending request instead of hashing again"
    );
    assert_eq!(background.line_count, 4_096);
}

#[test]
fn oversized_shared_text_prepare_falls_back_without_prepared_tree_sitter() {
    let _lock = lock_global_counter_tests();
    reset_prepared_syntax_cache();
    reset_deferred_drop_counters();

    let line = "let oversized_value: usize = 1;";
    let repeat = (TS_PREPARED_DOCUMENT_MAX_TEXT_BYTES / (line.len() + 1)).saturating_add(1);
    let source = std::iter::repeat_n(line, repeat)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        source.len() > TS_PREPARED_DOCUMENT_MAX_TEXT_BYTES,
        "fixture should exceed the prepared full-document syntax byte gate"
    );
    let input = treesitter_document_input_from_text(&source);
    let text: SharedString = source.clone().into();

    let attempt = prepare_treesitter_document_with_budget_reuse_text(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        text.clone(),
        Arc::clone(&input.line_starts),
        DiffSyntaxBudget {
            foreground_parse: Duration::from_secs(1),
        },
        None,
        None,
    );
    assert_eq!(
        attempt,
        PrepareTreesitterDocumentResult::Unsupported,
        "oversized full-document syntax should fall back before parsing"
    );
    assert_eq!(
        document_hash_count(),
        0,
        "oversized full-document syntax should skip whole-document hash work"
    );

    let background = prepare_treesitter_document_in_background_text_with_reuse(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        text,
        input.line_starts,
        None,
        None,
    );
    assert!(
        background.is_none(),
        "background prepared syntax should also skip oversized full-document inputs"
    );
}

#[test]
fn incremental_edit_ranges_cover_the_changed_window() {
    let old = b"alpha\nbeta\ngamma\n";
    let new = b"alpha\nbeta changed\ngamma\n";
    let ranges = compute_incremental_edit_ranges(old, new);
    assert_eq!(
        ranges.len(),
        1,
        "single local edit should produce one edit range"
    );

    let edit = ranges[0];
    let mut rebuilt = Vec::new();
    rebuilt.extend_from_slice(&old[..edit.start_byte]);
    rebuilt.extend_from_slice(&new[edit.start_byte..edit.new_end_byte]);
    rebuilt.extend_from_slice(&old[edit.old_end_byte..]);
    assert_eq!(
        rebuilt.as_slice(),
        new,
        "edit range should reconstruct the new buffer when applied to old bytes"
    );
}

#[test]
fn incremental_reparse_fallback_thresholds_cover_percent_and_absolute_limits() {
    let small_edit = [TreesitterByteEditRange {
        start_byte: 100,
        old_end_byte: 120,
        new_end_byte: 128,
    }];
    assert!(
        !incremental_reparse_should_fallback(&small_edit, 4_000, 4_008),
        "small deltas should stay on incremental path"
    );

    let percent_threshold_edit = [TreesitterByteEditRange {
        start_byte: 0,
        old_end_byte: 2_000,
        new_end_byte: 2_000,
    }];
    assert!(
        incremental_reparse_should_fallback(&percent_threshold_edit, 4_000, 4_000),
        "large percent deltas should force full parse fallback"
    );

    let absolute_threshold_edit = [TreesitterByteEditRange {
        start_byte: 0,
        old_end_byte: TS_INCREMENTAL_REPARSE_MAX_CHANGED_BYTES.saturating_add(8),
        new_end_byte: TS_INCREMENTAL_REPARSE_MAX_CHANGED_BYTES.saturating_add(8),
    }];
    assert!(
        incremental_reparse_should_fallback(
            &absolute_threshold_edit,
            TS_INCREMENTAL_REPARSE_MAX_CHANGED_BYTES.saturating_add(16),
            TS_INCREMENTAL_REPARSE_MAX_CHANGED_BYTES.saturating_add(16),
        ),
        "absolute changed-byte cap should force full parse fallback"
    );
}

#[test]
fn small_reparse_reuses_old_tree_with_input_edit() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();
    let base_lines = vec!["let value = 1;".to_string(); 256];
    let base_document = prepare_test_document(DiffSyntaxLanguage::Rust, &base_lines.join("\n"));
    let base_version =
        prepared_document_source_version(base_document).expect("base source version");
    assert_eq!(
        prepared_document_parse_mode(base_document),
        Some(TreesitterParseReuseMode::Full)
    );

    let mut edited = base_lines.clone();
    edited[42].push_str(" // tiny edit");
    let attempt = prepare_test_document_with_budget_reuse(
        DiffSyntaxLanguage::Rust,
        &edited.join("\n"),
        DiffSyntaxBudget {
            foreground_parse: Duration::from_millis(50),
        },
        Some(base_document),
    );
    let PrepareTreesitterDocumentResult::Ready(reparsed_document) = attempt else {
        panic!("small reparse should complete within default budget");
    };

    assert_eq!(
        prepared_document_parse_mode(reparsed_document),
        Some(TreesitterParseReuseMode::Incremental)
    );
    let reparsed_version =
        prepared_document_source_version(reparsed_document).expect("reparsed source version");
    assert!(
        reparsed_version > base_version,
        "incremental reparse should advance source version"
    );

    let (incremental, fallback) = incremental_reparse_counters();
    assert!(
        incremental > 0,
        "small edit should use incremental reparse path"
    );
    assert_eq!(fallback, 0, "small edit should not trigger fallback");
}

#[test]
fn unchanged_reparse_reuses_old_document_without_rehashing() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();
    reset_prepared_syntax_cache();

    let source = "let value = 1;\n".repeat(256);
    let base_input = treesitter_document_input_from_text(&source);
    let PrepareTreesitterDocumentResult::Ready(base_document) =
        prepare_treesitter_document_with_budget_reuse_text(
            DiffSyntaxLanguage::Rust,
            DiffSyntaxMode::Auto,
            source.clone().into(),
            base_input.line_starts.clone(),
            DiffSyntaxBudget {
                foreground_parse: Duration::from_millis(50),
            },
            None,
            None,
        )
    else {
        panic!("base text document should parse");
    };

    reset_deferred_drop_counters();
    let repeated_input = treesitter_document_input_from_text(&source);
    let attempt = prepare_treesitter_document_with_budget_reuse_text(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        source.into(),
        repeated_input.line_starts,
        DiffSyntaxBudget {
            foreground_parse: Duration::from_millis(50),
        },
        Some(base_document),
        None,
    );
    let PrepareTreesitterDocumentResult::Ready(reused_document) = attempt else {
        panic!("unchanged reparse should reuse the existing prepared document");
    };

    assert_eq!(reused_document, base_document);
    assert_eq!(
        document_hash_count(),
        0,
        "unchanged reparses with an old document should not rehash the full source"
    );
}

#[test]
fn small_reparse_without_edit_hint_does_not_rehash_full_source() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();
    reset_prepared_syntax_cache();

    let base_text = "let value = 1;\n".repeat(256);
    let base_input = treesitter_document_input_from_text(&base_text);
    let PrepareTreesitterDocumentResult::Ready(base_document) =
        prepare_treesitter_document_with_budget_reuse_text(
            DiffSyntaxLanguage::Rust,
            DiffSyntaxMode::Auto,
            base_text.clone().into(),
            base_input.line_starts.clone(),
            DiffSyntaxBudget {
                foreground_parse: Duration::from_millis(50),
            },
            None,
            None,
        )
    else {
        panic!("base text document should parse");
    };

    let insert_offset = base_input.line_starts[42].saturating_add("let value = 1;".len());
    let mut edited_text = base_text;
    edited_text.insert_str(insert_offset, " // tiny edit");
    let edited_input = treesitter_document_input_from_text(&edited_text);

    reset_deferred_drop_counters();
    let attempt = prepare_treesitter_document_with_budget_reuse_text(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        edited_text.into(),
        edited_input.line_starts,
        DiffSyntaxBudget {
            foreground_parse: Duration::from_millis(50),
        },
        Some(base_document),
        None,
    );
    let PrepareTreesitterDocumentResult::Ready(reparsed_document) = attempt else {
        panic!("small reparse should complete within budget");
    };

    assert_eq!(
        prepared_document_parse_mode(reparsed_document),
        Some(TreesitterParseReuseMode::Incremental)
    );
    assert_eq!(
        document_hash_count(),
        0,
        "small no-hint reparses should reuse the old source fingerprint without hashing the full text"
    );
}

#[test]
fn small_reparse_reuses_cached_prefix_chunks_before_the_edit() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();
    reset_prepared_syntax_cache();

    let line_count = TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS * 3;
    let base_lines = (0..line_count)
        .map(|ix| format!("let value_{ix} = {ix};"))
        .collect::<Vec<_>>();
    let base_document = prepare_test_document(DiffSyntaxLanguage::Rust, &base_lines.join("\n"));

    let _ = syntax_tokens_for_prepared_document_line(base_document, 0)
        .expect("base document should materialize its first chunk");
    assert_eq!(
        prepared_syntax_loaded_chunk_count(base_document),
        1,
        "base document should only have its first chunk materialized"
    );

    let mut edited = base_lines.clone();
    let edited_line = TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS * 2;
    edited[edited_line].push_str(" // tiny edit");
    let attempt = prepare_test_document_with_budget_reuse(
        DiffSyntaxLanguage::Rust,
        &edited.join("\n"),
        DiffSyntaxBudget {
            foreground_parse: Duration::from_millis(50),
        },
        Some(base_document),
    );
    let PrepareTreesitterDocumentResult::Ready(reparsed_document) = attempt else {
        panic!("small reparse should complete within budget");
    };

    assert_eq!(
        prepared_document_parse_mode(reparsed_document),
        Some(TreesitterParseReuseMode::Incremental),
        "small later-line edit should stay on the incremental path"
    );
    assert_eq!(
        prepared_syntax_loaded_chunk_count(reparsed_document),
        1,
        "cached prefix chunks before the edit should carry forward to the reparsed document"
    );

    benchmark_reset_prepared_syntax_cache_metrics();
    let _ = syntax_tokens_for_prepared_document_line(reparsed_document, 0)
        .expect("reparsed document should reuse the carried prefix chunk");
    let after_prefix_hit = prepared_syntax_cache_metrics();
    assert_eq!(after_prefix_hit.hit, 1);
    assert_eq!(after_prefix_hit.miss, 0);

    let _ = syntax_tokens_for_prepared_document_line(reparsed_document, edited_line)
        .expect("changed chunk should still be buildable on demand");
    let after_changed_lookup = prepared_syntax_cache_metrics();
    assert_eq!(after_changed_lookup.hit, 1);
    assert_eq!(after_changed_lookup.miss, 1);
}

#[test]
fn small_reparse_reuses_old_tree_with_explicit_edit_hint_text_input() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();

    let base_text = "let value = 1;\n".repeat(256);
    let base_input = treesitter_document_input_from_text(&base_text);
    let base_document =
        prepare_test_document_from_shared_text(DiffSyntaxLanguage::Rust, &base_text);

    let insert_offset = base_input.line_starts[42].saturating_add("let value = 1;".len());
    let mut edited_text = base_text.clone();
    edited_text.insert_str(insert_offset, " // tiny edit");
    let edited_input = treesitter_document_input_from_text(&edited_text);
    let attempt = prepare_treesitter_document_with_budget_reuse_text(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        edited_text.into(),
        edited_input.line_starts.clone(),
        DiffSyntaxBudget {
            foreground_parse: Duration::from_millis(50),
        },
        Some(base_document),
        Some(DiffSyntaxEdit {
            old_range: insert_offset..insert_offset,
            new_range: insert_offset..insert_offset.saturating_add(" // tiny edit".len()),
        }),
    );
    let PrepareTreesitterDocumentResult::Ready(reparsed_document) = attempt else {
        panic!("explicit-edit text reparse should complete within budget");
    };

    assert_eq!(
        prepared_document_parse_mode(reparsed_document),
        Some(TreesitterParseReuseMode::Incremental),
        "explicit edit hints should keep full-text reparses on the incremental path"
    );

    let (incremental, fallback) = incremental_reparse_counters();
    assert!(
        incremental > 0,
        "explicit edit hint path should use incremental reparse"
    );
    assert_eq!(
        fallback, 0,
        "explicit edit hint should not trigger fallback"
    );
}

#[test]
fn large_reparse_falls_back_to_full_parse() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();
    let base_lines = vec!["let value = 1;".to_string(); 256];
    let base_document = prepare_test_document(DiffSyntaxLanguage::Rust, &base_lines.join("\n"));

    let mut edited = base_lines.clone();
    for line in edited.iter_mut().take(180) {
        *line = "pub fn massive_fallback_path() { let x = vec![1,2,3,4]; }".to_string();
    }
    let attempt = prepare_test_document_with_budget_reuse(
        DiffSyntaxLanguage::Rust,
        &edited.join("\n"),
        DiffSyntaxBudget {
            foreground_parse: Duration::from_millis(200),
        },
        Some(base_document),
    );
    let PrepareTreesitterDocumentResult::Ready(reparsed_document) = attempt else {
        panic!("large reparse should complete within the test full-parse budget");
    };

    assert_eq!(
        prepared_document_parse_mode(reparsed_document),
        Some(TreesitterParseReuseMode::Full)
    );
    let (_incremental, fallback) = incremental_reparse_counters();
    assert!(
        fallback > 0,
        "large edit should trigger full-parse fallback path"
    );
}

#[test]
fn large_late_edit_with_preserved_prefix_can_stay_incremental() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();

    let base_lines = (0..256)
        .map(|ix| format!("let value_{ix} = {ix}; {}", "x".repeat(96)))
        .collect::<Vec<_>>();
    let base_document = prepare_test_document(DiffSyntaxLanguage::Rust, &base_lines.join("\n"));

    let mut edited = base_lines.clone();
    for (offset, line) in edited.iter_mut().skip(96).enumerate() {
        *line = format!(
            "pub fn large_late_edit_{offset}() {{ let values = [{offset}, {offset}, {offset}, {offset}]; }} {}",
            "y".repeat(64)
        );
    }
    let attempt = prepare_test_document_with_budget_reuse(
        DiffSyntaxLanguage::Rust,
        &edited.join("\n"),
        DiffSyntaxBudget {
            foreground_parse: Duration::from_millis(200),
        },
        Some(base_document),
    );
    let PrepareTreesitterDocumentResult::Ready(reparsed_document) = attempt else {
        panic!("large later-line reparse should complete within the test budget");
    };

    assert_eq!(
        prepared_document_parse_mode(reparsed_document),
        Some(TreesitterParseReuseMode::Incremental)
    );
    let (incremental, fallback) = incremental_reparse_counters();
    assert!(
        incremental > 0,
        "later large edit should use incremental reparse"
    );
    assert_eq!(
        fallback, 0,
        "later large edit should avoid full-parse fallback"
    );
}

#[test]
fn incremental_reparse_append_line_matches_full_parse_tokens() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();

    let base_lines = vec!["let value = 41;".to_string(); 256];
    let base_document = prepare_test_document(DiffSyntaxLanguage::Rust, &base_lines.join("\n"));

    let mut edited = base_lines.clone();
    edited.push("let appended = 42;".to_string());
    let attempt = prepare_test_document_with_budget_reuse(
        DiffSyntaxLanguage::Rust,
        &edited.join("\n"),
        DiffSyntaxBudget {
            foreground_parse: Duration::from_millis(50),
        },
        Some(base_document),
    );
    let PrepareTreesitterDocumentResult::Ready(incremental_document) = attempt else {
        panic!("incremental append reparse should complete within budget");
    };
    assert_eq!(
        prepared_document_parse_mode(incremental_document),
        Some(TreesitterParseReuseMode::Incremental),
        "small EOF append should stay on incremental reparse path"
    );

    let edited_text = edited.join("\n");
    let edited_input = treesitter_document_input_from_text(&edited_text);
    let request = treesitter_document_parse_request_from_input(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        edited_input,
    )
    .expect("edited rust lines should produce parse request");
    let full_tree = with_ts_parser(&request.ts_language, |parser| {
        parse_treesitter_tree(parser, request.input.text.as_bytes(), None, None)
    })
    .flatten()
    .expect("full parse should succeed");
    let highlight =
        tree_sitter_highlight_spec(request.language).expect("rust highlight spec should exist");

    let full_tokens = collect_treesitter_document_line_tokens_for_line_window(
        &full_tree,
        highlight,
        request.input.text.as_bytes(),
        &request.input.line_starts,
        0,
        request.input.line_starts.len(),
    );
    let incremental_tokens = (0..edited.len())
        .map(|line_ix| {
            syntax_tokens_for_prepared_document_line(incremental_document, line_ix)
                .expect("incremental document should have line tokens")
        })
        .collect::<Vec<_>>();

    assert_eq!(
        incremental_tokens, full_tokens,
        "incremental append reparse should match full-parse tokenization"
    );
}

#[test]
fn large_cache_replacement_uses_deferred_drop_queue() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();

    let mut cache = TreesitterDocumentCache::new();
    cache.insert_document(
        TreesitterDocumentCache::make_test_cache_key(1),
        benchmark_line_tokens_payload(2_048, 8, 0),
    );
    let (queued_before, dropped_before, _) = deferred_drop_counters();

    cache.insert_document(
        TreesitterDocumentCache::make_test_cache_key(1),
        benchmark_line_tokens_payload(2_048, 8, 0),
    );
    let (queued_after, _, _) = deferred_drop_counters();
    assert!(
        queued_after > queued_before,
        "large replacement should enqueue deferred drop work"
    );

    assert!(
        benchmark_flush_deferred_drop_queue(),
        "deferred drop queue should flush"
    );
    let (_, dropped_after, _) = deferred_drop_counters();
    assert!(
        dropped_after > dropped_before,
        "deferred drop worker should process queued payloads"
    );
}

#[test]
fn small_cache_replacement_keeps_inline_drop_path() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();

    let mut cache = TreesitterDocumentCache::new();
    cache.insert_document(
        TreesitterDocumentCache::make_test_cache_key(1),
        benchmark_line_tokens_payload(8, 1, 0),
    );
    let (_, _, inline_before) = deferred_drop_counters();

    cache.insert_document(
        TreesitterDocumentCache::make_test_cache_key(1),
        benchmark_line_tokens_payload(8, 1, 0),
    );
    let (_, _, inline_after) = deferred_drop_counters();
    assert!(
        inline_after > inline_before,
        "small replacement should drop old payload inline"
    );
}

#[test]
fn recent_duplicate_line_tokens_reuse_existing_arcs() {
    let document =
        TreesitterCachedDocument::from_line_tokens(benchmark_line_tokens_payload(4, 8, 0), None);
    let first_chunk = document
        .line_token_chunks
        .get(&0)
        .expect("single chunk should be present");
    assert_eq!(first_chunk.len(), 4);
    assert!(
        Arc::ptr_eq(&first_chunk[0], &first_chunk[2]),
        "alternating duplicate line tokens should reuse the two-back Arc"
    );
    assert!(
        Arc::ptr_eq(&first_chunk[1], &first_chunk[3]),
        "alternating duplicate line tokens should reuse the matching recent Arc"
    );
}

#[test]
fn cached_document_drop_payload_bytes_match_flattened_chunks() {
    let mut document =
        TreesitterCachedDocument::from_chunked_line_tokens(128, FxHashMap::default(), None);
    let first_chunk = benchmark_line_tokens_payload(64, 4, 0)
        .into_iter()
        .map(Arc::from)
        .collect::<Vec<_>>();
    let second_chunk = benchmark_line_tokens_payload(64, 4, 1)
        .into_iter()
        .map(Arc::from)
        .collect::<Vec<_>>();

    insert_line_token_chunk(&mut document, 0, Some(first_chunk));
    let bytes_after_first_insert = document.line_token_bytes;
    insert_line_token_chunk(&mut document, 0, Some(second_chunk.clone()));
    assert_eq!(
        document.line_token_bytes, bytes_after_first_insert,
        "reinserting an existing chunk should not double-count drop bytes"
    );

    insert_line_token_chunk(&mut document, 1, Some(second_chunk));
    let payload = document.into_drop_payload();
    assert_eq!(
        payload.estimated_bytes,
        estimated_line_tokens_allocation_bytes(&payload.line_tokens),
        "cached drop bytes should match the flattened payload"
    );
    assert_eq!(payload.line_tokens.len(), 128);
}

#[test]
fn large_cache_eviction_uses_deferred_drop_queue() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();

    let mut cache = TreesitterDocumentCache::new();
    for key in 0..TS_DOCUMENT_CACHE_MAX_ENTRIES {
        cache.insert_document(
            TreesitterDocumentCache::make_test_cache_key(key as u64),
            benchmark_line_tokens_payload(2_048, 8, 0),
        );
    }
    let (queued_before, dropped_before, _) = deferred_drop_counters();

    cache.insert_document(
        TreesitterDocumentCache::make_test_cache_key(TS_DOCUMENT_CACHE_MAX_ENTRIES as u64 + 1),
        benchmark_line_tokens_payload(2_048, 8, 0),
    );
    let (queued_after, _, _) = deferred_drop_counters();
    assert!(
        queued_after > queued_before,
        "large eviction should enqueue deferred drop work"
    );

    assert!(
        benchmark_flush_deferred_drop_queue(),
        "deferred drop queue should flush"
    );
    let (_, dropped_after, _) = deferred_drop_counters();
    assert!(
        dropped_after > dropped_before,
        "deferred drop worker should process evicted payloads"
    );
}

#[test]
fn large_full_documents_skip_default_foreground_probe_without_reuse() {
    let text = vec!["fn parse_budget_probe() { let value = Some(42); }"; 2_048].join("\n");
    let request = treesitter_document_parse_request_from_input(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        treesitter_document_input_from_text(&text),
    )
    .expect("rust request should build");

    assert!(should_skip_budgeted_foreground_parse(
        &request,
        DiffSyntaxBudget {
            foreground_parse: DIFF_SYNTAX_FOREGROUND_PARSE_BUDGET_NON_TEST,
        },
        false,
        false,
    ));
    assert!(!should_skip_budgeted_foreground_parse(
        &request,
        DiffSyntaxBudget {
            foreground_parse: Duration::from_millis(50),
        },
        false,
        false,
    ));
    assert!(!should_skip_budgeted_foreground_parse(
        &request,
        DiffSyntaxBudget {
            foreground_parse: DIFF_SYNTAX_FOREGROUND_PARSE_BUDGET_NON_TEST,
        },
        true,
        false,
    ));
}

#[test]
fn small_full_documents_keep_default_foreground_probe() {
    let text = vec!["fn small_probe() { value += 1; }"; 256].join("\n");
    let request = treesitter_document_parse_request_from_input(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        treesitter_document_input_from_text(&text),
    )
    .expect("rust request should build");

    assert!(!should_skip_budgeted_foreground_parse(
        &request,
        DiffSyntaxBudget {
            foreground_parse: DIFF_SYNTAX_FOREGROUND_PARSE_BUDGET_NON_TEST,
        },
        false,
        false,
    ));
}

#[test]
fn background_text_reparse_reuses_old_tree_without_explicit_edit_hint() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();

    let base_text = "let value = 1;\n".repeat(256);
    let base_input = treesitter_document_input_from_text(&base_text);
    let base_document =
        prepare_test_document_from_shared_text(DiffSyntaxLanguage::Rust, &base_text);
    let base_version =
        prepared_document_source_version(base_document).expect("base source version");

    let insert_offset = base_input.line_starts[42].saturating_add("let value = 1;".len());
    let mut edited_text = base_text.clone();
    edited_text.insert_str(insert_offset, " // background tiny edit");
    let edited_input = treesitter_document_input_from_text(&edited_text);

    let prepared = prepare_treesitter_document_in_background_text_with_reuse(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        edited_text.into(),
        edited_input.line_starts.clone(),
        Some(base_document),
        None,
    )
    .expect("background text reparse should produce prepared data");
    let reparsed_document = inject_prepared_document_data(prepared);

    assert_eq!(
        prepared_document_parse_mode(reparsed_document),
        Some(TreesitterParseReuseMode::Incremental),
        "background text reparses should keep small edits on the incremental path even without explicit edit hints"
    );
    let reparsed_version =
        prepared_document_source_version(reparsed_document).expect("reparsed source version");
    assert!(
        reparsed_version > base_version,
        "background incremental reparse should advance source version"
    );

    let (incremental, fallback) = incremental_reparse_counters();
    assert!(
        incremental > 0,
        "background no-edit-hint path should use incremental reparse"
    );
    assert_eq!(
        fallback, 0,
        "background no-edit-hint path should not trigger fallback"
    );
}

#[test]
fn background_text_reparse_reuses_old_tree_with_explicit_edit_hint() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();

    let base_text = "let value = 1;\n".repeat(256);
    let base_input = treesitter_document_input_from_text(&base_text);
    let base_document =
        prepare_test_document_from_shared_text(DiffSyntaxLanguage::Rust, &base_text);
    let base_version =
        prepared_document_source_version(base_document).expect("base source version");

    let insert_offset = base_input.line_starts[42].saturating_add("let value = 1;".len());
    let mut edited_text = base_text.clone();
    edited_text.insert_str(insert_offset, " // background tiny edit");
    let edited_input = treesitter_document_input_from_text(&edited_text);

    let prepared = prepare_treesitter_document_in_background_text_with_reuse(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        edited_text.into(),
        edited_input.line_starts.clone(),
        Some(base_document),
        Some(DiffSyntaxEdit {
            old_range: insert_offset..insert_offset,
            new_range: insert_offset
                ..insert_offset.saturating_add(" // background tiny edit".len()),
        }),
    )
    .expect("background text reparse should produce prepared data");
    let reparsed_document = inject_prepared_document_data(prepared);

    assert_eq!(
        prepared_document_parse_mode(reparsed_document),
        Some(TreesitterParseReuseMode::Incremental),
        "background text reparses should keep small edits on the incremental path"
    );
    let reparsed_version =
        prepared_document_source_version(reparsed_document).expect("reparsed source version");
    assert!(
        reparsed_version > base_version,
        "background incremental reparse should advance source version"
    );

    let (incremental, fallback) = incremental_reparse_counters();
    assert!(
        incremental > 0,
        "background explicit edit hint path should use incremental reparse"
    );
    assert_eq!(
        fallback, 0,
        "background explicit edit hint should not trigger fallback"
    );
}

#[test]
fn background_seed_reuses_cached_prefix_chunks_before_large_edit_fallback() {
    let _lock = lock_global_counter_tests();
    reset_deferred_drop_counters();
    reset_prepared_syntax_cache();

    let line_count = TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS * 4;
    let base_lines = (0..line_count)
        .map(|ix| format!("let value_{ix} = {ix};"))
        .collect::<Vec<_>>();
    let base_document = prepare_test_document(DiffSyntaxLanguage::Rust, &base_lines.join("\n"));

    let _ = syntax_tokens_for_prepared_document_line(base_document, 0)
        .expect("base document should materialize its first chunk");
    assert_eq!(
        prepared_syntax_loaded_chunk_count(base_document),
        1,
        "base document should only have its first chunk materialized"
    );

    let reparse_seed =
        prepared_document_reparse_seed(base_document).expect("base document should expose a seed");
    let mut edited = base_lines.clone();
    let first_changed_line = TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS * 2;
    for (offset, line) in edited.iter_mut().skip(first_changed_line).enumerate() {
        *line = format!(
            "pub fn fallback_edit_{offset}() {{ let values = [{offset}, {offset}, {offset}, {offset}]; }}"
        );
    }
    let edited_text = edited.join("\n");
    let edited_input = treesitter_document_input_from_text(&edited_text);

    let prepared = prepare_treesitter_document_in_background_text_with_reparse_seed(
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
        edited_text.into(),
        edited_input.line_starts,
        Some(reparse_seed),
        None,
    )
    .expect("background large-edit reparse should produce prepared data");
    let reparsed_document = inject_prepared_document_data(prepared);

    assert_eq!(
        prepared_document_parse_mode(reparsed_document),
        Some(TreesitterParseReuseMode::Full),
        "large edit should still take the full-parse fallback path"
    );
    assert_eq!(
        prepared_syntax_loaded_chunk_count(reparsed_document),
        1,
        "background reparse seed should preserve cached prefix chunks before the edit"
    );

    benchmark_reset_prepared_syntax_cache_metrics();
    let _ = syntax_tokens_for_prepared_document_line(reparsed_document, 0)
        .expect("reparsed document should reuse the preserved prefix chunk");
    let after_prefix_hit = prepared_syntax_cache_metrics();
    assert_eq!(after_prefix_hit.hit, 1);
    assert_eq!(after_prefix_hit.miss, 0);
}

#[test]
fn background_prepared_document_not_in_tls_until_injected() {
    let text = "/* background comment */\nlet value = 42;".to_string();
    let prepared = std::thread::spawn({
        let text = text.clone();
        move || {
            let input = treesitter_document_input_from_text(&text);
            prepare_treesitter_document_in_background_text_with_reuse(
                DiffSyntaxLanguage::Rust,
                DiffSyntaxMode::Auto,
                SharedString::from(text),
                input.line_starts,
                None,
                None,
            )
            .expect("background parse should produce prepared data")
        }
    })
    .join()
    .expect("background parse thread should not panic");

    let unresolved_handle = PreparedSyntaxDocument {
        cache_key: prepared.cache_key,
    };
    assert!(
        syntax_tokens_for_prepared_document_line(unresolved_handle, 0).is_none(),
        "background parse must not populate main-thread TLS cache until injected"
    );

    let document = inject_prepared_document_data(prepared);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("injected background document should have tokens");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Comment),
        "injected document should include parsed comment tokens"
    );
}

#[test]
fn extra_languages_capture_basic_semantic_tokens() {
    let cases = [
        (
            DiffSyntaxLanguage::C,
            "int main(void) { return 0; }",
            SyntaxTokenKind::Function,
        ),
        (
            DiffSyntaxLanguage::Cpp,
            "auto value = std::vector<int>{1, 2};",
            SyntaxTokenKind::Type,
        ),
        (
            DiffSyntaxLanguage::CSharp,
            "public class Example { string Name { get; } }",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::Bicep,
            "param location string = 'westeurope'",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::ObjectiveC,
            "NSString *value = @\"hi\";",
            SyntaxTokenKind::Property,
        ),
        (
            DiffSyntaxLanguage::FSharp,
            "let value = 42",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::Java,
            "class Example { int value() { return 1; } }",
            SyntaxTokenKind::FunctionMethod,
        ),
        (
            DiffSyntaxLanguage::Php,
            "<?php function foo(): int { return 1; }",
            SyntaxTokenKind::Function,
        ),
        (
            DiffSyntaxLanguage::Ruby,
            "class Example; def call(name) = 42 end",
            SyntaxTokenKind::FunctionMethod,
        ),
        (
            DiffSyntaxLanguage::PowerShell,
            "function Invoke-Test { return 42 }",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::Swift,
            "struct Example { let value = 42 }",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::R,
            "if (TRUE) print(1)",
            SyntaxTokenKind::Boolean,
        ),
        (
            DiffSyntaxLanguage::Dart,
            "class Example { int value() => 42; }",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::Scala,
            "object Example { def run(): Int = 42 }",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::Toml,
            "enabled = true",
            SyntaxTokenKind::Property,
        ),
        (
            DiffSyntaxLanguage::Lua,
            "local value = 42",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::Kotlin,
            "class Example { fun run() = 42 }",
            SyntaxTokenKind::Function,
        ),
        (
            DiffSyntaxLanguage::Zig,
            "const value: u32 = 42;",
            SyntaxTokenKind::TypeBuiltin,
        ),
        (
            DiffSyntaxLanguage::Sql,
            "select name from users",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::Groovy,
            "class Example { def run() { return 42 } }",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::Clojure,
            "(defn run [] 42)",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::Elixir,
            "defmodule Example do end",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::Erlang,
            "run(X) -> X + 1.",
            SyntaxTokenKind::Function,
        ),
        (
            DiffSyntaxLanguage::Haskell,
            "run :: Int -> Int",
            SyntaxTokenKind::Type,
        ),
        (
            DiffSyntaxLanguage::Julia,
            "function run(x) x + 1 end",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::OCaml,
            "let run x = x + 1",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::OCamlInterface,
            "val run : int -> int",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::Solidity,
            "contract Example { uint256 value; }",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::Assembly,
            "  mov eax, 1",
            SyntaxTokenKind::Number,
        ),
        (
            DiffSyntaxLanguage::Svelte,
            "<button class=\"go\">go</button>",
            SyntaxTokenKind::Tag,
        ),
    ];

    for (language, text, expected_kind) in cases {
        let tokens = syntax_tokens_for_line(text, language, DiffSyntaxMode::Auto);
        assert!(
            tokens.iter().any(|token| token.kind == expected_kind),
            "{language:?} should capture {expected_kind:?}: {tokens:?}"
        );
    }
}

#[test]
fn repo_languages_capture_basic_semantic_tokens() {
    let cases = [
        (
            DiffSyntaxLanguage::GoMod,
            "module example.com/project",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::GoWork,
            "use ./module",
            SyntaxTokenKind::Keyword,
        ),
        (
            DiffSyntaxLanguage::Diff,
            "diff --git a/src/lib.rs b/src/lib.rs",
            SyntaxTokenKind::VariableBuiltin,
        ),
        (
            DiffSyntaxLanguage::GitCommit,
            "feat: widen syntax support",
            SyntaxTokenKind::MarkupHeading,
        ),
    ];

    for (language, text, expected_kind) in cases {
        let tokens = syntax_tokens_for_line(text, language, DiffSyntaxMode::Auto);
        assert!(
            tokens.iter().any(|token| token.kind == expected_kind),
            "{language:?} should capture {expected_kind:?}: {tokens:?}"
        );
    }
}

#[test]
fn prepared_documents_capture_markup_specific_tokens() {
    let gitcommit = prepare_test_document(DiffSyntaxLanguage::GitCommit, "Subject\n\ncloses #123");

    let heading = syntax_tokens_for_prepared_document_line(gitcommit, 0)
        .expect("gitcommit subject line should have prepared tokens");
    assert!(
        heading
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::MarkupHeading),
        "gitcommit subject should produce MarkupHeading token, got: {heading:?}"
    );

    let link = syntax_tokens_for_prepared_document_line(gitcommit, 2)
        .expect("gitcommit body line should have prepared tokens");
    assert!(
        link.iter().any(|t| t.kind == SyntaxTokenKind::MarkupLink),
        "gitcommit issue reference should produce MarkupLink token, got: {link:?}"
    );

    let xml = prepare_test_document(DiffSyntaxLanguage::Xml, "<root><![CDATA[code]]></root>");
    let literal = syntax_tokens_for_prepared_document_line(xml, 0)
        .expect("XML CDATA line should have prepared tokens");
    assert!(
        literal
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::TextLiteral),
        "XML CDATA should produce TextLiteral token, got: {literal:?}"
    );
}

/// Every extension and fence alias the batch claims, plus the collisions the
/// mapping had to avoid. Path resolution is the only thing standing between a
/// wired-up grammar and a file that still renders as plain text, and nothing
/// else in the suite exercises these arms.
#[test]
fn batch_language_paths_and_fences_resolve() {
    let cases: &[(&str, DiffSyntaxLanguage)] = &[
        ("src/Demo.groovy", DiffSyntaxLanguage::Groovy),
        ("build.gradle", DiffSyntaxLanguage::Groovy),
        ("Jenkinsfile", DiffSyntaxLanguage::Groovy),
        ("src/demo/core.clj", DiffSyntaxLanguage::Clojure),
        ("src/demo/core.cljs", DiffSyntaxLanguage::Clojure),
        ("deps.edn", DiffSyntaxLanguage::Clojure),
        ("lib/demo/worker.ex", DiffSyntaxLanguage::Elixir),
        ("test/demo_test.exs", DiffSyntaxLanguage::Elixir),
        ("src/demo.erl", DiffSyntaxLanguage::Erlang),
        ("include/demo.hrl", DiffSyntaxLanguage::Erlang),
        ("rebar.config", DiffSyntaxLanguage::Erlang),
        ("src/Demo/Worker.hs", DiffSyntaxLanguage::Haskell),
        ("src/demo.jl", DiffSyntaxLanguage::Julia),
        ("lib/demo.ml", DiffSyntaxLanguage::OCaml),
        ("lib/demo.mli", DiffSyntaxLanguage::OCamlInterface),
        ("contracts/Demo.sol", DiffSyntaxLanguage::Solidity),
        ("src/boot.asm", DiffSyntaxLanguage::Assembly),
        ("src/memcpy.s", DiffSyntaxLanguage::Assembly),
        // `.S` is preprocessed assembly; the lowercasing in
        // `diff_syntax_language_for_path` is what makes it land here.
        ("src/entry.S", DiffSyntaxLanguage::Assembly),
        ("src/App.svelte", DiffSyntaxLanguage::Svelte),
    ];
    for (path, expected) in cases {
        assert_eq!(
            diff_syntax_language_for_path(path),
            Some(*expected),
            "{path} should resolve to {expected:?}"
        );
    }

    for (fence, expected) in [
        ("groovy", DiffSyntaxLanguage::Groovy),
        ("clojure", DiffSyntaxLanguage::Clojure),
        ("elixir", DiffSyntaxLanguage::Elixir),
        ("erlang", DiffSyntaxLanguage::Erlang),
        ("haskell", DiffSyntaxLanguage::Haskell),
        ("julia", DiffSyntaxLanguage::Julia),
        ("ocaml", DiffSyntaxLanguage::OCaml),
        ("solidity", DiffSyntaxLanguage::Solidity),
        ("asm", DiffSyntaxLanguage::Assembly),
        ("svelte", DiffSyntaxLanguage::Svelte),
    ] {
        assert_eq!(
            diff_syntax_language_for_code_fence_info(fence),
            Some(expected),
            "```{fence} should resolve to {expected:?}"
        );
    }
}

/// The three collisions the batch had to route around. Each one is a silent
/// regression if the identifier table is ever reordered: the file still
/// highlights, just as the wrong language.

/// The three collisions the batch had to route around. Each one is a silent
/// regression if the identifier table is ever reordered: the file still
/// highlights, just as the wrong language.
#[test]
fn batch_language_extensions_do_not_steal_existing_ones() {
    // `.gradle.kts` is Kotlin. The extension pass sees `kts` and never reaches
    // the `gradle` arm.
    assert_eq!(
        diff_syntax_language_for_path("build.gradle.kts"),
        Some(DiffSyntaxLanguage::Kotlin),
    );

    // `.m` stays Objective-C: the new `.ml` arm is one character away from it,
    // and an extension table is edited by hand.
    assert_eq!(
        diff_syntax_language_for_path("src/Demo.m"),
        Some(DiffSyntaxLanguage::ObjectiveC),
    );

    // `.ml` is OCaml, and must not be confused with the `.m` above.
    assert_eq!(
        diff_syntax_language_for_path("lib/demo.ml"),
        Some(DiffSyntaxLanguage::OCaml),
    );
}

#[test]
fn prepared_elixir_document_highlights_core_syntax() {
    let doc = prepare_test_document(DiffSyntaxLanguage::Elixir, &ELIXIR_FIXTURE.join("\n"));

    for (line_ix, fragment, expected) in [
        (0usize, "defmodule", SyntaxTokenKind::Keyword),
        (0, "Demo.Worker", SyntaxTokenKind::Namespace),
        (3, "when", SyntaxTokenKind::Keyword),
        (5, "end", SyntaxTokenKind::Keyword),
    ] {
        let kinds = token_kinds_for_line_fragment(doc, line_ix, ELIXIR_FIXTURE[line_ix], fragment);
        assert!(
            kinds.contains(&expected),
            "`{fragment}` should be {expected:?}: {kinds:?}"
        );
    }

    // Atoms are the shape that separates Elixir from a curly-brace language.
    let atom = token_kinds_for_line_fragment(doc, 4, ELIXIR_FIXTURE[4], ":ok");
    assert!(
        atom.contains(&SyntaxTokenKind::StringSpecial),
        "`:ok` is an atom, not an identifier: {atom:?}"
    );
}

#[test]
fn prepared_erlang_document_highlights_core_syntax() {
    let doc = prepare_test_document(DiffSyntaxLanguage::Erlang, &ERLANG_FIXTURE.join("\n"));

    let directive = token_kinds_for_line_fragment(doc, 0, ERLANG_FIXTURE[0], "module");
    assert!(
        directive.contains(&SyntaxTokenKind::Keyword),
        "`-module` is a directive: {directive:?}"
    );

    // `%` is Erlang's line comment and nothing else in the tree uses it.
    let comment = token_kinds_for_line_fragment(doc, 3, ERLANG_FIXTURE[3], "Adds one");
    assert!(
        comment.contains(&SyntaxTokenKind::Comment),
        "`%%` starts an Erlang comment: {comment:?}"
    );

    let guard = token_kinds_for_line_fragment(doc, 4, ERLANG_FIXTURE[4], "when");
    assert!(
        guard.contains(&SyntaxTokenKind::Keyword),
        "`when` introduces a guard: {guard:?}"
    );

    let number = token_kinds_for_line_fragment(doc, 5, ERLANG_FIXTURE[5], "1");
    assert!(
        number.contains(&SyntaxTokenKind::Number),
        "`1` is a number: {number:?}"
    );
}

#[test]
fn prepared_haskell_document_highlights_core_syntax() {
    let doc = prepare_test_document(DiffSyntaxLanguage::Haskell, &HASKELL_FIXTURE.join("\n"));

    for (line_ix, fragment, expected) in [
        (0usize, "module", SyntaxTokenKind::Keyword),
        (0, "where", SyntaxTokenKind::Keyword),
        (2, "import", SyntaxTokenKind::Keyword),
        (5, "Int", SyntaxTokenKind::Type),
    ] {
        let kinds = token_kinds_for_line_fragment(doc, line_ix, HASKELL_FIXTURE[line_ix], fragment);
        assert!(
            kinds.contains(&expected),
            "`{fragment}` should be {expected:?}: {kinds:?}"
        );
    }

    // Haddock's `-- |` is a documentation comment, not a plain one.
    let haddock = token_kinds_for_line_fragment(doc, 4, HASKELL_FIXTURE[4], "Adds one");
    assert!(
        haddock.contains(&SyntaxTokenKind::CommentDoc),
        "`-- |` opens a Haddock comment: {haddock:?}"
    );
}

#[test]
fn prepared_julia_document_highlights_core_syntax() {
    let doc = prepare_test_document(DiffSyntaxLanguage::Julia, &JULIA_FIXTURE.join("\n"));

    let comment = token_kinds_for_line_fragment(doc, 2, JULIA_FIXTURE[2], "Adds one");
    assert!(
        comment.contains(&SyntaxTokenKind::Comment),
        "`#` is Julia's line comment: {comment:?}"
    );

    for (line_ix, fragment, expected) in [
        (3usize, "function", SyntaxTokenKind::Keyword),
        (3, "Int", SyntaxTokenKind::TypeBuiltin),
        (5, "return", SyntaxTokenKind::Keyword),
    ] {
        let kinds = token_kinds_for_line_fragment(doc, line_ix, JULIA_FIXTURE[line_ix], fragment);
        assert!(
            kinds.contains(&expected),
            "`{fragment}` should be {expected:?}: {kinds:?}"
        );
    }
}

/// Both halves of the `.ml`/`.mli` pair, because they are separate grammars
/// sharing one query file. A change that compiles against the implementation
/// grammar can still fail against the interface one -- that is exactly why
/// `(shebang)` had to come out of the vendored copy.
#[test]
fn prepared_ocaml_documents_highlight_both_halves_of_the_pair() {
    let ml = prepare_test_document(DiffSyntaxLanguage::OCaml, &OCAML_FIXTURE.join("\n"));

    let comment = token_kinds_for_line_fragment(ml, 0, OCAML_FIXTURE[0], "Adds one");
    assert!(
        comment.contains(&SyntaxTokenKind::Comment),
        "`(* *)` is OCaml's only comment form: {comment:?}"
    );
    for (line_ix, fragment, expected) in [
        (1usize, "let", SyntaxTokenKind::Keyword),
        (1, "int", SyntaxTokenKind::TypeBuiltin),
        (2, "in", SyntaxTokenKind::Keyword),
    ] {
        let kinds = token_kinds_for_line_fragment(ml, line_ix, OCAML_FIXTURE[line_ix], fragment);
        assert!(
            kinds.contains(&expected),
            "`{fragment}` should be {expected:?} in a .ml: {kinds:?}"
        );
    }

    let mli = prepare_test_document(
        DiffSyntaxLanguage::OCamlInterface,
        &OCAML_INTERFACE_FIXTURE.join("\n"),
    );
    for (line_ix, fragment, expected) in [
        (1usize, "val", SyntaxTokenKind::Keyword),
        (3, "type", SyntaxTokenKind::Keyword),
        (3, "id", SyntaxTokenKind::Property),
        (3, "int", SyntaxTokenKind::TypeBuiltin),
    ] {
        let kinds =
            token_kinds_for_line_fragment(mli, line_ix, OCAML_INTERFACE_FIXTURE[line_ix], fragment);
        assert!(
            kinds.contains(&expected),
            "`{fragment}` should be {expected:?} in a .mli: {kinds:?}"
        );
    }
}

/// The reason queries/ocaml_highlights.scm exists rather than a reference to
/// `tree_sitter_ocaml::HIGHLIGHTS_QUERY`: upstream names `(shebang)`, which the
/// interface grammar has no rule for, and one unknown node type fails the whole
/// query rather than the pattern that names it.

#[test]
fn prepared_groovy_document_highlights_core_syntax() {
    let doc = prepare_test_document(DiffSyntaxLanguage::Groovy, &GROOVY_FIXTURE.join("\n"));

    let comment = token_kinds_for_line_fragment(doc, 0, GROOVY_FIXTURE[0], "Build config");
    assert!(
        comment.contains(&SyntaxTokenKind::Comment),
        "`//` is a Groovy line comment: {comment:?}"
    );

    // Single quotes are a plain string in Groovy, unlike the Haskell/OCaml/
    // Clojure arms added alongside it.
    let string = token_kinds_for_line_fragment(doc, 2, GROOVY_FIXTURE[2], "'java'");
    assert!(
        string.contains(&SyntaxTokenKind::String),
        "`'java'` is a string: {string:?}"
    );

    for (line_ix, fragment, expected) in [
        (5usize, "class", SyntaxTokenKind::Keyword),
        (6, "static", SyntaxTokenKind::Keyword),
        (6, "int", SyntaxTokenKind::TypeBuiltin),
        (7, "return", SyntaxTokenKind::KeywordControl),
    ] {
        let kinds = token_kinds_for_line_fragment(doc, line_ix, GROOVY_FIXTURE[line_ix], fragment);
        assert!(
            kinds.contains(&expected),
            "`{fragment}` should be {expected:?}: {kinds:?}"
        );
    }
}

#[test]
fn prepared_clojure_document_highlights_core_syntax() {
    let doc = prepare_test_document(DiffSyntaxLanguage::Clojure, &CLOJURE_FIXTURE.join("\n"));

    let comment = token_kinds_for_line_fragment(doc, 2, CLOJURE_FIXTURE[2], "Adds one");
    assert!(
        comment.contains(&SyntaxTokenKind::Comment),
        "`;;` is Clojure's line comment: {comment:?}"
    );

    // The head-position rules in queries/clojure_highlights.scm. Upstream's
    // six-pattern query has none of this: without them a Clojure file is
    // literals and nothing else.
    for (line_ix, fragment, expected) in [
        (0usize, "ns", SyntaxTokenKind::Keyword),
        (3, "defn", SyntaxTokenKind::Keyword),
        (3, "run", SyntaxTokenKind::Function),
        (4, "let", SyntaxTokenKind::Keyword),
    ] {
        let kinds = token_kinds_for_line_fragment(doc, line_ix, CLOJURE_FIXTURE[line_ix], fragment);
        assert!(
            kinds.contains(&expected),
            "`{fragment}` should be {expected:?}: {kinds:?}"
        );
    }

    let keyword_literal = token_kinds_for_line_fragment(doc, 5, CLOJURE_FIXTURE[5], ":id");
    assert!(
        keyword_literal.contains(&SyntaxTokenKind::Constant),
        "`:id` is a keyword literal: {keyword_literal:?}"
    );
    let string = token_kinds_for_line_fragment(doc, 5, CLOJURE_FIXTURE[5], "\"demo\"");
    assert!(
        string.contains(&SyntaxTokenKind::String),
        "`\"demo\"` is a string: {string:?}"
    );
}

/// The quoting literals span the whole quoted form, so capturing the *node*
/// paints `'(alpha beta)` end to end. Upstream captures the one-character
/// marker instead. Capturing the node is cheap to reintroduce and invisible
/// without a test.

#[test]
fn prepared_solidity_document_highlights_core_syntax() {
    let doc = prepare_test_document(DiffSyntaxLanguage::Solidity, &SOLIDITY_FIXTURE.join("\n"));

    let comment = token_kinds_for_line_fragment(doc, 0, SOLIDITY_FIXTURE[0], "SPDX");
    assert!(
        comment.contains(&SyntaxTokenKind::Comment),
        "the SPDX header is a comment: {comment:?}"
    );

    for (line_ix, fragment, expected) in [
        (1usize, "pragma", SyntaxTokenKind::Keyword),
        (3, "contract", SyntaxTokenKind::Keyword),
        (4, "uint256", SyntaxTokenKind::Type),
        (6, "function", SyntaxTokenKind::Keyword),
        (6, "returns", SyntaxTokenKind::Keyword),
        (8, "return", SyntaxTokenKind::Keyword),
    ] {
        let kinds =
            token_kinds_for_line_fragment(doc, line_ix, SOLIDITY_FIXTURE[line_ix], fragment);
        assert!(
            kinds.contains(&expected),
            "`{fragment}` should be {expected:?}: {kinds:?}"
        );
    }
}

/// If a grammar bump ships a query that compiles as-is, the vendored copy and
/// this test can both go.

#[test]
fn prepared_assembly_document_highlights_core_syntax() {
    let doc = prepare_test_document(DiffSyntaxLanguage::Assembly, &ASSEMBLY_FIXTURE.join("\n"));

    for (line_ix, fragment, expected) in [
        (2usize, "run", SyntaxTokenKind::Label),
        (3, "mov", SyntaxTokenKind::Function),
        (3, "eax", SyntaxTokenKind::VariableBuiltin),
        (3, "1", SyntaxTokenKind::Number),
        (5, "ret", SyntaxTokenKind::Function),
    ] {
        let kinds =
            token_kinds_for_line_fragment(doc, line_ix, ASSEMBLY_FIXTURE[line_ix], fragment);
        assert!(
            kinds.contains(&expected),
            "`{fragment}` should be {expected:?}: {kinds:?}"
        );
    }

    // Trailing comments are the only comment position this grammar accepts;
    // see `assembly_standalone_comment_lines_fall_back_to_the_heuristic`.
    let comment = token_kinds_for_line_fragment(doc, 3, ASSEMBLY_FIXTURE[3], "; seed");
    assert!(
        comment.contains(&SyntaxTokenKind::Comment),
        "a trailing `;` comment should be greyed out: {comment:?}"
    );
}

/// A documented limitation, not a bug in the wiring: tree-sitter-asm only
/// admits a comment after an instruction, so a comment on its own line -- which
/// is most comments in real assembly -- puts the tree into error recovery.
///
/// Recovery is survivable (the instructions around it still highlight) and the
/// heuristic path, which this repo also runs for short lines and oversized
/// diffs, gets it right. The test pins both halves so a grammar bump that fixes
/// the parse shows up here rather than going unnoticed.

#[test]
fn prepared_svelte_document_highlights_markup_and_block_tags() {
    let doc = prepare_test_document(DiffSyntaxLanguage::Svelte, &SVELTE_FIXTURE.join("\n"));

    // The html base embedded in queries/svelte_highlights.scm. Without it the
    // upstream query colours the block markers and leaves the markup plain.
    for (line_ix, fragment, expected) in [
        (0usize, "script", SyntaxTokenKind::Tag),
        (0, "lang", SyntaxTokenKind::Attribute),
        (5, "button", SyntaxTokenKind::Tag),
        (5, "class", SyntaxTokenKind::Attribute),
        (5, "btn", SyntaxTokenKind::String),
    ] {
        let kinds = token_kinds_for_line_fragment(doc, line_ix, SVELTE_FIXTURE[line_ix], fragment);
        assert!(
            kinds.contains(&expected),
            "`{fragment}` should be {expected:?}: {kinds:?}"
        );
    }

    // The svelte half: `{#if}` / `{:else}` / `{/if}`.
    for (line_ix, fragment) in [(4usize, "if"), (6, "else"), (8, "if")] {
        let kinds = token_kinds_for_line_fragment(doc, line_ix, SVELTE_FIXTURE[line_ix], fragment);
        assert!(
            kinds.contains(&SyntaxTokenKind::Keyword),
            "the `{{{fragment}}}` block marker should be a keyword: {kinds:?}"
        );
    }
}

/// The script and style bodies are the bulk of a `.svelte` file and neither is
/// reachable from the highlights query -- they arrive as injections or not at
/// all. The `lang="ts"` veto is what keeps the default javascript rule from
/// firing over the same `raw_text`; see the note in svelte_injections.scm.

/// Three of the new languages spell something other than a string with `'`:
/// Haskell primes identifiers, OCaml opens type variables, Clojure quotes
/// forms. Left as `HeuristicSingleQuote::String` each one runs a string from
/// the tick to the end of the line -- the Nix bug, three more times.
#[test]
fn batch_apostrophes_do_not_open_a_string() {
    for (line, language) in [
        ("run xs = foldl' (+) 0 xs", DiffSyntaxLanguage::Haskell),
        ("let ids : 'a list = []", DiffSyntaxLanguage::OCaml),
        (
            "val map : ('a -> 'b) -> 'a list -> 'b list",
            DiffSyntaxLanguage::OCamlInterface,
        ),
        ("(def syms '(alpha beta))", DiffSyntaxLanguage::Clojure),
    ] {
        assert!(
            heuristic_string_spans(line, language).is_empty(),
            "an apostrophe opened a string in {language:?} line {line:?}: {:?}",
            heuristic_tokens(line, language)
        );
    }

    // Double quotes still work everywhere.
    assert_eq!(
        heuristic_string_spans("  name = \"demo\"", DiffSyntaxLanguage::Haskell),
        vec!["\"demo\""]
    );
}

/// Julia is the one language in the batch where `'` is both: `A'` is the
/// adjoint operator and `'c'` is a character literal. ValuePositionOnly tells
/// them apart by what precedes the tick.

/// Every comment form the batch introduced. The heuristic runs in production
/// for lines past MAX_TREESITTER_LINE_BYTES and in HeuristicOnly mode, and
/// these arms are reached by nothing else.
#[test]
fn batch_heuristic_comment_forms_are_covered() {
    for (line, language) in [
        ("-- | Adds one.", DiffSyntaxLanguage::Haskell),
        ("{- block -}", DiffSyntaxLanguage::Haskell),
        ("%% Adds one.", DiffSyntaxLanguage::Erlang),
        (";; Adds one.", DiffSyntaxLanguage::Clojure),
        ("(* Adds one. *)", DiffSyntaxLanguage::OCaml),
        ("(* Adds one. *)", DiffSyntaxLanguage::OCamlInterface),
        ("# Adds one.", DiffSyntaxLanguage::Elixir),
        ("# Adds one.", DiffSyntaxLanguage::Julia),
        ("// Adds one.", DiffSyntaxLanguage::Groovy),
        ("/* Adds one. */", DiffSyntaxLanguage::Solidity),
        ("    ret ; done", DiffSyntaxLanguage::Assembly),
        ("<!-- Adds one. -->", DiffSyntaxLanguage::Svelte),
    ] {
        let tokens = heuristic_tokens(line, language);
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == SyntaxTokenKind::Comment),
            "{language:?} should treat {line:?} as a comment: {tokens:?}"
        );
    }

    // Haskell's `--` must not swallow an operator section: `x -- y` is a
    // comment, but the heuristic has no way to know that `--` in
    // `f -->> g` is not one either. Pin the ordinary case only.
    let subtraction = heuristic_tokens("    y = x - 1", DiffSyntaxLanguage::Haskell);
    assert!(
        !subtraction
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Comment),
        "a single `-` is not a Haskell comment: {subtraction:?}"
    );
}

/// Per the Haskell report a run of dashes is a comment only when it is *not*
/// followed by a symbol character; otherwise the whole run is an operator.
/// `line_comment: Some("--")` cannot express that, so it greyed `a --> b` from
/// the dashes to the end of the line -- the worst failure mode this path has,
/// because it hides code rather than mis-colouring it.

/// Neighbouring entries in four of the keyword tables, each gap visible as two
/// adjacent lines highlighting differently.
#[test]
fn batch_keyword_tables_cover_their_neighbours() {
    for (line, language, expected) in [
        // `let ... in` is one form; highlighting half of it looked like a bug.
        ("  let x = 1 in x + 1", DiffSyntaxLanguage::Haskell, "in"),
        // Erlang's word-spelled operators are reserved words.
        ("  Y = X div 2,", DiffSyntaxLanguage::Erlang, "div"),
        ("  Z = X band 255,", DiffSyntaxLanguage::Erlang, "band"),
        ("  ok = not Flag,", DiffSyntaxLanguage::Erlang, "not"),
        // Groovy had `boolean` but none of the other primitives.
        ("    int x = 1", DiffSyntaxLanguage::Groovy, "int"),
        ("    double d = 1.0", DiffSyntaxLanguage::Groovy, "double"),
        ("    char c = 'x'", DiffSyntaxLanguage::Groovy, "char"),
        // Solidity's two block forms.
        (
            "        assembly { let p := 1 }",
            DiffSyntaxLanguage::Solidity,
            "assembly",
        ),
        (
            "        unchecked { x += 1; }",
            DiffSyntaxLanguage::Solidity,
            "unchecked",
        ),
    ] {
        let found = heuristic_keywords(line, language);
        assert!(
            found.contains(&expected),
            "{language:?} should treat `{expected}` in {line:?} as a keyword: {found:?}"
        );
    }
}

/// The other half of the Solidity fix: sized types are *uniformly* absent now.
/// Listing `uint256` alone meant `uint256 total;` highlighted and `uint8 flags;`
/// two lines below it did not.

/// The eleven keyword tables the batch added to `is_keyword`, none of which any
/// other test reaches: every other test in this section goes through
/// `prepare_test_document`, i.e. tree-sitter.
#[test]
fn batch_heuristic_keyword_tables_are_covered() {
    for (line, language, expected) in [
        (
            "class Demo extends Base {",
            DiffSyntaxLanguage::Groovy,
            "class",
        ),
        ("(defn run [x] x)", DiffSyntaxLanguage::Clojure, "defn"),
        ("defmodule Demo do", DiffSyntaxLanguage::Elixir, "defmodule"),
        (
            "run(X) when is_integer(X) ->",
            DiffSyntaxLanguage::Erlang,
            "when",
        ),
        (
            "newtype Wrapper = Wrapper Int",
            DiffSyntaxLanguage::Haskell,
            "newtype",
        ),
        ("mutable struct Point", DiffSyntaxLanguage::Julia, "struct"),
        ("let rec loop n =", DiffSyntaxLanguage::OCaml, "rec"),
        (
            "val run : int -> int",
            DiffSyntaxLanguage::OCamlInterface,
            "val",
        ),
        (
            "contract Demo is Base {",
            DiffSyntaxLanguage::Solidity,
            "contract",
        ),
        ("section .text", DiffSyntaxLanguage::Assembly, "section"),
        ("{#each items as item}", DiffSyntaxLanguage::Svelte, "each"),
    ] {
        let found = heuristic_keywords(line, language);
        assert!(
            found.contains(&expected),
            "{language:?} should treat `{expected}` in {line:?} as a keyword: {found:?}"
        );
    }
}

/// The `potential_open_state_lead` fast-skip decides which bytes are even worth
/// examining, and a language whose comment lead is missing from it has its
/// comments run past entirely on the streamed path. Haskell's `-` had to be
/// added there when `line_comment` became None for it.
///
/// The long body is not padding. Below the checkpoint threshold the streamed
/// entry point hands the visible region straight to the per-line tokenizer, so a
/// short-line version of this test exercises the scanner not at all and passes
/// with the fast-skip entry deleted.
///
/// Each case puts the comment opener *before* the slice, so the token can only
/// be right if the scanner resumed in the comment state.

/// The `potential_open_state_lead` fast-skip decides which bytes are even worth
/// examining, and a language whose comment lead is missing from it has its
/// comments run past entirely on the streamed path. Haskell's `-` had to be
/// added there when `line_comment` became None for it.
///
/// The long body is not padding. Below the checkpoint threshold the streamed
/// entry point hands the visible region straight to the per-line tokenizer, so a
/// short-line version of this test exercises the scanner not at all and passes
/// with the fast-skip entry deleted.
///
/// Each case puts the comment opener *before* the slice, so the token can only
/// be right if the scanner resumed in the comment state.
#[test]
fn streamed_slices_resume_inside_batch_line_comments() {
    const CHECKPOINT_SPACING: usize = 32 * 1024;

    for (language, opener) in [
        (DiffSyntaxLanguage::Haskell, "-- "),
        (DiffSyntaxLanguage::Erlang, "% "),
        (DiffSyntaxLanguage::Clojure, "; "),
        (DiffSyntaxLanguage::Assembly, "    ret ; "),
        (DiffSyntaxLanguage::Groovy, "// "),
        (DiffSyntaxLanguage::Elixir, "# "),
    ] {
        reset_streamed_heuristic_line_cache();

        let body = "note ".repeat(CHECKPOINT_SPACING / 5 + 64);
        let text = format!("{opener}{body}");
        let slice_start = opener.len() + CHECKPOINT_SPACING;
        let slice_end = slice_start + 128;
        let raw_text = worktree_core::file_diff::FileDiffLineText::shared(Arc::from(text.clone()));
        let (slice_text, resolved) = raw_text
            .slice_text_resolved(slice_start..slice_end)
            .expect("ASCII slice should resolve");

        let tokens = syntax_tokens_for_streamed_line_slice_heuristic(
            &raw_text,
            language,
            slice_start..slice_end,
            resolved,
        )
        .expect("streamed slice should be supported");
        assert_token_ranges_are_utf8_safe(slice_text.as_ref(), &tokens);

        assert!(
            tokens
                .iter()
                .any(|token| token.kind == SyntaxTokenKind::Comment),
            "{language:?}: a slice {CHECKPOINT_SPACING} bytes into a `{opener}` comment \
             must still be a comment, got {tokens:?}"
        );
    }
}

/// The two block-comment kinds the batch touched: Haskell's `{- -}` is a new
/// `HeuristicBlockCommentKind`, and OCaml reuses the F# `(* *)` spec. Both are
/// resumed from a checkpoint here, which is the only place the start/end byte
/// tables are consulted rather than the per-line `starts_with`.

/// The two block-comment kinds the batch touched: Haskell's `{- -}` is a new
/// `HeuristicBlockCommentKind`, and OCaml reuses the F# `(* *)` spec. Both are
/// resumed from a checkpoint here, which is the only place the start/end byte
/// tables are consulted rather than the per-line `starts_with`.
#[test]
fn streamed_slices_resume_inside_haskell_and_ocaml_block_comments() {
    const CHECKPOINT_SPACING: usize = 32 * 1024;

    for (language, open, close) in [
        (DiffSyntaxLanguage::Haskell, "{-", "-}"),
        (DiffSyntaxLanguage::OCaml, "(*", "*)"),
        (DiffSyntaxLanguage::OCamlInterface, "(*", "*)"),
    ] {
        reset_streamed_heuristic_line_cache();

        let body = "b".repeat(CHECKPOINT_SPACING + 192);
        let text = format!("{open}{body}{close} let x = 1");
        let slice_start = open.len() + CHECKPOINT_SPACING;
        let slice_end = slice_start + 96;
        let raw_text = worktree_core::file_diff::FileDiffLineText::shared(Arc::from(text.clone()));
        let (slice_text, resolved) = raw_text
            .slice_text_resolved(slice_start..slice_end)
            .expect("ASCII slice should resolve");

        let tokens = syntax_tokens_for_streamed_line_slice_heuristic(
            &raw_text,
            language,
            slice_start..slice_end,
            resolved,
        )
        .expect("streamed slice should be supported");
        assert_token_ranges_are_utf8_safe(slice_text.as_ref(), &tokens);

        assert!(
            tokens
                .iter()
                .any(|token| token.kind == SyntaxTokenKind::Comment),
            "{language:?}: a slice inside a `{open} {close}` block must still be a \
             comment, got {tokens:?}"
        );

        // …and the block has to *end*. Asserting only the line above passes even
        // with `heuristic_block_comment_end_bytes` corrupted: an unterminated
        // comment swallows the rest of the line, so the slice above stays inside
        // it either way.
        let tail_start = text.find(close).expect("close should be present") + close.len();
        let (tail_text, tail_resolved) = raw_text
            .slice_text_resolved(tail_start..text.len())
            .expect("tail slice should resolve");
        let tail = syntax_tokens_for_streamed_line_slice_heuristic(
            &raw_text,
            language,
            tail_start..text.len(),
            tail_resolved,
        )
        .expect("streamed tail slice should be supported");
        assert_token_ranges_are_utf8_safe(tail_text.as_ref(), &tail);
        assert!(
            !tail
                .iter()
                .any(|token| token.kind == SyntaxTokenKind::Comment),
            "{language:?}: `{close}` must close the block, but the code after it is \
             still a comment: {tail:?}"
        );
    }
}

/// Token ranges are used to slice the line for rendering, so an out-of-bounds or
/// mid-codepoint range panics rather than mis-colouring. The batch added eleven
/// languages to a hand-written scanner; these are the inputs that break scanners.
#[test]
fn batch_languages_emit_well_formed_tokens_on_hostile_input() {
    let hostile = [
        "",
        " ",
        "\t",
        // Unterminated everything.
        "\"unterminated",
        "'unterminated",
        "`unterminated",
        "/* unterminated",
        "{- unterminated",
        "(* unterminated",
        "<!-- unterminated",
        // Bare openers at end of line, where a lookahead can run past the end.
        "-",
        "--",
        "/",
        "//",
        "{",
        "(",
        "#",
        ";",
        "%",
        "\\",
        "\"",
        "'",
        // Multi-byte, including a comment opener immediately before one.
        "-- ✨ é 日本語",
        "x = \"日本語\" -- ✨",
        "'é'",
        "«»‹›",
        // Adjacent delimiters.
        "\"\"''``",
        "/*/*/*",
        "{-{-{-",
        "(*(*(*",
        "-->--|--<",
        "REM",
        "rem\tx",
    ];

    for (language, _) in batch_language_samples() {
        for line in hostile {
            let tokens = syntax_tokens_for_line(line, language, DiffSyntaxMode::HeuristicOnly);
            assert_token_ranges_are_utf8_safe(line, &tokens);

            // Ranges must also be ordered and non-overlapping: the renderer walks
            // them with a single forward cursor.
            let mut previous_end = 0usize;
            for token in tokens.iter() {
                assert!(
                    token.range.start >= previous_end,
                    "{language:?} emitted overlapping or unsorted tokens for {line:?}: \
                     {tokens:?}"
                );
                previous_end = token.range.end;
            }
        }
    }
}

/// `heuristic_comment_range` now delegates to `line_comment_start_len`, which
/// tests `is_ascii_whitespace()` where the old copy compared against a literal
/// `"rem "`. Visual Basic is the only caller that notices, and it is not a
/// language the batch touched -- exactly the kind of bystander a refactor
/// breaks quietly.

/// `heuristic_comment_range` now delegates to `line_comment_start_len`, which
/// tests `is_ascii_whitespace()` where the old copy compared against a literal
/// `"rem "`. Visual Basic is the only caller that notices, and it is not a
/// language the batch touched -- exactly the kind of bystander a refactor
/// breaks quietly.
#[test]
fn visual_basic_rem_comment_survives_the_shared_comment_decision() {
    for line in ["REM note", "rem note", "Rem note", "REM\tnote"] {
        let tokens = heuristic_tokens(line, DiffSyntaxLanguage::VisualBasic);
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == SyntaxTokenKind::Comment),
            "{line:?} is a Visual Basic REM comment: {tokens:?}"
        );
    }

    // `REM` still needs a delimiter: `REMARK` is an identifier.
    let tokens = heuristic_tokens("REMARK = 1", DiffSyntaxLanguage::VisualBasic);
    assert!(
        !tokens
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Comment),
        "`REMARK` is not a REM comment: {tokens:?}"
    );
}

/// A completeness sweep rather than a behaviour check: every language that
/// claims a grammar must actually produce tokens for a line of itself. A
/// mis-wired grammar, a query that compiles but matches nothing, or an enum
/// variant wired to the wrong `LANGUAGE` constant all show up here as silence.

/// A completeness sweep rather than a behaviour check: every language that
/// claims a grammar must actually produce tokens for a line of itself. A
/// mis-wired grammar, a query that compiles but matches nothing, or an enum
/// variant wired to the wrong `LANGUAGE` constant all show up here as silence.
#[test]
fn every_batch_language_produces_treesitter_tokens() {
    for (language, sample) in batch_language_samples() {
        assert!(
            tree_sitter_grammar(language).is_some(),
            "{language:?} should have a grammar"
        );

        let doc = prepare_test_document(language, sample);
        let tokens = syntax_tokens_for_prepared_document_line(doc, 0)
            .unwrap_or_else(|| panic!("{language:?} should produce prepared tokens"));
        assert!(
            !tokens.is_empty(),
            "{language:?} produced no tokens for {sample:?} -- the grammar is wired but \
             its query matches nothing"
        );
        assert_token_ranges_are_utf8_safe(sample, &tokens);
    }
}

/* NIX_FIXTURE and prepare_nix_document are provided by the parent mod. */

#[test]
fn prepared_nix_document_highlights_core_syntax() {
    let doc = prepare_nix_document(NIX_FIXTURE);

    let comment = token_kinds_for_line_fragment(doc, 0, NIX_FIXTURE[0], "demo package");
    assert!(
        comment.contains(&SyntaxTokenKind::Comment),
        "`# …` is a Nix line comment: {comment:?}"
    );

    for (line_ix, keyword) in [(2usize, "let"), (3, "inherit"), (6, "in"), (7, "rec")] {
        let kinds = token_kinds_for_line_fragment(doc, line_ix, NIX_FIXTURE[line_ix], keyword);
        assert!(
            kinds.contains(&SyntaxTokenKind::Keyword),
            "`{keyword}` should be a keyword: {kinds:?}"
        );
    }

    let formal = token_kinds_for_line_fragment(doc, 1, NIX_FIXTURE[1], "pkgs");
    assert!(
        formal.contains(&SyntaxTokenKind::VariableParameter),
        "`pkgs` is a formal in the function's argument set: {formal:?}"
    );

    let attr = token_kinds_for_line_fragment(doc, 8, NIX_FIXTURE[8], "pname");
    assert!(
        attr.contains(&SyntaxTokenKind::Property),
        "a binding attrpath should read as a property: {attr:?}"
    );

    let string = token_kinds_for_line_fragment(doc, 8, NIX_FIXTURE[8], "\"demo\"");
    assert!(
        string.contains(&SyntaxTokenKind::String),
        "`\"demo\"` should be a string: {string:?}"
    );

    let path = token_kinds_for_line_fragment(doc, 5, NIX_FIXTURE[5], "./README.md");
    assert!(
        path.contains(&SyntaxTokenKind::StringSpecial),
        "a bare Nix path is `@string.special.path`: {path:?}"
    );

    let interpolation = token_kinds_for_line_fragment(doc, 9, NIX_FIXTURE[9], "${");
    assert!(
        interpolation.contains(&SyntaxTokenKind::PunctuationSpecial),
        "`${{` opens an interpolation: {interpolation:?}"
    );
}

/// The one real guard on the reordering in nix_highlights.scm.
///
/// Two patterns capturing the *same* node tie on start byte, so the tiebreak is
/// pattern index — the later rule in the file wins. Upstream ends with a blanket
/// `(identifier) @variable`, which ported verbatim buries every specific
/// identifier rule. Confirmed to fail against upstream's ordering: `builtins`
/// comes back as `[Variable]`. If this fails after a re-sync, the query was not
/// re-sorted.

#[test]
fn prepared_jinja_document_highlights_template_tags() {
    let doc = prepare_jinja_document(JINJA_TEMPLATE_FIXTURE);

    let comment = token_kinds_for_line_fragment(doc, 0, JINJA_TEMPLATE_FIXTURE[0], "heading");
    assert!(
        comment.contains(&SyntaxTokenKind::Comment),
        "`{{# … #}}` is a Jinja comment: {comment:?}"
    );

    let open = token_kinds_for_line_fragment(doc, 2, JINJA_TEMPLATE_FIXTURE[2], "{%");
    assert!(
        open.contains(&SyntaxTokenKind::PunctuationSpecial),
        "the `{{%` delimiter should be punctuation, not plain text: {open:?}"
    );

    for (line_ix, keyword) in [(2usize, "for"), (4, "endfor")] {
        let kinds =
            token_kinds_for_line_fragment(doc, line_ix, JINJA_TEMPLATE_FIXTURE[line_ix], keyword);
        assert!(
            kinds.contains(&SyntaxTokenKind::KeywordControl),
            "`{keyword}` is control flow and should render semibold: {kinds:?}"
        );
    }

    let filter = token_kinds_for_line_fragment(doc, 3, JINJA_TEMPLATE_FIXTURE[3], "upper");
    assert!(
        filter.contains(&SyntaxTokenKind::Function),
        "a filter name after `|` should read as a function: {filter:?}"
    );

    let property = token_kinds_for_line_fragment(doc, 3, JINJA_TEMPLATE_FIXTURE[3], "name");
    assert!(
        property.contains(&SyntaxTokenKind::Property),
        "`item.name` should colour `name` as a property: {property:?}"
    );
}

/// The HTML half of a template comes from the combined injection, not the
/// Jinja grammar -- which sees only opaque `text` nodes.

/// The HTML half of a template comes from the combined injection, not the
/// Jinja grammar -- which sees only opaque `text` nodes.
#[test]
fn prepared_jinja_document_highlights_html_via_the_combined_injection() {
    let doc = prepare_jinja_document(JINJA_TEMPLATE_FIXTURE);

    let tag = token_kinds_for_line_fragment(doc, 1, JINJA_TEMPLATE_FIXTURE[1], "ul");
    assert!(
        tag.contains(&SyntaxTokenKind::Tag),
        "`<ul>` should be tagged by the injected HTML layer: {tag:?}"
    );
    let attribute = token_kinds_for_line_fragment(doc, 1, JINJA_TEMPLATE_FIXTURE[1], "class");
    assert!(
        attribute.contains(&SyntaxTokenKind::Attribute),
        "`class=` should be an HTML attribute: {attribute:?}"
    );

    // The whole point of the combined injection: `<li>` sits inside the loop
    // body, in a different `text` node from `<ul>`, and still highlights.
    let inner = token_kinds_for_line_fragment(doc, 3, JINJA_TEMPLATE_FIXTURE[3], "li");
    assert!(
        inner.contains(&SyntaxTokenKind::Tag),
        "`<li>` is in a separate text run from `<ul>`; only a combined layer \
         sees them as one document: {inner:?}"
    );
}

/// The injected HTML must stay off the template tags, which the Jinja
/// grammar owns. See `combined_injection_gaps`.

#[test]
fn merge_sorted_injection_ranges_normalises_for_set_included_ranges() {
    // Empty stays empty: an empty slice is tree-sitter's "whole document"
    // reset, which callers must detect rather than pass on.
    assert!(merge_sorted_injection_ranges(Vec::new()).is_empty());
    // Degenerate ranges are dropped, not kept as zero-width.
    assert!(merge_sorted_injection_ranges(vec![5..5]).is_empty());
    assert_eq!(merge_sorted_injection_ranges(vec![2..5]), vec![2..5]);
    // Unsorted input is sorted: set_included_ranges rejects descending ranges.
    assert_eq!(
        merge_sorted_injection_ranges(vec![10..12, 2..5]),
        vec![2..5, 10..12]
    );
    // Touching ranges coalesce, so the gap list carries no empty entries.
    assert_eq!(merge_sorted_injection_ranges(vec![2..5, 5..9]), vec![2..9]);
    // Overlapping ranges coalesce: set_included_ranges rejects overlap.
    assert_eq!(merge_sorted_injection_ranges(vec![2..7, 5..9]), vec![2..9]);
    // Fully contained range is absorbed rather than shortening the outer one.
    assert_eq!(
        merge_sorted_injection_ranges(vec![2..20, 5..9]),
        vec![2..20]
    );
}

// The one-range cases are the point: a single included range is the shape
// every non-combined injection has, and both helpers have to leave it alone.
#[allow(clippy::single_range_in_vec_init)]

/// An 8-column table row used to produce 513 ranges in one 64-line chunk, one
/// over the ceiling, and the whole chunk lost its HTML.
#[test]
fn dense_table_template_keeps_its_html_highlighting() {
    for cells in [4usize, 8, 16] {
        let text = dense_jinja_table(200, cells);
        let lines: Vec<&str> = text.lines().collect();
        let doc = prepare_test_document(DiffSyntaxLanguage::Jinja, &text);

        let kinds = token_kinds_for_line_fragment(doc, 100, lines[100], "<td>");
        assert!(
            kinds.contains(&SyntaxTokenKind::Tag),
            "a {cells}-cell table row lost its HTML highlighting: {kinds:?}"
        );
    }
}

/// The byte ceiling had the same defect at an ordinary file size: all the HTML
/// between two template tags is ONE `(text)` node, so a ~1800-line template
/// tripped the 128KB ceiling in every window.

/// The byte ceiling had the same defect at an ordinary file size: all the HTML
/// between two template tags is ONE `(text)` node, so a ~1800-line template
/// tripped the 128KB ceiling in every window.
#[test]
fn large_template_with_one_huge_text_run_keeps_its_html_highlighting() {
    let mut lines = vec!["{% block body %}".to_string()];
    for ix in 0..2_400 {
        lines.push(format!(
            "  <span class=\"cell\" data-row=\"{ix}\">value {ix} padded out</span>"
        ));
    }
    lines.push("{% endblock %}".to_string());
    let text = lines.join("\n");
    assert!(
        text.len() > TS_COMBINED_INJECTION_MAX_BYTES,
        "fixture must exceed the byte ceiling to be a regression test ({} bytes)",
        text.len()
    );

    let line_refs: Vec<&str> = text.lines().collect();
    let doc = prepare_test_document(DiffSyntaxLanguage::Jinja, &text);
    for line_ix in [1usize, 700, 1_500, 2_300] {
        let kinds = token_kinds_for_line_fragment(doc, line_ix, line_refs[line_ix], "span");
        assert!(
            kinds.contains(&SyntaxTokenKind::Tag),
            "line {line_ix} of a {}-byte template lost its HTML: {kinds:?}",
            text.len()
        );
    }
}

/// The property the whole optimisation rests on, and the reason for the margin:
/// a `<section` whose attributes run onto the next lines straddles the window
/// edge, and an exact clip cuts it in half. Asserted against an unclipped parse
/// so it stays honest if the margin is ever tuned.

/// The property the whole optimisation rests on, and the reason for the margin:
/// a `<section` whose attributes run onto the next lines straddles the window
/// edge, and an exact clip cuts it in half. Asserted against an unclipped parse
/// so it stays honest if the margin is ever tuned.
#[test]
fn clipping_a_combined_layer_to_the_window_preserves_its_tokens() {
    let mut lines = vec!["{% block body %}".to_string()];
    for ix in 0..300 {
        if ix == 62 || ix == 126 {
            lines.push("  <section".to_string());
            lines.push("     id=\"straddle\"".to_string());
            lines.push("     class=\"wide\">body</section>".to_string());
        } else {
            lines.push(format!(
                "  <span class=\"c{ix}\" data-x='y'>row {ix}</span>"
            ));
        }
    }
    lines.push("{% endblock %}".to_string());
    let text = lines.join("\n") + "\n";

    let input = treesitter_document_input_from_text(&text);
    let bytes = text.as_bytes();
    let line_starts = input.line_starts.as_ref();
    let jinja = tree_sitter_highlight_spec(DiffSyntaxLanguage::Jinja).expect("jinja spec");
    let root = with_ts_parser_parse_result(&jinja.ts_language, |parser| {
        parse_treesitter_tree(parser, bytes, None, None)
    })
    .expect("root parse");

    let start_line_ix = 64usize;
    let end_line_ix = start_line_ix + TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS;
    let matches = collect_treesitter_injection_matches_for_line_window(
        &root,
        jinja,
        bytes,
        line_starts,
        start_line_ix,
        end_line_ix,
    );
    let group = matches.combined.first().expect("one combined html group");
    assert_eq!(
        group.ranges.len(),
        1,
        "the fixture's body must be one text run, or it is not testing the hard case"
    );

    let window_start = line_starts[start_line_ix];
    let window_end = line_region_end_byte(line_starts, bytes.len(), end_line_ix - 1);
    let html = tree_sitter_highlight_spec(DiffSyntaxLanguage::Html).expect("html spec");
    let render = |ranges: &[Range<usize>]| -> Vec<Vec<SyntaxToken>> {
        let tree = parse_combined_injection_tree(html, bytes, line_starts, ranges)
            .expect("combined parse");
        let mut injected = collect_treesitter_document_line_tokens_for_line_window(
            &tree,
            html,
            bytes,
            line_starts,
            start_line_ix,
            end_line_ix,
        );
        for gap in combined_injection_gaps(window_start..window_end, ranges) {
            subtract_absolute_range_from_document_tokens(
                line_starts,
                bytes,
                start_line_ix,
                &mut injected,
                gap,
            );
        }
        injected
    };

    let clip_region =
        combined_injection_clip_region(line_starts, bytes.len(), start_line_ix, end_line_ix);
    let clipped_ranges = clip_injection_ranges_to_region(&group.ranges, &clip_region);
    let clipped_bytes: usize = clipped_ranges.iter().map(|r| r.end - r.start).sum();
    let full_bytes: usize = group.ranges.iter().map(|r| r.end - r.start).sum();
    assert!(
        clipped_bytes < full_bytes,
        "the clip must actually shrink the parse ({clipped_bytes} vs {full_bytes})"
    );

    assert_eq!(
        render(&group.ranges),
        render(&clipped_ranges),
        "clipping to the window changed the tokens the window renders"
    );
}

/// The clip region is the window plus a margin on both sides, and the margin is
/// load-bearing rather than decorative -- see the constant.

/// A cut that touches nothing must leave the line's tokens exactly as they were,
/// and must not reallocate to do it.
#[test]
fn subtracting_a_non_overlapping_range_leaves_line_tokens_untouched() {
    let original = vec![
        SyntaxToken {
            range: 0..4,
            kind: SyntaxTokenKind::Tag,
        },
        SyntaxToken {
            range: 10..14,
            kind: SyntaxTokenKind::String,
        },
    ];

    // Entirely before, entirely after, and in the gap between the two tokens.
    for cut in [20..30usize, 4..10, 100..200] {
        let mut tokens = original.clone();
        subtract_relative_range_from_line_tokens(&mut tokens, cut.clone());
        assert_eq!(tokens, original, "cut {cut:?} must be a no-op");
    }

    // ... and a cut that does overlap still splits, so the fast path is not
    // swallowing real work.
    let mut tokens = original.clone();
    subtract_relative_range_from_line_tokens(&mut tokens, 2..12);
    assert_eq!(
        tokens,
        vec![
            SyntaxToken {
                range: 0..2,
                kind: SyntaxTokenKind::Tag,
            },
            SyntaxToken {
                range: 12..14,
                kind: SyntaxTokenKind::String,
            },
        ]
    );
}

/// Pins the ordering rather than a symptom: no in-tree grammar declares both
/// kinds over one span yet, but with combined applied first an overlapping
/// single would delete its tokens and repaint only part of the span.

#[test]
#[ignore]
fn perf_treesitter_tokenization_smoke() {
    let text = "fn main() { let x = Some(123); println!(\"{x:?}\"); }";
    let start = Instant::now();
    for _ in 0..200_000 {
        let _ = syntax_tokens_for_line(text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    }
    eprintln!("syntax_tokens_for_line (rust): {:?}", start.elapsed());
}

// ---- heuristic tokenizer tests ----
