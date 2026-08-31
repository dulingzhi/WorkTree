use super::*;
use std::time::{Duration, Instant};

/// Serializes tests that reset or assert on the shared syntax instrumentation
/// counters. Without this lock, concurrent tests can reset or bump those
/// counters while another test is asserting on them, causing flaky failures
/// under parallel test execution.
static GLOBAL_COUNTER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_global_counter_tests() -> std::sync::MutexGuard<'static, ()> {
    match GLOBAL_COUNTER_TEST_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn assert_token_ranges_are_utf8_safe(text: &str, tokens: &[SyntaxToken]) {
    for token in tokens {
        assert!(
            token.range.start <= token.range.end,
            "{token:?} in {text:?}"
        );
        assert!(token.range.end <= text.len(), "{token:?} in {text:?}");
        assert!(
            text.is_char_boundary(token.range.start),
            "{token:?} start is not a char boundary in {text:?}"
        );
        assert!(
            text.is_char_boundary(token.range.end),
            "{token:?} end is not a char boundary in {text:?}"
        );
    }
}

fn has_token_kind_and_text(
    text: &str,
    tokens: &[SyntaxToken],
    kind: SyntaxTokenKind,
    expected: &str,
) -> bool {
    tokens.iter().any(|token| {
        token.kind == kind
            && token.range.end <= text.len()
            && &text[token.range.clone()] == expected
    })
}

fn reset_ts_parser_test_state() {
    TS_PARSER.with(|parser| {
        *parser.borrow_mut() = tree_sitter::Parser::new();
    });
    TS_CURSOR.with(|cursor| {
        *cursor.borrow_mut() = tree_sitter::QueryCursor::new();
    });
    TS_INPUT.with(|input| input.borrow_mut().clear());
    TS_LINE_TOKEN_CACHE.with(|cache| {
        *cache.borrow_mut() = SingleLineSyntaxTokenCache::new();
    });
    TS_PARSER_REQUIRES_LANGUAGE_RESET.with(|needs_reset| needs_reset.set(false));
    TS_PARSER_SET_LANGUAGE_CALL_COUNT.with(|count| count.set(0));
}

fn prepare_test_document(language: DiffSyntaxLanguage, text: &str) -> PreparedSyntaxDocument {
    let input = treesitter_document_input_from_text(text);
    match prepare_treesitter_document_with_budget_reuse_text(
        language,
        DiffSyntaxMode::Auto,
        SharedString::from(text.to_owned()),
        input.line_starts,
        DiffSyntaxBudget {
            foreground_parse: Duration::from_millis(200),
        },
        None,
        None,
    ) {
        PrepareTreesitterDocumentResult::Ready(doc) => doc,
        other => panic!("test document should parse successfully, got {other:?}"),
    }
}

fn prepare_test_document_with_budget_reuse(
    language: DiffSyntaxLanguage,
    text: &str,
    budget: DiffSyntaxBudget,
    old_document: Option<PreparedSyntaxDocument>,
) -> PrepareTreesitterDocumentResult {
    let input = treesitter_document_input_from_text(text);
    prepare_treesitter_document_with_budget_reuse_text(
        language,
        DiffSyntaxMode::Auto,
        SharedString::from(text.to_owned()),
        input.line_starts,
        budget,
        old_document,
        None,
    )
}

fn prepare_html_document(lines: &[&str]) -> PreparedSyntaxDocument {
    prepare_test_document(DiffSyntaxLanguage::Html, &lines.join("\n"))
}

fn prepare_vue_document(lines: &[&str]) -> PreparedSyntaxDocument {
    prepare_test_document(DiffSyntaxLanguage::Vue, &lines.join("\n"))
}

/// Kinds of every token overlapping `fragment` within `line_ix`. Token ranges
/// on a prepared document are line-relative, including tokens remapped back
/// from an injection, so this works across the injection boundary.
fn token_kinds_for_line_fragment(
    doc: PreparedSyntaxDocument,
    line_ix: usize,
    line_text: &str,
    fragment: &str,
) -> Vec<SyntaxTokenKind> {
    let start = line_text
        .find(fragment)
        .unwrap_or_else(|| panic!("fragment {fragment:?} should appear in {line_text:?}"));
    let end = start + fragment.len();
    syntax_tokens_for_prepared_document_line(doc, line_ix)
        .unwrap_or_else(|| panic!("line {line_ix} tokens should be available"))
        .iter()
        .filter(|token| token.range.start < end && token.range.end > start)
        .map(|token| token.kind)
        .collect()
}

fn heuristic_tokens(text: &str, language: DiffSyntaxLanguage) -> Vec<SyntaxToken> {
    syntax_tokens_for_line(text, language, DiffSyntaxMode::HeuristicOnly).to_vec()
}

fn heuristic_string_spans(text: &str, language: DiffSyntaxLanguage) -> Vec<&str> {
    heuristic_tokens(text, language)
        .into_iter()
        .filter(|token| token.kind == SyntaxTokenKind::String)
        .map(|token| &text[token.range])
        .collect()
}

/// The keyword and keyword-control spans a line yields on the heuristic path.
///
/// Shared rather than redefined per test: the three copies this replaced drifted
/// apart on whether `KeywordControl` counted.
fn heuristic_keywords(text: &str, language: DiffSyntaxLanguage) -> Vec<&str> {
    syntax_tokens_for_line(text, language, DiffSyntaxMode::HeuristicOnly)
        .iter()
        .filter(|token| {
            matches!(
                token.kind,
                SyntaxTokenKind::Keyword | SyntaxTokenKind::KeywordControl
            )
        })
        .map(|token| &text[token.range.clone()])
        .collect()
}

/// A query's rule lines, with blanks and `;` comments dropped.
///
/// Used by the three `..._embeds_the_..._base_verbatim` tripwires. They compare
/// vendored copies against their upstream, so all three have to strip comments
/// the same way or the comparison means different things in each.
fn query_rule_lines(query: &str) -> Vec<&str> {
    query
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && !line.trim_start().starts_with(';'))
        .collect()
}

/// A single-file component covering every Vue injection path at once.
/// Line indices are asserted against by the tests below, so keep them stable.
const VUE_SFC_FIXTURE: &[&str] = &[
    /* 0 */ "<template>",
    /* 1 */ r#"  <div :class="wrapperClass">"#,
    /* 2 */ r#"    <button v-if="count > 10">{{ count + 1 }}</button>"#,
    /* 3 */ "  </div>",
    /* 4 */ "</template>",
    /* 5 */ "",
    /* 6 */ r#"<script setup lang="ts">"#,
    /* 7 */ "const count = 42;",
    /* 8 */ "</script>",
    /* 9 */ "",
    /* 10 */ r#"<style lang="scss">"#,
    /* 11 */ ".wrapper { color: red; }",
    /* 12 */ "</style>",
];

fn capture_name_is_intentionally_ignored(name: &str) -> bool {
    name == "none"
        || name == "clean"
        || name == "assignvalue"
        || name == "embedded"
        || name == "error"
        || name == "nested"
        || name == "spell"
        || name == "injection.content"
        || name.starts_with("text.")
        || name.starts_with('_')
}

fn assert_capture_names_are_supported(language: tree_sitter::Language, source: &str) {
    let query = tree_sitter::Query::new(&language, source).expect("query should compile");
    for name in query.capture_names() {
        assert!(
            syntax_kind_from_capture_name(name).is_some()
                || capture_name_is_intentionally_ignored(name),
            "unsupported capture name in vendored asset: {name}"
        );
    }
}

/// Every `DiffSyntaxLanguage` variant, listed by hand.
///
/// Deliberately not derived from the enum: the point is that adding a variant
/// breaks a test until someone states what the new language does, rather than
/// being silently swept into whatever the loop asserts.
fn all_supported_languages() -> Vec<DiffSyntaxLanguage> {
    Vec::from([
        DiffSyntaxLanguage::Markdown,
        DiffSyntaxLanguage::MarkdownInline,
        DiffSyntaxLanguage::Html,
        DiffSyntaxLanguage::Vue,
        DiffSyntaxLanguage::Svelte,
        DiffSyntaxLanguage::Jinja,
        DiffSyntaxLanguage::Css,
        DiffSyntaxLanguage::Hcl,
        DiffSyntaxLanguage::Bicep,
        DiffSyntaxLanguage::Lua,
        DiffSyntaxLanguage::Makefile,
        DiffSyntaxLanguage::Nix,
        DiffSyntaxLanguage::Kotlin,
        DiffSyntaxLanguage::Zig,
        DiffSyntaxLanguage::Groovy,
        DiffSyntaxLanguage::Clojure,
        DiffSyntaxLanguage::Elixir,
        DiffSyntaxLanguage::Erlang,
        DiffSyntaxLanguage::Haskell,
        DiffSyntaxLanguage::Julia,
        DiffSyntaxLanguage::OCaml,
        DiffSyntaxLanguage::OCamlInterface,
        DiffSyntaxLanguage::Solidity,
        DiffSyntaxLanguage::Assembly,
        DiffSyntaxLanguage::Rust,
        DiffSyntaxLanguage::Python,
        DiffSyntaxLanguage::JavaScript,
        DiffSyntaxLanguage::Jsdoc,
        DiffSyntaxLanguage::TypeScript,
        DiffSyntaxLanguage::Tsx,
        DiffSyntaxLanguage::Regex,
        DiffSyntaxLanguage::Go,
        DiffSyntaxLanguage::GoMod,
        DiffSyntaxLanguage::GoWork,
        DiffSyntaxLanguage::C,
        DiffSyntaxLanguage::Cpp,
        DiffSyntaxLanguage::ObjectiveC,
        DiffSyntaxLanguage::CSharp,
        DiffSyntaxLanguage::FSharp,
        DiffSyntaxLanguage::VisualBasic,
        DiffSyntaxLanguage::Java,
        DiffSyntaxLanguage::Php,
        DiffSyntaxLanguage::Ruby,
        DiffSyntaxLanguage::PowerShell,
        DiffSyntaxLanguage::Swift,
        DiffSyntaxLanguage::R,
        DiffSyntaxLanguage::Dart,
        DiffSyntaxLanguage::Scala,
        DiffSyntaxLanguage::Perl,
        DiffSyntaxLanguage::Json,
        DiffSyntaxLanguage::Toml,
        DiffSyntaxLanguage::Yaml,
        DiffSyntaxLanguage::Sql,
        DiffSyntaxLanguage::Diff,
        DiffSyntaxLanguage::GitCommit,
        DiffSyntaxLanguage::Bash,
        DiffSyntaxLanguage::Xml,
    ])
}

const GROOVY_FIXTURE: &[&str] = &[
    /*  0 */ "// Build config.",
    /*  1 */ "plugins {",
    /*  2 */ "    id 'java'",
    /*  3 */ "}",
    /*  4 */ "",
    /*  5 */ "class Demo {",
    /*  6 */ "    static int run(int x) {",
    /*  7 */ "        return x + 1",
    /*  8 */ "    }",
    /*  9 */ "}",
];

const SOLIDITY_FIXTURE: &[&str] = &[
    /*  0 */ "// SPDX-License-Identifier: MIT",
    /*  1 */ "pragma solidity ^0.8.0;",
    /*  2 */ "",
    /*  3 */ "contract Demo {",
    /*  4 */ "    uint256 public total;",
    /*  5 */ "",
    /*  6 */ "    function add(uint256 x) public returns (uint256) {",
    /*  7 */ "        total += x;",
    /*  8 */ "        return total;",
    /*  9 */ "    }",
    /* 10 */ "}",
];

const ASSEMBLY_FIXTURE: &[&str] = &[
    /*  0 */ "section .text",
    /*  1 */ "global run",
    /*  2 */ "run:",
    /*  3 */ "    mov eax, 1 ; seed",
    /*  4 */ "    add eax, edi",
    /*  5 */ "    ret",
];

const SVELTE_FIXTURE: &[&str] = &[
    /*  0 */ "<script lang=\"ts\">",
    /*  1 */ "  let count: number = 0;",
    /*  2 */ "</script>",
    /*  3 */ "",
    /*  4 */ "{#if count > 0}",
    /*  5 */ "  <button class=\"btn\">{count}</button>",
    /*  6 */ "{:else}",
    /*  7 */ "  <p>none</p>",
    /*  8 */ "{/if}",
    /*  9 */ "",
    /* 10 */ "<style>",
    /* 11 */ "  .btn { color: red; }",
    /* 12 */ "</style>",
];

const NIX_FIXTURE: &[&str] = &[
    /*  0 */ "# Build a demo package.",
    /*  1 */ "{ pkgs, lib ? pkgs.lib, ... }:",
    /*  2 */ "let",
    /*  3 */ "  inherit (pkgs) stdenv;",
    /*  4 */ "  version = \"1.0\";",
    /*  5 */ "  readme = builtins.readFile ./README.md;",
    /*  6 */ "in",
    /*  7 */ "stdenv.mkDerivation rec {",
    /*  8 */ "  pname = \"demo\";",
    /*  9 */ "  meta.description = \"demo v${version}\";",
    /* 10 */ "  buildPhase = ''",
    /* 11 */ "    export OUT=$out",
    /* 12 */ "    if [ -d bin ]; then",
    /* 13 */ "      cp -r bin \"$out/bin\"",
    /* 14 */ "    fi",
    /* 15 */ "  '';",
    /* 16 */ "}",
];

fn prepare_nix_document(lines: &[&str]) -> PreparedSyntaxDocument {
    prepare_test_document(DiffSyntaxLanguage::Nix, &lines.join("\n"))
}

/// The `.njk` / `.j2` / `.jinja` fixture, shared by the tests below.
const JINJA_TEMPLATE_FIXTURE: &[&str] = &[
    /* 0 */ "{# page heading #}",
    /* 1 */ "<ul class=\"list\">",
    /* 2 */ "  {% for item in items %}",
    /* 3 */ "    <li>{{ item.name | upper }}</li>",
    /* 4 */ "  {% endfor %}",
    /* 5 */ "</ul>",
];

fn prepare_jinja_document(lines: &[&str]) -> PreparedSyntaxDocument {
    prepare_test_document(DiffSyntaxLanguage::Jinja, &lines.join("\n"))
}

/// A template dense enough to exercise the per-window ceilings, `rows` lines of
/// `cells` cells each wrapped in a block so the body is one big text run.
fn dense_jinja_table(rows: usize, cells: usize) -> String {
    let mut lines = vec!["{% block body %}".to_string()];
    for row in 0..rows {
        let mut line = String::from("<tr>");
        for cell in 0..cells {
            line.push_str(&format!("<td>{{{{ r{row}.c{cell} }}}}</td>"));
        }
        line.push_str("</tr>");
        lines.push(line);
    }
    lines.push("{% endblock %}".to_string());
    lines.join("\n")
}

/// An 8-column table row used to produce 513 ranges in one 64-line chunk, one
/// over the ceiling, and the whole chunk lost its HTML.
mod engine;
mod heuristic;
mod injections;
mod languages;
mod prepared;
mod vendored;
