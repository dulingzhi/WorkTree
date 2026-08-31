use super::*;

fn ts_parser_set_language_call_count() -> usize {
    TS_PARSER_SET_LANGUAGE_CALL_COUNT.with(Cell::get)
}

fn with_silenced_panic_hook<R>(f: impl FnOnce() -> R) -> R {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = f();
    std::panic::set_hook(previous_hook);
    result
}

fn prepare_test_document_in_background(
    language: DiffSyntaxLanguage,
    text: &str,
) -> Option<PreparedSyntaxDocumentData> {
    let input = treesitter_document_input_from_text(text);
    prepare_treesitter_document_in_background_text_with_reuse(
        language,
        DiffSyntaxMode::Auto,
        SharedString::from(text.to_owned()),
        input.line_starts,
        None,
        None,
    )
}

#[test]
fn treesitter_line_length_guard() {
    assert!(super::should_use_treesitter_for_line("fn main() {}"));
    assert!(!super::should_use_treesitter_for_line(
        &"a".repeat(MAX_TREESITTER_LINE_BYTES + 1)
    ));
}

#[test]
fn treesitter_query_cursor_sets_match_limit_for_line_queries() {
    let _ = syntax_tokens_for_line(
        "fn main() { let value = Some(1); }",
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
    );
    TS_CURSOR.with(|cursor| {
        assert_eq!(cursor.borrow().match_limit(), TS_QUERY_MATCH_LIMIT);
    });
}

#[test]
fn treesitter_variable_capture_maps_but_gets_no_color() {
    // `@variable` now maps to `Variable` (tracked but rendered without color)
    // so the capture info is preserved for potential theme use.
    assert_eq!(
        super::syntax_kind_from_capture_name("variable"),
        Some(SyntaxTokenKind::Variable)
    );
    // `@variable.parameter` maps to its own distinct kind
    assert_eq!(
        super::syntax_kind_from_capture_name("variable.parameter"),
        Some(SyntaxTokenKind::VariableParameter)
    );
}

#[test]
fn treesitter_tokenization_is_safe_across_languages() {
    let rust_line = "fn main() { let x = 1; }";
    let json_line = "{\"x\": 1}";

    let rust = syntax_tokens_for_line(rust_line, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    let json = syntax_tokens_for_line(json_line, DiffSyntaxLanguage::Json, DiffSyntaxMode::Auto);

    for t in rust {
        assert!(t.range.start <= t.range.end);
        assert!(t.range.end <= rust_line.len());
    }
    for t in json {
        assert!(t.range.start <= t.range.end);
        assert!(t.range.end <= json_line.len());
    }
}

#[test]
fn treesitter_line_fallback_survives_incomplete_fragments() {
    let cases = [
        (
            DiffSyntaxLanguage::Rust,
            vec![
                "pub struct Example<'a",
                "let value = Some(\"unterminated",
                "match value { Some(inner) => inner.",
            ],
        ),
        (
            DiffSyntaxLanguage::JavaScript,
            vec![
                "const element = document.querySelector(\".demo",
                "return values.map((entry) => entry.",
                "class Example extends React.Component {",
            ],
        ),
        (
            DiffSyntaxLanguage::TypeScript,
            vec![
                "const value: Promise<Result<string, Error>> =",
                "type Example<T extends Record<string, number>",
            ],
        ),
        (
            DiffSyntaxLanguage::Html,
            vec![
                "<button onclick=\"const value = 1;",
                "<div style=\"color: red;",
                "<input class=\"demo\"",
            ],
        ),
        (
            DiffSyntaxLanguage::Xml,
            vec![
                "<root attr=\"shared",
                "<?xml-stylesheet href=\"theme.css",
                "<item key=\"value\"",
            ],
        ),
    ];

    for (language, fragments) in cases {
        for fragment in fragments {
            let _ = syntax_tokens_for_line(fragment, language, DiffSyntaxMode::Auto);
            for trim in 0..=8usize {
                if trim > fragment.len()
                    || !fragment.is_char_boundary(fragment.len().saturating_sub(trim))
                {
                    continue;
                }
                let shortened = &fragment[..fragment.len().saturating_sub(trim)];
                let result = std::panic::catch_unwind(|| {
                    syntax_tokens_for_line(shortened, language, DiffSyntaxMode::Auto)
                });
                assert!(
                    result.is_ok(),
                    "single-line tree-sitter fallback should not panic for {language:?} fragment {shortened:?}"
                );
            }
        }
    }
}

#[test]
fn parser_fast_path_reuses_same_language_until_switch() {
    reset_ts_parser_test_state();

    let rust_tokens =
        syntax_tokens_for_line_treesitter("fn main() { let x = 1; }", DiffSyntaxLanguage::Rust)
            .expect("first rust parse should succeed");
    assert!(!rust_tokens.is_empty());
    assert_eq!(ts_parser_set_language_call_count(), 1);

    let rust_tokens_again =
        syntax_tokens_for_line_treesitter("fn helper() { let y = 2; }", DiffSyntaxLanguage::Rust)
            .expect("second rust parse should succeed");
    assert!(!rust_tokens_again.is_empty());
    assert_eq!(ts_parser_set_language_call_count(), 1);

    let json_tokens = syntax_tokens_for_line_treesitter("{\"x\": 1}", DiffSyntaxLanguage::Json)
        .expect("json parse should succeed");
    assert!(!json_tokens.is_empty());
    assert_eq!(ts_parser_set_language_call_count(), 2);

    let json_tokens_again =
        syntax_tokens_for_line_treesitter("{\"y\": 2}", DiffSyntaxLanguage::Json)
            .expect("second json parse should succeed");
    assert!(!json_tokens_again.is_empty());
    assert_eq!(ts_parser_set_language_call_count(), 2);
}

#[test]
fn parser_fast_path_reconfigures_after_recovered_query_panic() {
    reset_ts_parser_test_state();

    let baseline =
        syntax_tokens_for_line_treesitter("fn main() { let x = 1; }", DiffSyntaxLanguage::Rust)
            .expect("baseline rust parse should succeed");
    assert!(!baseline.is_empty());
    assert_eq!(ts_parser_set_language_call_count(), 1);

    let recovered: Option<()> = with_silenced_panic_hook(|| {
        catch_treesitter_query_panic(|| panic!("simulate query panic"))
    });
    assert!(recovered.is_none());

    let reparsed =
        syntax_tokens_for_line_treesitter("fn main() { let y = 2; }", DiffSyntaxLanguage::Rust)
            .expect("rust parse after panic recovery should succeed");
    assert!(
        reparsed
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Keyword),
        "rust parse after panic recovery should still contain keyword highlights: {reparsed:?}"
    );
    assert_eq!(ts_parser_set_language_call_count(), 2);
}

#[test]
fn parser_fast_path_reconfigures_after_interrupted_parse() {
    reset_ts_parser_test_state();

    let baseline =
        syntax_tokens_for_line_treesitter("fn main() { let x = 1; }", DiffSyntaxLanguage::Rust)
            .expect("baseline rust parse should succeed");
    assert!(!baseline.is_empty());
    assert_eq!(ts_parser_set_language_call_count(), 1);

    let spec = tree_sitter_highlight_spec(DiffSyntaxLanguage::Rust)
        .expect("Rust highlight spec should exist");
    let interrupted_input = "fn main() { let value = Some(42); }\n".repeat(4_096);
    let interrupted = with_ts_parser_parse_result(&spec.ts_language, |parser| {
        parse_treesitter_tree(
            parser,
            interrupted_input.as_bytes(),
            None,
            Some(Duration::ZERO),
        )
    });
    assert!(
        interrupted.is_none(),
        "zero-budget parse should interrupt before producing a tree"
    );

    let reparsed =
        syntax_tokens_for_line_treesitter("fn helper() { let y = 2; }", DiffSyntaxLanguage::Rust)
            .expect("rust parse after interrupted parse should succeed");
    assert!(
        reparsed
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Keyword),
        "rust parse after interrupted parse should still contain keyword highlights: {reparsed:?}"
    );
    assert_eq!(ts_parser_set_language_call_count(), 2);
}

#[test]
fn parser_fast_path_reconfigures_when_parser_slot_loses_language() {
    reset_ts_parser_test_state();

    let first =
        syntax_tokens_for_line_treesitter("fn main() { let x = 1; }", DiffSyntaxLanguage::Rust)
            .expect("baseline rust parse should succeed");
    assert!(!first.is_empty());
    assert_eq!(ts_parser_set_language_call_count(), 1);

    TS_PARSER.with(|parser| {
        *parser.borrow_mut() = tree_sitter::Parser::new();
    });

    let reparsed =
        syntax_tokens_for_line_treesitter("fn helper() { let y = 2; }", DiffSyntaxLanguage::Rust)
            .expect("rust parse should recover after parser slot reset");
    assert!(
        reparsed
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Keyword),
        "rust parse after parser slot reset should still contain keyword highlights: {reparsed:?}"
    );
    assert_eq!(ts_parser_set_language_call_count(), 2);
}

#[test]
fn treesitter_document_cache_lru_touch_keeps_recent_entry_alive() {
    for trial in 0..128usize {
        let mut cache = TreesitterDocumentCache::new();
        for key in 0..TS_DOCUMENT_CACHE_MAX_ENTRIES {
            cache.insert_document(
                TreesitterDocumentCache::make_test_cache_key(key as u64),
                vec![Vec::new()],
            );
        }

        let touched_key = TreesitterDocumentCache::make_test_cache_key(0);
        assert!(cache.contains_document(touched_key, 1));
        cache.insert_document(
            TreesitterDocumentCache::make_test_cache_key(10_000 + trial as u64),
            vec![Vec::new()],
        );

        assert!(
            cache.contains_key(touched_key),
            "touched key should survive eviction on trial {trial}"
        );
    }
}

#[test]
fn treesitter_point_for_byte_maps_newline_terminated_eof_to_next_row() {
    let input = b"alpha\nbeta\n";
    let line_starts: Vec<usize> = vec![0, 6];
    assert_eq!(
        treesitter_point_for_byte(&line_starts, input, input.len()),
        tree_sitter::Point::new(2, 0),
        "EOF for newline-terminated input should point to the next row start"
    );
}

#[test]
fn parse_budget_timeout_falls_back_to_background_prepare() {
    let text = vec!["/* budget */ let value = Some(42);"; 2_048].join("\n");
    let attempt = prepare_test_document_with_budget_reuse(
        DiffSyntaxLanguage::Rust,
        &text,
        DiffSyntaxBudget {
            foreground_parse: Duration::ZERO,
        },
        None,
    );
    assert_eq!(attempt, PrepareTreesitterDocumentResult::TimedOut);

    let prepared = prepare_test_document_in_background(DiffSyntaxLanguage::Rust, &text)
        .expect("background parse should produce a prepared document");
    let document = inject_prepared_document_data(prepared);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("background-prepared document should have tokens");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Comment),
        "background parse should still yield syntax tokens"
    );
}

#[test]
fn capture_name_mapping_preserves_rich_semantics() {
    // Full dot-qualified names should map to specific variants
    assert_eq!(
        syntax_kind_from_capture_name("comment.doc"),
        Some(SyntaxTokenKind::CommentDoc)
    );
    assert_eq!(
        syntax_kind_from_capture_name("string.escape"),
        Some(SyntaxTokenKind::StringEscape)
    );
    assert_eq!(
        syntax_kind_from_capture_name("keyword.control"),
        Some(SyntaxTokenKind::KeywordControl)
    );
    assert_eq!(
        syntax_kind_from_capture_name("function.method"),
        Some(SyntaxTokenKind::FunctionMethod)
    );
    assert_eq!(
        syntax_kind_from_capture_name("function.special"),
        Some(SyntaxTokenKind::FunctionSpecial)
    );
    assert_eq!(
        syntax_kind_from_capture_name("constructor"),
        Some(SyntaxTokenKind::Constructor)
    );
    assert_eq!(
        syntax_kind_from_capture_name("type.builtin"),
        Some(SyntaxTokenKind::TypeBuiltin)
    );
    assert_eq!(
        syntax_kind_from_capture_name("type.interface"),
        Some(SyntaxTokenKind::TypeInterface)
    );
    assert_eq!(
        syntax_kind_from_capture_name("namespace"),
        Some(SyntaxTokenKind::Namespace)
    );
    assert_eq!(
        syntax_kind_from_capture_name("variable"),
        Some(SyntaxTokenKind::Variable)
    );
    assert_eq!(
        syntax_kind_from_capture_name("variable.parameter"),
        Some(SyntaxTokenKind::VariableParameter)
    );
    assert_eq!(
        syntax_kind_from_capture_name("variable.special"),
        Some(SyntaxTokenKind::VariableSpecial)
    );
    assert_eq!(
        syntax_kind_from_capture_name("variable.builtin"),
        Some(SyntaxTokenKind::VariableBuiltin)
    );
    assert_eq!(
        syntax_kind_from_capture_name("label"),
        Some(SyntaxTokenKind::Label)
    );
    assert_eq!(
        syntax_kind_from_capture_name("operator"),
        Some(SyntaxTokenKind::Operator)
    );
    assert_eq!(
        syntax_kind_from_capture_name("punctuation.bracket"),
        Some(SyntaxTokenKind::PunctuationBracket)
    );
    assert_eq!(
        syntax_kind_from_capture_name("punctuation.delimiter"),
        Some(SyntaxTokenKind::PunctuationDelimiter)
    );
    assert_eq!(
        syntax_kind_from_capture_name("punctuation.special"),
        Some(SyntaxTokenKind::PunctuationSpecial)
    );
    assert_eq!(
        syntax_kind_from_capture_name("punctuation.list_marker.markup"),
        Some(SyntaxTokenKind::PunctuationListMarker)
    );
    assert_eq!(
        syntax_kind_from_capture_name("punctuation.list_marker"),
        Some(SyntaxTokenKind::PunctuationListMarker)
    );
    assert_eq!(
        syntax_kind_from_capture_name("tag"),
        Some(SyntaxTokenKind::Tag)
    );
    assert_eq!(
        syntax_kind_from_capture_name("attribute"),
        Some(SyntaxTokenKind::Attribute)
    );
    assert_eq!(
        syntax_kind_from_capture_name("lifetime"),
        Some(SyntaxTokenKind::Lifetime)
    );
    assert_eq!(
        syntax_kind_from_capture_name("boolean"),
        Some(SyntaxTokenKind::Boolean)
    );
    assert_eq!(
        syntax_kind_from_capture_name("preproc"),
        Some(SyntaxTokenKind::Preproc)
    );
    assert_eq!(
        syntax_kind_from_capture_name("string.regex"),
        Some(SyntaxTokenKind::StringRegex)
    );
    assert_eq!(
        syntax_kind_from_capture_name("string.regexp"),
        Some(SyntaxTokenKind::StringRegex)
    );
    assert_eq!(
        syntax_kind_from_capture_name("string.special.regex"),
        Some(SyntaxTokenKind::StringRegex)
    );
    assert_eq!(
        syntax_kind_from_capture_name("string.special.symbol"),
        Some(SyntaxTokenKind::StringSpecial)
    );
    assert_eq!(
        syntax_kind_from_capture_name("constant.builtin"),
        Some(SyntaxTokenKind::ConstantBuiltin)
    );
    assert_eq!(
        syntax_kind_from_capture_name("markup.heading"),
        Some(SyntaxTokenKind::MarkupHeading)
    );
    assert_eq!(
        syntax_kind_from_capture_name("title.markup"),
        Some(SyntaxTokenKind::MarkupHeading)
    );
    assert_eq!(
        syntax_kind_from_capture_name("markup.link.url"),
        Some(SyntaxTokenKind::MarkupLink)
    );
    assert_eq!(
        syntax_kind_from_capture_name("link_uri.markup"),
        Some(SyntaxTokenKind::MarkupLink)
    );
    assert_eq!(
        syntax_kind_from_capture_name("text.uri"),
        Some(SyntaxTokenKind::MarkupLink)
    );
    assert_eq!(
        syntax_kind_from_capture_name("text.literal.markup"),
        Some(SyntaxTokenKind::TextLiteral)
    );
    assert_eq!(
        syntax_kind_from_capture_name("text.literal"),
        Some(SyntaxTokenKind::TextLiteral)
    );
    assert_eq!(
        syntax_kind_from_capture_name("text.title"),
        Some(SyntaxTokenKind::MarkupHeading)
    );
    assert_eq!(
        syntax_kind_from_capture_name("diff.plus"),
        Some(SyntaxTokenKind::DiffPlus)
    );
    assert_eq!(
        syntax_kind_from_capture_name("diff.minus"),
        Some(SyntaxTokenKind::DiffMinus)
    );
    assert_eq!(
        syntax_kind_from_capture_name("diff.delta"),
        Some(SyntaxTokenKind::DiffDelta)
    );
    assert_eq!(
        syntax_kind_from_capture_name("tag.jsx"),
        Some(SyntaxTokenKind::Tag)
    );
    assert_eq!(
        syntax_kind_from_capture_name("property.name"),
        Some(SyntaxTokenKind::Property)
    );
    assert_eq!(
        syntax_kind_from_capture_name("type.name"),
        Some(SyntaxTokenKind::Type)
    );
    assert_eq!(
        syntax_kind_from_capture_name("punctuation.bracket.html"),
        Some(SyntaxTokenKind::PunctuationBracket)
    );
    assert_eq!(
        syntax_kind_from_capture_name("punctuation.delimiter.jsx"),
        Some(SyntaxTokenKind::PunctuationDelimiter)
    );

    // Base names should still work
    assert_eq!(
        syntax_kind_from_capture_name("comment"),
        Some(SyntaxTokenKind::Comment)
    );
    assert_eq!(
        syntax_kind_from_capture_name("string"),
        Some(SyntaxTokenKind::String)
    );
    assert_eq!(
        syntax_kind_from_capture_name("keyword"),
        Some(SyntaxTokenKind::Keyword)
    );

    // Unknown dot-qualified names fall back through shorter dotted prefixes
    assert_eq!(
        syntax_kind_from_capture_name("keyword.operator.regex"),
        Some(SyntaxTokenKind::Keyword)
    );
    assert_eq!(
        syntax_kind_from_capture_name("comment.block"),
        Some(SyntaxTokenKind::Comment)
    );

    // Truly unknown names return None
    assert_eq!(syntax_kind_from_capture_name("none"), None);
    assert_eq!(syntax_kind_from_capture_name("embedded"), None);
    assert_eq!(syntax_kind_from_capture_name("text.jsx"), None);
}

#[test]
fn normalize_non_overlapping_tokens_keeps_later_same_range_token() {
    let tokens = normalize_non_overlapping_tokens(vec![
        SyntaxToken {
            range: 0..5,
            kind: SyntaxTokenKind::Function,
        },
        SyntaxToken {
            range: 0..5,
            kind: SyntaxTokenKind::Type,
        },
    ]);
    assert_eq!(
        tokens,
        vec![SyntaxToken {
            range: 0..5,
            kind: SyntaxTokenKind::Type,
        }]
    );
}

#[test]
fn normalize_non_overlapping_tokens_splits_outer_token_for_inner_semantics() {
    let tokens = normalize_non_overlapping_tokens(vec![
        SyntaxToken {
            range: 0..22,
            kind: SyntaxTokenKind::Comment,
        },
        SyntaxToken {
            range: 2..10,
            kind: SyntaxTokenKind::DiffPlus,
        },
        SyntaxToken {
            range: 12..22,
            kind: SyntaxTokenKind::StringSpecial,
        },
    ]);
    assert_eq!(
        tokens,
        vec![
            SyntaxToken {
                range: 0..2,
                kind: SyntaxTokenKind::Comment,
            },
            SyntaxToken {
                range: 2..10,
                kind: SyntaxTokenKind::DiffPlus,
            },
            SyntaxToken {
                range: 10..12,
                kind: SyntaxTokenKind::Comment,
            },
            SyntaxToken {
                range: 12..22,
                kind: SyntaxTokenKind::StringSpecial,
            },
        ]
    );
}

#[test]
fn normalize_non_overlapping_tokens_splits_contained_later_token() {
    let tokens = normalize_non_overlapping_tokens(vec![
        SyntaxToken {
            range: 0..10,
            kind: SyntaxTokenKind::Function,
        },
        SyntaxToken {
            range: 4..6,
            kind: SyntaxTokenKind::Operator,
        },
    ]);
    assert_eq!(
        tokens,
        vec![
            SyntaxToken {
                range: 0..4,
                kind: SyntaxTokenKind::Function,
            },
            SyntaxToken {
                range: 4..6,
                kind: SyntaxTokenKind::Operator,
            },
            SyntaxToken {
                range: 6..10,
                kind: SyntaxTokenKind::Function,
            },
        ]
    );
}

#[test]
fn normalize_non_overlapping_tokens_assigns_partial_overlap_to_later_token() {
    let tokens = normalize_non_overlapping_tokens(vec![
        SyntaxToken {
            range: 0..8,
            kind: SyntaxTokenKind::Comment,
        },
        SyntaxToken {
            range: 5..12,
            kind: SyntaxTokenKind::DiffMinus,
        },
    ]);
    assert_eq!(
        tokens,
        vec![
            SyntaxToken {
                range: 0..5,
                kind: SyntaxTokenKind::Comment,
            },
            SyntaxToken {
                range: 5..12,
                kind: SyntaxTokenKind::DiffMinus,
            },
        ]
    );
}

/// The warm-up runs on its own thread and races the render path by design.
/// This is a smoke test for the plumbing: repeated requests must be cheap and
/// must not deadlock against `OnceLock::get_or_init` on this thread.
#[test]
fn highlight_spec_warmup_requests_are_idempotent() {
    for _ in 0..3 {
        for language in [
            DiffSyntaxLanguage::Vue,
            DiffSyntaxLanguage::Html,
            DiffSyntaxLanguage::Markdown,
            DiffSyntaxLanguage::Rust,
        ] {
            request_highlight_spec_warmup(language);
        }
    }

    // Touching a warmed language from this thread must still return a spec,
    // whether this thread or the warm-up thread won the race.
    assert!(tree_sitter_highlight_spec(DiffSyntaxLanguage::Vue).is_some());
    assert!(tree_sitter_highlight_spec(DiffSyntaxLanguage::TypeScript).is_some());
}

/// `lang="…"` values are read out of the document at runtime, so unlike the
/// `#set!` targets above they cannot be enumerated from the query. Drive
/// them end to end instead: build a real SFC for each value and check the
/// block actually came out highlighted. Asserting on
/// `diff_syntax_language_for_code_fence_info` alone would not do -- that is
/// only half the path, and it would keep passing if the query rule that
/// forwards the attribute were deleted.

#[test]
fn grammar_and_highlight_spec_agree_on_supported_languages() {
    for lang in all_supported_languages() {
        let has_grammar = tree_sitter_grammar(lang).is_some();
        let has_spec = tree_sitter_highlight_spec(lang).is_some();
        assert_eq!(
            has_grammar, has_spec,
            "tree_sitter_grammar and tree_sitter_highlight_spec disagree for {lang:?}: \
             grammar={has_grammar}, spec={has_spec}"
        );
    }
}

// ---- Batch: Groovy, Clojure, Elixir, Erlang, Haskell, Julia, OCaml, ------
// ---- Solidity, Assembly, Svelte ------------------------------------------

/// Every extension and fence alias the batch claims, plus the collisions the
/// mapping had to avoid. Path resolution is the only thing standing between a
/// wired-up grammar and a file that still renders as plain text, and nothing
/// else in the suite exercises these arms.

/// A `.j2` says the file is templated, not that it is markup. Resolving a shell
/// or config template to the HTML-injecting reading hands the HTML grammar
/// `cat <<EOF` and `2>&1`, which open bogus elements.
#[test]
fn text_bodied_jinja_templates_do_not_get_html_injected() {
    for path in [
        "roles/web/templates/nginx.conf.j2",
        "charts/app/values.yaml.j2",
        "deploy/deploy.sh.j2",
        "docker-compose.yml.j2",
        "config/settings.ini.jinja",
        "db/schema.sql.j2",
    ] {
        assert_eq!(
            diff_syntax_language_for_path(path),
            Some(DiffSyntaxLanguage::JinjaText),
            "{path} has a non-markup body, so it must not inject HTML"
        );
    }

    let markup = tree_sitter_highlight_spec(DiffSyntaxLanguage::Jinja).expect("jinja spec");
    let text = tree_sitter_highlight_spec(DiffSyntaxLanguage::JinjaText).expect("text spec");
    assert!(
        markup.injection_query.is_some(),
        "the markup reading is the one that injects HTML"
    );
    assert!(
        text.injection_query.is_none(),
        "the text reading must have no injection query at all"
    );
    assert!(
        !text.has_combined_injections,
        "with no injection query there is no combined group to build"
    );
}

/// The shell-template shape that motivated the split, end to end.

#[test]
fn highlight_spec_exposes_ts_language() {
    let spec = tree_sitter_highlight_spec(DiffSyntaxLanguage::Rust)
        .expect("Rust highlight spec should exist");
    // Verify the ts_language field is usable for parsing
    with_ts_parser(&spec.ts_language, |_| ()).expect("should accept the spec's ts_language");
}
