use super::*;

#[test]
fn xml_has_own_language_variant() {
    assert_eq!(
        diff_syntax_language_for_path("foo.xml"),
        Some(DiffSyntaxLanguage::Xml)
    );
    assert_eq!(
        diff_syntax_language_for_path("layout.svg"),
        Some(DiffSyntaxLanguage::Xml)
    );
    // HTML stays separate
    assert_eq!(
        diff_syntax_language_for_path("index.html"),
        Some(DiffSyntaxLanguage::Html)
    );
}

#[test]
fn js_and_jsx_use_distinct_language_variants() {
    assert_eq!(
        diff_syntax_language_for_path("main.js"),
        Some(DiffSyntaxLanguage::JavaScript)
    );
    assert_eq!(
        diff_syntax_language_for_path("main.jsx"),
        Some(DiffSyntaxLanguage::Tsx)
    );
    assert_eq!(
        diff_syntax_language_for_path("main.tsx"),
        Some(DiffSyntaxLanguage::Tsx)
    );
}

#[test]
fn vue_extension_is_supported() {
    assert_eq!(
        diff_syntax_language_for_path("src/components/App.vue"),
        Some(DiffSyntaxLanguage::Vue)
    );
    // The same alias table backs injections and fenced code info strings.
    assert_eq!(
        diff_syntax_language_for_code_fence_info("vue"),
        Some(DiffSyntaxLanguage::Vue)
    );
}

#[test]
fn sql_extension_is_supported() {
    assert_eq!(
        diff_syntax_language_for_path("query.sql"),
        Some(DiffSyntaxLanguage::Sql)
    );
}

#[test]
fn markdown_extension_is_supported() {
    assert_eq!(
        diff_syntax_language_for_path("README.md"),
        Some(DiffSyntaxLanguage::Markdown)
    );
    assert_eq!(
        diff_syntax_language_for_path("notes.markdown"),
        Some(DiffSyntaxLanguage::Markdown)
    );
}

#[test]
fn markdown_heading_and_inline_code_are_highlighted() {
    let heading = syntax_tokens_for_line(
        "# Hello world",
        DiffSyntaxLanguage::Markdown,
        DiffSyntaxMode::Auto,
    );
    assert!(
        heading.iter().any(|t| t.kind == SyntaxTokenKind::Keyword),
        "expected markdown heading to be highlighted"
    );

    let inline = syntax_tokens_for_line(
        "Use `git status` here",
        DiffSyntaxLanguage::Markdown,
        DiffSyntaxMode::Auto,
    );
    assert!(
        inline.iter().any(|t| t.kind == SyntaxTokenKind::String),
        "expected markdown inline code to be highlighted"
    );
}

#[test]
fn markdown_inline_code_handles_unterminated_and_multibyte_spans_without_invalid_ranges() {
    for text in [
        "Use `cafe` here",
        "Use `café` here",
        "Use ``naïve `code` span`` here",
        "emoji `😀` end",
        "unterminated `😀",
        "`",
        "````",
        "prefix ``😀`` suffix",
    ] {
        let tokens = syntax_tokens_for_line_markdown(text);
        assert_token_ranges_are_utf8_safe(text, &tokens);
    }
}

#[test]
fn json_string_value_with_underscores_stays_one_string_token() {
    let line = r#"  "transition_policy": "adjacent_and_first","#;
    let key_start = line
        .find(r#""transition_policy""#)
        .expect("fixture should contain JSON key");
    let key_end = key_start + r#""transition_policy""#.len();
    let value_start = line
        .find(r#""adjacent_and_first""#)
        .expect("fixture should contain JSON string value");
    let value_end = value_start + r#""adjacent_and_first""#.len();

    let tokens = syntax_tokens_for_line(line, DiffSyntaxLanguage::Json, DiffSyntaxMode::Auto);

    assert!(
        tokens.iter().any(|token| {
            token.range == (key_start..key_end) && token.kind == SyntaxTokenKind::Property
        }),
        "JSON key should be highlighted as one property token: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|token| {
            token.range == (value_start..value_end) && token.kind == SyntaxTokenKind::String
        }),
        "JSON value should be highlighted as one string token: {tokens:?}"
    );
    assert!(
        !tokens.iter().any(|token| {
            token.range.start < key_end
                && key_start < token.range.end
                && token.kind != SyntaxTokenKind::Property
        }),
        "no non-property token should overlap the JSON key: {tokens:?}"
    );
    assert!(
        !tokens.iter().any(|token| {
            token.range.start < value_end
                && value_start < token.range.end
                && token.kind != SyntaxTokenKind::String
        }),
        "no non-string token should overlap the JSON value: {tokens:?}"
    );
}

/// Treating `'` as a quote painted the rest of the line as a string from the
/// tick in `foldl'` onward. HeuristicOnly is a production path for large diffs,
/// not just a fallback.
#[test]
fn nix_apostrophe_identifiers_do_not_open_a_string() {
    for line in [
        "  x = lib.foldl' add 0 xs;",
        "  y = builtins.mapAttrs' (n: v: v) set;",
        "  inherit (lib) foldl' concatMapAttrs';",
    ] {
        assert!(
            heuristic_string_spans(line, DiffSyntaxLanguage::Nix).is_empty(),
            "an apostrophe identifier opened a string in {line:?}: {:?}",
            heuristic_tokens(line, DiffSyntaxLanguage::Nix)
        );
    }

    // The tick is part of the identifier, so a keyword check sees the whole
    // name rather than a truncated prefix.
    let tokens = heuristic_tokens("  inherit' = 1;", DiffSyntaxLanguage::Nix);
    assert!(
        !tokens
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Keyword),
        "`inherit'` is not the keyword `inherit`: {tokens:?}"
    );

    // Double-quoted strings are untouched by any of this.
    assert_eq!(
        heuristic_string_spans("  z = \"literal\";", DiffSyntaxLanguage::Nix),
        vec!["\"literal\""]
    );
}

/// The reason the Nix arm exists instead of reusing the Hcl one: `//` is Nix's
/// update operator, so Hcl's `//` line comment would grey out the rest of the
/// line. Nothing else guards it -- every other Nix test takes the tree-sitter
/// path -- so folding Nix back into the `Hcl | Php` arm would pass the suite.

/// The reason the Nix arm exists instead of reusing the Hcl one: `//` is Nix's
/// update operator, so Hcl's `//` line comment would grey out the rest of the
/// line. Nothing else guards it -- every other Nix test takes the tree-sitter
/// path -- so folding Nix back into the `Hcl | Php` arm would pass the suite.
#[test]
fn nix_update_operator_is_not_a_line_comment() {
    let line = "  merged = { a = 1; } // { b = 2; };";
    let tokens = heuristic_tokens(line, DiffSyntaxLanguage::Nix);
    assert!(
        !tokens
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Comment),
        "the `//` update operator was greyed out as a comment: {tokens:?}"
    );

    // `#` still is one, and `/* */` too.
    let hashed = heuristic_tokens("  a = 1; # note", DiffSyntaxLanguage::Nix);
    assert!(
        hashed
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Comment),
        "`#` is Nix's line comment: {hashed:?}"
    );
    let blocked = heuristic_tokens("  a = /* note */ 1;", DiffSyntaxLanguage::Nix);
    assert!(
        blocked
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Comment),
        "`/* */` is Nix's block comment: {blocked:?}"
    );
}

/// Both new keyword tables, which nothing else reaches: every other Nix and
/// Jinja test goes through `prepare_test_document`, i.e. tree-sitter.

/// Both new keyword tables, which nothing else reaches: every other Nix and
/// Jinja test goes through `prepare_test_document`, i.e. tree-sitter.
#[test]
fn nix_and_jinja_heuristic_keyword_tables_are_covered() {
    let line = "  x = with pkgs; let y = 1; in rec { inherit y; }";
    let found = heuristic_keywords(line, DiffSyntaxLanguage::Nix);
    for expected in ["with", "let", "in", "rec", "inherit"] {
        assert!(
            found.contains(&expected),
            "Nix keyword `{expected}` missing from {found:?}"
        );
    }
    assert!(
        heuristic_keywords("  buildInputs = [ pkgs.hello ];", DiffSyntaxLanguage::Nix).is_empty(),
        "an ordinary Nix attribute name must not colour as a keyword"
    );

    // The Jinja table omits any identifier that could also be an HTML attribute
    // name or an English word: the heuristic sees the whole line.
    let found = heuristic_keywords("{% endif %}{% extends 'base' %}", DiffSyntaxLanguage::Jinja);
    for expected in ["endif", "extends"] {
        assert!(
            found.contains(&expected),
            "Jinja keyword `{expected}` missing from {found:?}"
        );
    }
    for prose in [
        "  <label for=\"name\">Name</label>",
        "  <p>Do it with care, and set it aside.</p>",
    ] {
        assert!(
            heuristic_keywords(prose, DiffSyntaxLanguage::Jinja).is_empty(),
            "an HTML attribute or English word coloured as a Jinja keyword in \
             {prose:?}: {:?}",
            heuristic_keywords(prose, DiffSyntaxLanguage::Jinja)
        );
    }

    // The text-bodied reading shares the table.
    assert_eq!(
        heuristic_keywords("{% endif %}", DiffSyntaxLanguage::JinjaText),
        heuristic_keywords("{% endif %}", DiffSyntaxLanguage::Jinja),
        "both Jinja readings must share one keyword table"
    );
}

/// Templates are mostly prose, and an unconditional single-quote rule painted
/// the rest of the line from the first `It's`.

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
fn vue_heuristic_fallback_uses_the_markup_dialect_for_the_whole_file() {
    let template_line = r#"  <img class="logo" src="//cdn.example.com/logo.png">"#;
    let tokens = syntax_tokens_for_line(
        template_line,
        DiffSyntaxLanguage::Vue,
        DiffSyntaxMode::HeuristicOnly,
    );
    assert!(
        !tokens
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Comment),
        "a protocol-relative URL in a template must not be greyed out as a `//` comment: \
         {tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(template_line, &tokens, SyntaxTokenKind::Keyword, "class"),
        "template attribute names must not render as JS keywords: {tokens:?}"
    );

    // The accepted cost, asserted so a future change to
    // `heuristic_comment_config` cannot flip it unnoticed.
    let script_line = "const count = 42; // note";
    let tokens = syntax_tokens_for_line(
        script_line,
        DiffSyntaxLanguage::Vue,
        DiffSyntaxMode::HeuristicOnly,
    );
    assert!(
        !tokens
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Comment),
        "known limitation: the heuristic cannot see that this line is inside <script>, so \
         `//` is not a comment here. If this now fails, the dialect was changed -- re-check \
         the template assertions above: {tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(script_line, &tokens, SyntaxTokenKind::Keyword, "const"),
        "known limitation: `const` is not a keyword in the markup dialect: {tokens:?}"
    );
}

/// Regression guard for the injection-per-directive blowup. Without the
/// `#not-match?` guards in vue_injections.scm every directive and every
/// interpolation became its own injected layer: ~5 per line here, which
/// overruns TS_INJECTION_CACHE_MAX_ENTRIES (32) and evicts half the cache
/// mid-render, so scrolling re-parses everything.
#[test]
fn vue_plain_binding_directives_produce_no_injections() {
    TS_INJECTION_CACHE.with(|cache| cache.borrow_mut().clear());

    let mut lines = vec!["<template>".to_string(), "  <ul>".to_string()];
    for ix in 0..30 {
        lines.push(format!(
            "    <li :key=\"row{ix}.id\" :class=\"row{ix}.cls\" \
             v-model=\"form.field{ix}\" @click=\"select{ix}\">{{{{ row{ix}.label }}}}</li>"
        ));
    }
    lines.push("  </ul>".to_string());
    lines.push("</template>".to_string());
    let line_count = lines.len();

    let doc = prepare_test_document(DiffSyntaxLanguage::Vue, &lines.join("\n"));
    for line_ix in 0..line_count {
        let _ = syntax_tokens_for_prepared_document_line(doc, line_ix);
    }

    let cached = TS_INJECTION_CACHE.with(|cache| cache.borrow().len());
    assert_eq!(
        cached, 0,
        "{line_count} lines of bare identifier / dotted-path bindings need no TypeScript \
         parse -- vue_highlights.scm already colours them -- but {cached} injection cache \
         entries were created (cap is {TS_INJECTION_CACHE_MAX_ENTRIES})"
    );

    TS_INJECTION_CACHE.with(|cache| cache.borrow_mut().clear());
}

/// The other half of the guard above: skipping plain bindings must not cost
/// highlighting for the expressions the injection actually exists to serve.

/// The other half of the guard above: skipping plain bindings must not cost
/// highlighting for the expressions the injection actually exists to serve.
#[test]
fn vue_expression_directives_still_inject_typescript() {
    let line = r#"  <button v-if="count > 10" @click="submit($event, 'now')">"#;
    let doc = prepare_vue_document(&["<template>", line, "</template>"]);

    let kinds = token_kinds_for_line_fragment(doc, 1, line, "count > 10");
    assert!(
        kinds.contains(&SyntaxTokenKind::Number),
        "an expression directive must still be parsed as TypeScript, got: {kinds:?}"
    );

    // `PreparedSyntaxDocument` is Copy, so the first handle is still usable.
    let kinds = token_kinds_for_line_fragment(doc, 1, line, "'now'");
    assert!(
        kinds.contains(&SyntaxTokenKind::String),
        "a call argument inside a directive should be parsed as TypeScript, got: {kinds:?}"
    );
}

/// Capturing the whole `(interpolation)` node paints the expression inside
/// it, not just the braces. Upstream relies on a companion `(raw_text) @none`
/// rule to punch the body back out, but `none` emits no token in this
/// engine, so the outer capture wins outright. That was invisible while
/// every interpolation was injected -- the injection carved the body out --
/// and became visible the moment plain interpolations stopped injecting.

/// Capturing the whole `(interpolation)` node paints the expression inside
/// it, not just the braces. Upstream relies on a companion `(raw_text) @none`
/// rule to punch the body back out, but `none` emits no token in this
/// engine, so the outer capture wins outright. That was invisible while
/// every interpolation was injected -- the injection carved the body out --
/// and became visible the moment plain interpolations stopped injecting.
#[test]
fn vue_plain_interpolation_does_not_paint_its_expression_as_a_sigil() {
    let line = r#"  <p>{{ msg }}</p>"#;
    let doc = prepare_vue_document(&["<template>", line, "</template>"]);

    let braces = token_kinds_for_line_fragment(doc, 1, line, "{{");
    assert!(
        braces.contains(&SyntaxTokenKind::PunctuationSpecial),
        "the interpolation delimiters should be sigil-coloured, got: {braces:?}"
    );

    let body = token_kinds_for_line_fragment(doc, 1, line, "msg");
    assert!(
        !body.contains(&SyntaxTokenKind::PunctuationSpecial),
        "the expression inside `{{{{ }}}}` must not inherit the delimiter colour, \
         got: {body:?}"
    );
}

/// The Vue grammar allows `v-if=ok` as well as `v-if="ok"`. Only the quoted
/// form has a `quoted_attribute_value`, so the unquoted one used to fall
/// through both the @variable override and the injection, landing on the
/// html `(attribute_value) @string` rule -- the exact miscolouring the
/// override exists to prevent.

/// The Vue grammar allows `v-if=ok` as well as `v-if="ok"`. Only the quoted
/// form has a `quoted_attribute_value`, so the unquoted one used to fall
/// through both the @variable override and the injection, landing on the
/// html `(attribute_value) @string` rule -- the exact miscolouring the
/// override exists to prevent.
#[test]
fn vue_unquoted_directive_value_is_not_coloured_as_a_string() {
    let line = r#"  <p v-if=ok>x</p>"#;
    let doc = prepare_vue_document(&["<template>", line, "</template>"]);
    let kinds = token_kinds_for_line_fragment(doc, 1, line, "ok");

    assert!(
        !kinds.contains(&SyntaxTokenKind::String),
        "an unquoted directive value is an expression, not a string: {kinds:?}"
    );
    assert!(
        !kinds.is_empty(),
        "an unquoted directive value should still be coloured, got nothing"
    );
}

/// `<script type="module" lang="ts">` matches a `type=` base rule and a
/// `lang=` vue rule over the same `raw_text`. prepared.rs tolerates the
/// duplicate by accident, but live.rs keeps both layers and interleaves
/// their captures at equal depth, so the editor colours the block
/// arbitrarily. The `lang` veto on the `type=` rules keeps it to one.

/// `<script type="module" lang="ts">` matches a `type=` base rule and a
/// `lang=` vue rule over the same `raw_text`. prepared.rs tolerates the
/// duplicate by accident, but live.rs keeps both layers and interleaves
/// their captures at equal depth, so the editor colours the block
/// arbitrarily. The `lang` veto on the `type=` rules keeps it to one.
#[test]
fn vue_script_with_both_type_and_lang_injects_exactly_one_language() {
    let text = "<script type=\"module\" lang=\"ts\">\nconst x: number = 1;\n</script>\n";

    let lang: tree_sitter::Language = tree_sitter_vue::LANGUAGE.into();
    let query = tree_sitter::Query::new(&lang, VUE_INJECTIONS_QUERY)
        .expect("vendored Vue injections.scm should compile");
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang)
        .expect("vendored Vue grammar should load");
    let tree = parser.parse(text, None).expect("script should parse");

    let mut cursor = tree_sitter::QueryCursor::new();
    cursor.set_match_limit(TS_QUERY_MATCH_LIMIT);
    let mut patterns = Vec::new();
    {
        let mut matches = cursor.matches(&query, tree.root_node(), text.as_bytes());
        tree_sitter::StreamingIterator::advance(&mut matches);
        while let Some(m) = matches.get() {
            patterns.push(m.pattern_index);
            tree_sitter::StreamingIterator::advance(&mut matches);
        }
    }

    assert_eq!(
        patterns.len(),
        1,
        "a script carrying both `type` and `lang` must match exactly one injection \
         pattern, matched {patterns:?}"
    );

    // …and it must be the TypeScript one, not the `type="module"` javascript one.
    let doc = prepare_test_document(DiffSyntaxLanguage::Vue, text);
    let tokens = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("script body should have prepared tokens");
    assert!(
        tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Type || t.kind == SyntaxTokenKind::TypeBuiltin),
        "`lang=\"ts\"` should win over `type=\"module\"`, so the `: number` annotation \
         should be typed: {tokens:?}"
    );
}

/// The directive guard does not cover the inherited attribute rules, so
/// those had to stop injecting unconditionally too. Inline `style=` was both
/// the worst offender and actively wrong (the CSS grammar reads an attribute
/// body as a stylesheet, making `color` a type selector), so it was dropped.

/// The directive guard does not cover the inherited attribute rules, so
/// those had to stop injecting unconditionally too. Inline `style=` was both
/// the worst offender and actively wrong (the CSS grammar reads an attribute
/// body as a stylesheet, making `color` a type selector), so it was dropped.
#[test]
fn vue_static_inline_styles_do_not_flood_the_injection_cache() {
    TS_INJECTION_CACHE.with(|cache| cache.borrow_mut().clear());

    let mut lines = vec!["<template>".to_string()];
    for ix in 0..40 {
        lines.push(format!("  <div style=\"color: red\" id=\"d{ix}\">x</div>"));
    }
    lines.push("</template>".to_string());
    let line_count = lines.len();

    let doc = prepare_test_document(DiffSyntaxLanguage::Vue, &lines.join("\n"));
    for line_ix in 0..line_count {
        let _ = syntax_tokens_for_prepared_document_line(doc, line_ix);
    }

    let cached = TS_INJECTION_CACHE.with(|cache| cache.borrow().len());
    assert_eq!(
        cached, 0,
        "static inline styles should not inject at all, but {cached} cache entries were \
         created from {line_count} lines (cap is {TS_INJECTION_CACHE_MAX_ENTRIES})"
    );

    TS_INJECTION_CACHE.with(|cache| cache.borrow_mut().clear());
}

/// A skipped injection still has to leave the value coloured -- that is the
/// premise the skip rests on.

/// A skipped injection still has to leave the value coloured -- that is the
/// premise the skip rests on.
#[test]
fn vue_plain_binding_directives_are_still_coloured_by_the_host_grammar() {
    let line = r#"  <div :class="wrapperClass">{{ label }}</div>"#;
    let doc = prepare_vue_document(&["<template>", line, "</template>"]);

    let kinds = token_kinds_for_line_fragment(doc, 1, line, "wrapperClass");
    assert!(
        !kinds.is_empty(),
        "a directive value skipped by the injection guard must still be coloured by \
         vue_highlights.scm, got nothing"
    );
    assert!(
        !kinds.contains(&SyntaxTokenKind::String),
        "…and must not fall back to the html `(attribute_value) @string` rule, got: {kinds:?}"
    );
}

/// Every language the Vue injections name has to be a language this
/// repository actually ships a grammar for, or the injection silently
/// no-ops. Reading the targets back off the compiled query keeps this
/// honest when the query changes.
#[test]
fn vue_injection_targets_resolve_to_working_grammars() {
    let lang: tree_sitter::Language = tree_sitter_vue::LANGUAGE.into();
    let query = tree_sitter::Query::new(&lang, VUE_INJECTIONS_QUERY)
        .expect("vendored Vue injections.scm should compile");

    let mut checked = 0;
    for pattern_ix in 0..query.pattern_count() {
        for setting in query.property_settings(pattern_ix) {
            if setting.key.as_ref() != "injection.language" {
                continue;
            }
            let Some(value) = setting.value.as_deref() else {
                continue;
            };
            let language = diff_syntax_language_for_code_fence_info(value).unwrap_or_else(|| {
                panic!("vue_injections.scm names an unknown injection language {value:?}")
            });
            assert!(
                tree_sitter_highlight_spec(language).is_some(),
                "vue_injections.scm injects {value:?}, but {language:?} has no grammar wired up",
            );
            checked += 1;
        }
    }
    assert!(
        checked >= 5,
        "expected several `#set! injection.language` targets in vue_injections.scm, found {checked}"
    );
}

/// The point of `request_highlight_spec_warmup` is to keep the expensive
/// specs off the render path, and the expensive one a `.vue` file reaches is
/// TypeScript (~86ms cold to compile, against Vue's own ~3ms). The warm-up
/// discovers its targets by walking `#set! injection.language` on the
/// compiled query, so a query edit that moved TypeScript behind an
/// `@injection.language` capture would silently stop warming it and put the
/// stall back. Assert it stays reachable the way the warm-up can see it.

/// The point of `request_highlight_spec_warmup` is to keep the expensive
/// specs off the render path, and the expensive one a `.vue` file reaches is
/// TypeScript (~86ms cold to compile, against Vue's own ~3ms). The warm-up
/// discovers its targets by walking `#set! injection.language` on the
/// compiled query, so a query edit that moved TypeScript behind an
/// `@injection.language` capture would silently stop warming it and put the
/// stall back. Assert it stays reachable the way the warm-up can see it.
#[test]
fn vue_spec_warmup_reaches_typescript_through_a_set_directive() {
    let lang: tree_sitter::Language = tree_sitter_vue::LANGUAGE.into();
    let query = tree_sitter::Query::new(&lang, VUE_INJECTIONS_QUERY)
        .expect("vendored Vue injections.scm should compile");

    let mut warmable = Vec::new();
    for pattern_ix in 0..query.pattern_count() {
        for setting in query.property_settings(pattern_ix) {
            if setting.key.as_ref() != "injection.language" {
                continue;
            }
            if let Some(language) = setting
                .value
                .as_deref()
                .and_then(diff_syntax_language_for_code_fence_info)
            {
                warmable.push(language);
            }
        }
    }

    assert!(
        warmable.contains(&DiffSyntaxLanguage::TypeScript),
        "TypeScript must stay reachable from a `#set! injection.language` in \
         vue_injections.scm, or the warm-up cannot pre-build the one spec that \
         actually costs anything. Reachable targets: {warmable:?}"
    );
    for language in warmable {
        assert!(
            tree_sitter_highlight_spec(language).is_some(),
            "the warm-up would try to build {language:?}, which has no grammar wired up",
        );
    }
}

/// The warm-up runs on its own thread and races the render path by design.
/// This is a smoke test for the plumbing: repeated requests must be cheap and
/// must not deadlock against `OnceLock::get_or_init` on this thread.

/// `lang="…"` values are read out of the document at runtime, so unlike the
/// `#set!` targets above they cannot be enumerated from the query. Drive
/// them end to end instead: build a real SFC for each value and check the
/// block actually came out highlighted. Asserting on
/// `diff_syntax_language_for_code_fence_info` alone would not do -- that is
/// only half the path, and it would keep passing if the query rule that
/// forwards the attribute were deleted.
#[test]
fn vue_lang_attribute_values_highlight_their_block() {
    // (lang, block body, tag, kind the injected grammar must produce)
    let cases: &[(&str, &str, &str, SyntaxTokenKind)] = &[
        (
            "css",
            ".a { color: red; }",
            "style",
            SyntaxTokenKind::Property,
        ),
        (
            "scss",
            ".a { color: red; }",
            "style",
            SyntaxTokenKind::Property,
        ),
        (
            "less",
            ".a { color: red; }",
            "style",
            SyntaxTokenKind::Property,
        ),
        (
            "postcss",
            ".a { color: red; }",
            "style",
            SyntaxTokenKind::Property,
        ),
        (
            "sass",
            ".a { color: red; }",
            "style",
            SyntaxTokenKind::Property,
        ),
        (
            "ts",
            "const value = 42;",
            "script",
            SyntaxTokenKind::Keyword,
        ),
        (
            "js",
            "const value = 42;",
            "script",
            SyntaxTokenKind::Keyword,
        ),
        (
            "tsx",
            "const value = 42;",
            "script",
            SyntaxTokenKind::Keyword,
        ),
        (
            "jsx",
            "const value = 42;",
            "script",
            SyntaxTokenKind::Keyword,
        ),
        // Not in any `#any-of?` list anywhere: it resolves purely through the
        // shared alias table, which is the whole point of forwarding the
        // attribute verbatim rather than enumerating values in the query.
        (
            "typescript",
            "const value = 42;",
            "script",
            SyntaxTokenKind::Keyword,
        ),
        (
            "mts",
            "const value = 42;",
            "script",
            SyntaxTokenKind::Keyword,
        ),
    ];

    for (lang, body, tag, expected) in cases {
        let text = format!("<{tag} lang=\"{lang}\">\n{body}\n</{tag}>");
        let doc = prepare_test_document(DiffSyntaxLanguage::Vue, &text);
        let tokens = syntax_tokens_for_prepared_document_line(doc, 1)
            .unwrap_or_else(|| panic!("`lang=\"{lang}\"` body should have prepared tokens"));
        assert!(
            tokens.iter().any(|t| t.kind == *expected),
            "`<{tag} lang=\"{lang}\">` should inject a grammar producing {expected:?}, \
             got: {tokens:?}"
        );
    }
}

/// The failure mode this guards is specific: the html base rules are vetoed
/// by `#not-match? "\\slang\\s*="`, so a `lang` the vue rules do not handle
/// used to lose the fallback *and* match nothing, leaving the block with no
/// highlighting whatsoever.

/// The failure mode this guards is specific: the html base rules are vetoed
/// by `#not-match? "\\slang\\s*="`, so a `lang` the vue rules do not handle
/// used to lose the fallback *and* match nothing, leaving the block with no
/// highlighting whatsoever.
#[test]
fn vue_unknown_lang_attribute_does_not_silently_disable_highlighting() {
    // A value no grammar in this repo can serve: no injection is the correct
    // outcome, and the surrounding markup must keep working. (Asserting only
    // `is_some()` here would be vacuous -- a blank block returns `Some(vec![])`.)
    let doc = prepare_vue_document(&["<script lang=\"coffee\">", "x = 1", "</script>"]);
    let tokens = syntax_tokens_for_prepared_document_line(doc, 0)
        .expect("the opening tag should have prepared tokens");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Tag),
        "an unservable lang must not disturb the host grammar: {tokens:?}"
    );

    // …and the servable-but-unenumerated case really does highlight.
    let doc = prepare_vue_document(&["<style lang=\"pcss\">", ".a { color: red; }", "</style>"]);
    let tokens = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("style body should have prepared tokens");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Property),
        "`lang=\"pcss\"` resolves to Css through the alias table and must highlight: \
         {tokens:?}"
    );
}

/// The tree-sitter parser is a thread-local reused across languages, with a
/// fast path that skips `set_language`. Adding a grammar that is loaded a
/// different way (vendored, not from crates.io) should not disturb that.

/// The tree-sitter parser is a thread-local reused across languages, with a
/// fast path that skips `set_language`. Adding a grammar that is loaded a
/// different way (vendored, not from crates.io) should not disturb that.
#[test]
fn vue_documents_interleave_with_other_language_documents() {
    let vue = prepare_vue_document(VUE_SFC_FIXTURE);
    let html = prepare_html_document(&["<style>", "body { color: red; }", "</style>"]);
    let vue_again = prepare_vue_document(VUE_SFC_FIXTURE);

    assert!(
        syntax_tokens_for_prepared_document_line(html, 1)
            .expect("html line tokens should be available")
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Property),
        "an HTML document prepared after a Vue one should still highlight"
    );
    for doc in [vue, vue_again] {
        assert!(
            syntax_tokens_for_prepared_document_line(doc, 7)
                .expect("vue script line tokens should be available")
                .iter()
                .any(|t| t.kind == SyntaxTokenKind::Keyword),
            "Vue documents should still highlight when interleaved with other languages"
        );
    }
}

/// vue_highlights.scm inlines queries/html_highlights.scm, because the Vue
/// grammar inherits html and TreesitterQueryAsset takes a single source.
/// Nothing structural keeps the copy honest, so an edit to the html file
/// that is not mirrored here would silently skip .vue files.
///
/// The check is order-sensitive on purpose. Rule order is load-bearing:
/// `normalize_non_overlapping_tokens` resolves overlaps last-capture-wins,
/// so the html `(attribute_value) @string` rule has to come *before* the
/// `@variable` directive override for the override to take effect. A
/// per-line containment check would pass on a reordered copy.
#[test]
fn vue_highlights_query_embeds_the_html_base_verbatim() {
    let html_rules = query_rule_lines(HTML_HIGHLIGHTS_QUERY);
    let vue_rules = query_rule_lines(VUE_HIGHLIGHTS_QUERY);
    assert!(
        !html_rules.is_empty(),
        "html_highlights.scm should not be comment-only"
    );

    let embedded = vue_rules
        .windows(html_rules.len())
        .any(|window| window == html_rules.as_slice());
    assert!(
        embedded,
        "vue_highlights.scm must contain queries/html_highlights.scm as a contiguous, \
         in-order block -- the Vue grammar inherits html, and rule order decides which \
         capture wins. Mirror the change into the `--- html base ---` section.\n\
         expected block:\n{html_rules:#?}\nvue rules:\n{vue_rules:#?}"
    );
}

/// `configure_query_cursor` caps in-progress matches at TS_QUERY_MATCH_LIMIT
/// and nothing consults `did_exceed_match_limit`, so an overflow silently
/// drops injections. The Vue injection query has the most patterns of any in
/// the repo and anchors several on very common template nodes, which makes
/// it the one most likely to hit the cap.

/// `configure_query_cursor` caps in-progress matches at TS_QUERY_MATCH_LIMIT
/// and nothing consults `did_exceed_match_limit`, so an overflow silently
/// drops injections. The Vue injection query has the most patterns of any in
/// the repo and anchors several on very common template nodes, which makes
/// it the one most likely to hit the cap.
#[test]
fn vue_injection_query_stays_under_the_match_limit_on_a_dense_template() {
    let mut lines = vec!["<template>".to_string(), "  <ul>".to_string()];
    for ix in 0..120 {
        lines.push(format!(
            "    <li v-if=\"n{ix} > {ix}\" :key=\"k{ix}\" :class=\"[a{ix}, b{ix}]\" \
             @click.stop=\"pick{ix}($event)\" #row=\"{{ v{ix} }}\">{{{{ n{ix} + 1 }}}}</li>"
        ));
    }
    lines.push("  </ul>".to_string());
    lines.push("</template>".to_string());
    let text = lines.join("\n");

    let lang: tree_sitter::Language = tree_sitter_vue::LANGUAGE.into();
    let query = tree_sitter::Query::new(&lang, VUE_INJECTIONS_QUERY)
        .expect("vendored Vue injections.scm should compile");
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang)
        .expect("vendored Vue grammar should load");
    let tree = parser
        .parse(&text, None)
        .expect("dense template should parse");

    let mut cursor = tree_sitter::QueryCursor::new();
    cursor.set_match_limit(TS_QUERY_MATCH_LIMIT);
    let mut matched = 0usize;
    {
        let mut matches = cursor.matches(&query, tree.root_node(), text.as_bytes());
        tree_sitter::StreamingIterator::advance(&mut matches);
        while matches.get().is_some() {
            matched += 1;
            tree_sitter::StreamingIterator::advance(&mut matches);
        }
    }

    assert!(
        !cursor.did_exceed_match_limit(),
        "the Vue injection query overflowed the {TS_QUERY_MATCH_LIMIT}-match in-progress \
         pool on a {}-line template; tree-sitter discards matches on overflow, so some \
         directives and interpolations would silently lose highlighting",
        lines.len(),
    );
    assert!(
        matched > 0,
        "the dense template should produce injection matches at all"
    );
}

#[test]
fn xml_treesitter_captures_tag_and_attribute() {
    let text = r#"<root attr="value">text</root>"#;
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::Xml, DiffSyntaxMode::Auto);
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Tag),
        "XML should capture tags: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Property),
        "XML should capture attributes as properties: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::String),
        "XML should capture attribute values as strings: {tokens:?}"
    );
}

#[test]
fn xml_treesitter_captures_comment() {
    let text = "<!-- a comment -->";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::Xml, DiffSyntaxMode::Auto);
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Comment),
        "XML should capture comments: {tokens:?}"
    );
}

#[test]
fn javascript_treesitter_captures_function_and_keyword() {
    let text = "function foo() { return 42; }";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::JavaScript, DiffSyntaxMode::Auto);
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Function),
        "JS should capture function names: {tokens:?}"
    );
    assert!(
        tokens.iter().any(
            |t| t.kind == SyntaxTokenKind::Keyword || t.kind == SyntaxTokenKind::KeywordControl
        ),
        "JS should capture keywords: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Number),
        "JS should capture numbers: {tokens:?}"
    );
}

#[test]
fn typescript_treesitter_preserves_arrow_operator_inside_arrow_function() {
    let text = "const fn = (x: number): number => x + 1;";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::TypeScript, DiffSyntaxMode::Auto);
    assert!(
        tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Function && &text[t.range.clone()] == "fn"),
        "TypeScript should capture the arrow function name, got: {tokens:?}"
    );
    assert!(
        tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Operator && &text[t.range.clone()] == "=>"),
        "TypeScript should preserve the fat-arrow operator, got: {tokens:?}"
    );
}

#[test]
fn html_highlight_spec_compiles_injection_query() {
    let spec = tree_sitter_highlight_spec(DiffSyntaxLanguage::Html)
        .expect("HTML highlight spec should exist");
    assert!(
        spec.injection_query.is_some(),
        "HTML should compile and retain its vendored injections.scm"
    );
}

#[test]
fn javascript_highlight_spec_compiles_injection_query() {
    let spec = tree_sitter_highlight_spec(DiffSyntaxLanguage::JavaScript)
        .expect("JavaScript highlight spec should exist");
    assert!(
        spec.injection_query.is_some(),
        "JavaScript should compile and retain its injections.scm"
    );
}

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

#[test]
fn rust_treesitter_captures_variable_parameter() {
    let text = "fn foo(bar: u32) {}";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::VariableParameter),
        "Rust function parameter should produce VariableParameter token, got: {tokens:?}"
    );
}

#[test]
fn rust_treesitter_captures_self_as_variable_special() {
    let text = "impl Widget { fn render(&self, item: Item) { self.draw(item); } }";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::VariableSpecial, "self"),
        "Rust `self` should produce VariableSpecial token, got: {tokens:?}"
    );
}

#[test]
fn rust_treesitter_captures_type_builtin() {
    let text = "let x: u32 = 0;";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::TypeBuiltin),
        "Rust primitive type should produce TypeBuiltin token, got: {tokens:?}"
    );
}

#[test]
fn rust_treesitter_captures_macro_as_function_special() {
    let text = "println!(\"hello\");";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::FunctionSpecial),
        "Rust macro invocation should produce FunctionSpecial token, got: {tokens:?}"
    );
}

#[test]
fn rust_treesitter_captures_keyword_function_type_and_string_families() {
    let text = r#"fn foo(bar: u32) { let x = "hi"; }"#;
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Keyword, "fn"),
        "Rust should highlight `fn` as a keyword, got: {tokens:?}"
    );
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Function, "foo"),
        "Rust function declarations should capture the function name, got: {tokens:?}"
    );
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Keyword, "let"),
        "Rust should highlight `let` as a keyword, got: {tokens:?}"
    );
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::TypeBuiltin, "u32"),
        "Rust primitive types should keep their dedicated type token, got: {tokens:?}"
    );
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::String, "\"hi\""),
        "Rust string literals should produce String tokens, got: {tokens:?}"
    );
}

#[test]
fn rust_treesitter_captures_impl_family_as_preproc() {
    let impl_text = "impl Widget where T: Trait {}";
    let impl_tokens =
        syntax_tokens_for_line(impl_text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(impl_text, &impl_tokens, SyntaxTokenKind::Preproc, "impl"),
        "Rust `impl` should route through Preproc for the violet family, got: {impl_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(impl_text, &impl_tokens, SyntaxTokenKind::Preproc, "where"),
        "Rust `where` should route through Preproc, got: {impl_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(impl_text, &impl_tokens, SyntaxTokenKind::Type, "Widget"),
        "Rust impl targets should keep their type token, got: {impl_tokens:?}"
    );

    let trait_text = "trait Painter where Self: Sized {}";
    let trait_tokens =
        syntax_tokens_for_line(trait_text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(trait_text, &trait_tokens, SyntaxTokenKind::Preproc, "trait"),
        "Rust `trait` should route through Preproc, got: {trait_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(trait_text, &trait_tokens, SyntaxTokenKind::Preproc, "where"),
        "Rust `where` should stay violet in trait declarations, got: {trait_tokens:?}"
    );

    let dyn_text = "let painter: dyn Painter = todo!();";
    let dyn_tokens =
        syntax_tokens_for_line(dyn_text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(dyn_text, &dyn_tokens, SyntaxTokenKind::Preproc, "dyn"),
        "Rust `dyn` should route through Preproc, got: {dyn_tokens:?}"
    );
}

#[test]
fn rust_treesitter_captures_use_roots_and_tails() {
    let type_text = "use foo::Bar;";
    let type_tokens =
        syntax_tokens_for_line(type_text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(type_text, &type_tokens, SyntaxTokenKind::Preproc, "foo"),
        "Non-`crate` import roots should route through Preproc, got: {type_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(type_text, &type_tokens, SyntaxTokenKind::Type, "Bar"),
        "Imported uppercase tails should keep their type token, got: {type_tokens:?}"
    );

    let function_text = "use foo::bar;";
    let function_tokens = syntax_tokens_for_line(
        function_text,
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
    );
    assert!(
        has_token_kind_and_text(
            function_text,
            &function_tokens,
            SyntaxTokenKind::Preproc,
            "foo",
        ),
        "Non-`crate` import roots should stay violet, got: {function_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(
            function_text,
            &function_tokens,
            SyntaxTokenKind::Function,
            "bar",
        ),
        "Imported lowercase tails should route through Function, got: {function_tokens:?}"
    );
}

#[test]
fn rust_treesitter_keeps_use_middle_modules_neutral() {
    let type_text = "use foo::bar::Baz;";
    let type_tokens =
        syntax_tokens_for_line(type_text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(type_text, &type_tokens, SyntaxTokenKind::Preproc, "foo"),
        "The top import root should stay violet, got: {type_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(type_text, &type_tokens, SyntaxTokenKind::Type, "Baz"),
        "The imported type should stay green, got: {type_tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(type_text, &type_tokens, SyntaxTokenKind::Preproc, "bar"),
        "Middle modules should not inherit the root violet accent, got: {type_tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(type_text, &type_tokens, SyntaxTokenKind::Function, "bar"),
        "Middle modules should not be recolored as imported tails, got: {type_tokens:?}"
    );

    let crate_type_text = "use crate::foo::Bar;";
    let crate_type_tokens = syntax_tokens_for_line(
        crate_type_text,
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
    );
    assert!(
        has_token_kind_and_text(
            crate_type_text,
            &crate_type_tokens,
            SyntaxTokenKind::Keyword,
            "crate",
        ),
        "Rust should keep `crate` on the keyword/orange family, got: {crate_type_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(
            crate_type_text,
            &crate_type_tokens,
            SyntaxTokenKind::Type,
            "Bar",
        ),
        "Imported types under `crate` should stay green, got: {crate_type_tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(
            crate_type_text,
            &crate_type_tokens,
            SyntaxTokenKind::Preproc,
            "foo",
        ),
        "The segment after `crate::` should stay neutral, got: {crate_type_tokens:?}"
    );

    let crate_function_text = "use crate::foo::bar;";
    let crate_function_tokens = syntax_tokens_for_line(
        crate_function_text,
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
    );
    assert!(
        has_token_kind_and_text(
            crate_function_text,
            &crate_function_tokens,
            SyntaxTokenKind::Function,
            "bar",
        ),
        "The final lowercase import tail should stay blue under `crate`, got: {crate_function_tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(
            crate_function_text,
            &crate_function_tokens,
            SyntaxTokenKind::Preproc,
            "foo",
        ),
        "The segment after `crate::` should remain neutral, got: {crate_function_tokens:?}"
    );
}

#[test]
fn rust_treesitter_captures_root_modules_before_functions_and_types() {
    let call_text = "let handler = foo::bar::baz();";
    let call_tokens =
        syntax_tokens_for_line(call_text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(call_text, &call_tokens, SyntaxTokenKind::Preproc, "foo"),
        "Rust code paths should color the bare root module as Preproc, got: {call_tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(call_text, &call_tokens, SyntaxTokenKind::Preproc, "bar"),
        "Inner code-path modules should stay neutral instead of inheriting the root violet, got: {call_tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(call_text, &call_tokens, SyntaxTokenKind::Function, "bar"),
        "Inner code-path modules should not be recolored as callable tails, got: {call_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(call_text, &call_tokens, SyntaxTokenKind::Function, "baz"),
        "Rust function paths should keep the callable name as Function, got: {call_tokens:?}"
    );

    let associated_text = "let factory = foo::bar::Baz::new();";
    let associated_tokens = syntax_tokens_for_line(
        associated_text,
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
    );
    assert!(
        has_token_kind_and_text(
            associated_text,
            &associated_tokens,
            SyntaxTokenKind::Preproc,
            "foo",
        ),
        "Associated paths should keep the bare root module violet, got: {associated_tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(
            associated_text,
            &associated_tokens,
            SyntaxTokenKind::Preproc,
            "bar",
        ),
        "Inner modules before associated functions should stay neutral, got: {associated_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(
            associated_text,
            &associated_tokens,
            SyntaxTokenKind::Type,
            "Baz",
        ),
        "Associated function paths should keep the type token, got: {associated_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(
            associated_text,
            &associated_tokens,
            SyntaxTokenKind::Function,
            "new",
        ),
        "Associated function paths should keep the callable name as Function, got: {associated_tokens:?}"
    );

    let crate_text = "let value: crate::foo::Bar = todo!();";
    let crate_tokens =
        syntax_tokens_for_line(crate_text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(crate_text, &crate_tokens, SyntaxTokenKind::Keyword, "crate"),
        "Rust should keep `crate` on the keyword/orange family, got: {crate_tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(crate_text, &crate_tokens, SyntaxTokenKind::Preproc, "foo"),
        "The first named segment after `crate::` should stay neutral in code paths, got: {crate_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(crate_text, &crate_tokens, SyntaxTokenKind::Type, "Bar"),
        "Rust type tails under `crate` should stay green, got: {crate_tokens:?}"
    );
}

#[test]
fn rust_treesitter_captures_constants_in_scoped_paths() {
    let constant_text = "let mode = NotForContentType::SSE;";
    let constant_tokens = syntax_tokens_for_line(
        constant_text,
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
    );
    assert!(
        has_token_kind_and_text(
            constant_text,
            &constant_tokens,
            SyntaxTokenKind::Type,
            "NotForContentType",
        ),
        "Rust should keep the type side of associated constants green, got: {constant_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(
            constant_text,
            &constant_tokens,
            SyntaxTokenKind::Constant,
            "SSE",
        ),
        "Rust ALL_CAPS associated constants should route through Constant, got: {constant_tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(
            constant_text,
            &constant_tokens,
            SyntaxTokenKind::Type,
            "SSE",
        ),
        "Rust ALL_CAPS associated constants should no longer be typed green, got: {constant_tokens:?}"
    );

    let scoped_text = "let root = foo::BAR;";
    let scoped_tokens =
        syntax_tokens_for_line(scoped_text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(scoped_text, &scoped_tokens, SyntaxTokenKind::Preproc, "foo"),
        "Bare module roots should stay violet before constant tails, got: {scoped_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(
            scoped_text,
            &scoped_tokens,
            SyntaxTokenKind::Constant,
            "BAR",
        ),
        "Scoped ALL_CAPS references should route through Constant, got: {scoped_tokens:?}"
    );

    let crate_scoped_text = "let root = crate::foo::BAR;";
    let crate_scoped_tokens = syntax_tokens_for_line(
        crate_scoped_text,
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
    );
    assert!(
        has_token_kind_and_text(
            crate_scoped_text,
            &crate_scoped_tokens,
            SyntaxTokenKind::Keyword,
            "crate",
        ),
        "Rust should keep `crate` orange before constant tails, got: {crate_scoped_tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(
            crate_scoped_text,
            &crate_scoped_tokens,
            SyntaxTokenKind::Preproc,
            "foo",
        ),
        "The first named segment after `crate::` should stay neutral before constants, got: {crate_scoped_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(
            crate_scoped_text,
            &crate_scoped_tokens,
            SyntaxTokenKind::Constant,
            "BAR",
        ),
        "ALL_CAPS constant tails under `crate` should stay pink/Constant, got: {crate_scoped_tokens:?}"
    );

    let standalone_text = "let standalone = SSE;";
    let standalone_tokens = syntax_tokens_for_line(
        standalone_text,
        DiffSyntaxLanguage::Rust,
        DiffSyntaxMode::Auto,
    );
    assert!(
        has_token_kind_and_text(
            standalone_text,
            &standalone_tokens,
            SyntaxTokenKind::Constant,
            "SSE",
        ),
        "Standalone ALL_CAPS Rust names should route through Constant, got: {standalone_tokens:?}"
    );
}

#[test]
fn rust_treesitter_captures_grouped_use_import_semantics() {
    let text = "use foo::{bar, baz::Qux};";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Preproc, "foo"),
        "Grouped imports should accent the non-`crate` root, got: {tokens:?}"
    );
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Function, "bar"),
        "Grouped imports should keep lowercase imported tails blue, got: {tokens:?}"
    );
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Type, "Qux"),
        "Grouped imports should keep uppercase imported tails green, got: {tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Preproc, "baz"),
        "Grouped middle modules should not inherit the root violet accent, got: {tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Function, "baz"),
        "Grouped middle modules should stay neutral when importing a type, got: {tokens:?}"
    );

    let crate_text = "use crate::{foo::bar, baz::Qux};";
    let crate_tokens =
        syntax_tokens_for_line(crate_text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(crate_text, &crate_tokens, SyntaxTokenKind::Keyword, "crate",),
        "Grouped imports should keep `crate` on the keyword/orange family, got: {crate_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(crate_text, &crate_tokens, SyntaxTokenKind::Function, "bar",),
        "Grouped imports under `crate` should keep lowercase tails blue, got: {crate_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(crate_text, &crate_tokens, SyntaxTokenKind::Type, "Qux"),
        "Grouped imports under `crate` should keep uppercase tails green, got: {crate_tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(crate_text, &crate_tokens, SyntaxTokenKind::Preproc, "foo",),
        "Paths under `crate::{{...}}` should not add a violet root accent, got: {crate_tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(crate_text, &crate_tokens, SyntaxTokenKind::Function, "baz",),
        "Middle grouped modules should stay neutral before imported types, got: {crate_tokens:?}"
    );
}

#[test]
fn rust_treesitter_keeps_use_aliases_neutral() {
    let text = "use foo::bar as baz;";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::Rust, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Preproc, "foo"),
        "Aliased imports should keep the non-`crate` root violet, got: {tokens:?}"
    );
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Function, "bar"),
        "Aliased imports should keep the source tail blue, got: {tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Preproc, "baz"),
        "Import aliases should stay neutral instead of inheriting the root accent, got: {tokens:?}"
    );
    assert!(
        !has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Function, "baz"),
        "Import aliases should stay neutral instead of inheriting the source tail color, got: {tokens:?}"
    );
}

#[test]
fn tsx_treesitter_highlights_jsx_tag_and_attribute() {
    let text = "const node = <button disabled />;";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::Tsx, DiffSyntaxMode::Auto);
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Tag),
        "TSX should highlight JSX tags, got: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Attribute),
        "TSX should highlight JSX attributes, got: {tokens:?}"
    );
}

#[test]
fn css_treesitter_captures_property_and_keyword() {
    let text = "@media screen { .foo { color: red; } }";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::Css, DiffSyntaxMode::Auto);
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Keyword),
        "CSS should highlight @media as keyword: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Property),
        "CSS should highlight 'color' as property: {tokens:?}"
    );
}

#[test]
fn javascript_tagged_template_injects_css() {
    let document = prepare_test_document(
        DiffSyntaxLanguage::JavaScript,
        "const styles = css`color: red;`;",
    );
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("JavaScript document should have prepared tokens");
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Property),
        "tagged CSS template should inject CSS property highlighting: {tokens:?}"
    );
}

#[test]
fn javascript_tagged_template_injects_html() {
    let text = "const markup = html`<div class=\"note\">ok</div>`;";
    let document = prepare_test_document(DiffSyntaxLanguage::JavaScript, text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("JavaScript document should have prepared tokens");
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Tag, "div"),
        "tagged HTML template should inject HTML tags in JavaScript: {tokens:?}"
    );
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Attribute, "class"),
        "tagged HTML template should inject HTML attributes in JavaScript: {tokens:?}"
    );
}

#[test]
fn javascript_styled_member_template_injects_css() {
    let text = "const Button = styled.div`color: red;`;";
    let document = prepare_test_document(DiffSyntaxLanguage::JavaScript, text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("JavaScript document should have prepared tokens");
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Property, "color"),
        "styled member templates should inject CSS properties in JavaScript: {tokens:?}"
    );
}

#[test]
fn javascript_styled_call_template_injects_css() {
    let text = "const Button = styled(Link)`color: red;`;";
    let document = prepare_test_document(DiffSyntaxLanguage::JavaScript, text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("JavaScript document should have prepared tokens");
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Property, "color"),
        "styled call templates should inject CSS properties in JavaScript: {tokens:?}"
    );
}

#[test]
fn javascript_comment_prefixed_string_injects_html() {
    let text = r#"const markup = /* html */ "<div class='note'>ok</div>";"#;
    let document = prepare_test_document(DiffSyntaxLanguage::JavaScript, text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("JavaScript document should have prepared tokens");
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Tag, "div"),
        "comment-prefixed HTML string should inject HTML tags: {tokens:?}"
    );
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Attribute, "class"),
        "comment-prefixed HTML string should inject HTML attributes: {tokens:?}"
    );
}

#[test]
fn javascript_comment_prefixed_string_injects_css() {
    let text = r#"const styles = /* css */ "color: red;";"#;
    let document = prepare_test_document(DiffSyntaxLanguage::JavaScript, text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("JavaScript document should have prepared tokens");
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Property, "color"),
        "comment-prefixed CSS strings should inject CSS properties in JavaScript: {tokens:?}"
    );
}

#[test]
fn javascript_comment_prefixed_template_literal_injects_css() {
    let text = "const styles = /* css */ `color: red;`;";
    let document = prepare_test_document(DiffSyntaxLanguage::JavaScript, text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("JavaScript document should have prepared tokens");
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Property, "color"),
        "comment-prefixed CSS template literals should inject CSS properties: {tokens:?}"
    );
}

#[test]
fn typescript_tagged_template_injects_yaml() {
    let text = "const workflow = yaml`enabled: true`;";
    let document = prepare_test_document(DiffSyntaxLanguage::TypeScript, text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("TypeScript document should have prepared tokens");
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Property, "enabled"),
        "tagged YAML template should inject YAML properties: {tokens:?}"
    );
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Boolean, "true"),
        "tagged YAML template should inject YAML booleans: {tokens:?}"
    );
}

#[test]
fn typescript_tagged_template_injects_html() {
    let text = "const markup = html`<div class=\"note\">ok</div>`;";
    let document = prepare_test_document(DiffSyntaxLanguage::TypeScript, text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("TypeScript document should have prepared tokens");
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Tag, "div"),
        "tagged HTML template should inject HTML tags in TypeScript: {tokens:?}"
    );
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Attribute, "class"),
        "tagged HTML template should inject HTML attributes in TypeScript: {tokens:?}"
    );
}

#[test]
fn typescript_tagged_template_injects_sql() {
    let text = "const query = sql`select name from users`;";
    let document = prepare_test_document(DiffSyntaxLanguage::TypeScript, text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("TypeScript document should have prepared tokens");
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Keyword, "select"),
        "tagged SQL template should inject SQL keywords in TypeScript: {tokens:?}"
    );
}

#[test]
fn typescript_comment_prefixed_string_injects_css() {
    let text = r#"const styles = /* css */ "color: red;";"#;
    let document = prepare_test_document(DiffSyntaxLanguage::TypeScript, text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("TypeScript document should have prepared tokens");
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Property, "color"),
        "comment-prefixed CSS strings should inject CSS properties in TypeScript: {tokens:?}"
    );
}

#[test]
fn typescript_component_styles_array_template_injects_css() {
    let text = "Component({ styles: [`div { color: red; }`] });";
    let document = prepare_test_document(DiffSyntaxLanguage::TypeScript, text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("TypeScript document should have prepared tokens");
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Property, "color"),
        "TypeScript Component styles templates should inject CSS properties: {tokens:?}"
    );
}

#[test]
fn tsx_tagged_template_injects_html() {
    let text = "const markup = html`<div class=\"note\">ok</div>`;";
    let document = prepare_test_document(DiffSyntaxLanguage::Tsx, text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("TSX document should have prepared tokens");
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Tag, "div"),
        "tagged HTML template should inject HTML tags in TSX: {tokens:?}"
    );
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Attribute, "class"),
        "tagged HTML template should inject HTML attributes in TSX: {tokens:?}"
    );
}

#[test]
fn go_treesitter_captures_function_method_property_and_number() {
    let declaration = "func Hello(a B) C { return C{} }";
    let declaration_tokens =
        syntax_tokens_for_line(declaration, DiffSyntaxLanguage::Go, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(
            declaration,
            &declaration_tokens,
            SyntaxTokenKind::Function,
            "Hello",
        ),
        "Go should capture function declarations: {declaration_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(declaration, &declaration_tokens, SyntaxTokenKind::Type, "B"),
        "Go should capture parameter types: {declaration_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(declaration, &declaration_tokens, SyntaxTokenKind::Type, "C"),
        "Go should capture return or composite literal types: {declaration_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(
            declaration,
            &declaration_tokens,
            SyntaxTokenKind::Keyword,
            "return",
        ),
        "Go should capture keywords: {declaration_tokens:?}"
    );

    let method_call = "value.Do(42)";
    let method_call_tokens =
        syntax_tokens_for_line(method_call, DiffSyntaxLanguage::Go, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(
            method_call,
            &method_call_tokens,
            SyntaxTokenKind::FunctionMethod,
            "Do",
        ),
        "Go should capture method calls: {method_call_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(
            method_call,
            &method_call_tokens,
            SyntaxTokenKind::Number,
            "42",
        ),
        "Go should capture numeric literals: {method_call_tokens:?}"
    );

    let field_access = "value.Field";
    let field_access_tokens =
        syntax_tokens_for_line(field_access, DiffSyntaxLanguage::Go, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(
            field_access,
            &field_access_tokens,
            SyntaxTokenKind::Property,
            "Field",
        ),
        "Go should capture field accesses: {field_access_tokens:?}"
    );
}

#[test]
fn go_comment_prefixed_strings_inject_supported_languages() {
    let cases = [
        (
            r#"var payload = /* json */ `{"count": 42}`;"#,
            SyntaxTokenKind::Number,
            "42",
        ),
        (
            r#"var config = /* yaml */ `enabled: true`;"#,
            SyntaxTokenKind::Boolean,
            "true",
        ),
        (
            r#"var markup = /* html */ `<div class="note">ok</div>`;"#,
            SyntaxTokenKind::Tag,
            "div",
        ),
        (
            r#"var markup = /* xml */ `<root attr="value"/>`;"#,
            SyntaxTokenKind::Tag,
            "root",
        ),
        (
            r#"var script = /* js */ `const value = 42;`;"#,
            SyntaxTokenKind::Number,
            "42",
        ),
        (
            r#"var query = /* sql */ `select name from users`;"#,
            SyntaxTokenKind::Keyword,
            "select",
        ),
    ];

    for (text, expected_kind, expected_text) in cases {
        let document = prepare_test_document(DiffSyntaxLanguage::Go, text);
        let tokens = syntax_tokens_for_prepared_document_line(document, 0)
            .expect("Go document should have prepared tokens");
        assert!(
            has_token_kind_and_text(text, &tokens, expected_kind, expected_text),
            "Go comment-prefixed injection should produce {expected_kind:?} token {expected_text:?}: {tokens:?}"
        );
    }
}

#[test]
fn yaml_github_actions_script_injects_javascript() {
    let text = [
        "jobs:",
        "  test:",
        "    steps:",
        "      - uses: actions/github-script@v7",
        "        with:",
        "          script: |",
        "            const value = 42",
    ]
    .join("\n");
    let document = prepare_test_document(DiffSyntaxLanguage::Yaml, &text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 6)
        .expect("YAML github-script line should have prepared tokens");
    assert!(
        tokens.iter().any(|t| {
            t.kind == SyntaxTokenKind::Keyword || t.kind == SyntaxTokenKind::KeywordControl
        }),
        "github-script YAML block should inject JavaScript keywords: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Number),
        "github-script YAML block should inject JavaScript numbers: {tokens:?}"
    );
}

#[test]
fn yaml_github_actions_inline_script_injects_javascript() {
    let text = [
        "jobs:",
        "  test:",
        "    steps:",
        "      - uses: actions/github-script@v7",
        "        with:",
        "          script: const value = 42",
    ]
    .join("\n");
    let inline_line = text.lines().nth(5).unwrap_or_default();
    let document = prepare_test_document(DiffSyntaxLanguage::Yaml, &text);
    let tokens = syntax_tokens_for_prepared_document_line(document, 5)
        .expect("YAML github-script inline line should have prepared tokens");
    assert!(
        has_token_kind_and_text(inline_line, &tokens, SyntaxTokenKind::Keyword, "const")
            || tokens
                .iter()
                .any(|t| t.kind == SyntaxTokenKind::KeywordControl),
        "github-script YAML inline scalars should inject JavaScript keywords: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Number),
        "github-script YAML inline scalars should inject JavaScript numbers: {tokens:?}"
    );
}

#[test]
fn javascript_treesitter_captures_regex_literal() {
    let text = "const re = /foo+/gi;";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::JavaScript, DiffSyntaxMode::Auto);
    assert!(
        tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::StringRegex),
        "JavaScript regex literal should produce StringRegex token, got: {tokens:?}"
    );
}

#[test]
fn javascript_treesitter_captures_constructor_and_constant_builtin() {
    let constructor_tokens = syntax_tokens_for_line(
        "class Example { constructor() {} }",
        DiffSyntaxLanguage::JavaScript,
        DiffSyntaxMode::Auto,
    );
    assert!(
        constructor_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Constructor),
        "JavaScript constructor should produce Constructor token, got: {constructor_tokens:?}"
    );

    let builtin_tokens = syntax_tokens_for_line(
        "const value = undefined;",
        DiffSyntaxLanguage::JavaScript,
        DiffSyntaxMode::Auto,
    );
    assert!(
        builtin_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::ConstantBuiltin),
        "JavaScript builtins should produce ConstantBuiltin token, got: {builtin_tokens:?}"
    );
}

#[test]
fn go_treesitter_captures_namespace_package_identifier() {
    let tokens =
        syntax_tokens_for_line("package main", DiffSyntaxLanguage::Go, DiffSyntaxMode::Auto);
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Namespace),
        "Go package identifier should produce Namespace token, got: {tokens:?}"
    );
}

#[test]
fn lua_and_c_treesitter_capture_preprocessor_and_label() {
    let preproc = syntax_tokens_for_line(
        "#!/usr/bin/env lua",
        DiffSyntaxLanguage::Lua,
        DiffSyntaxMode::Auto,
    );
    assert!(
        preproc.iter().any(|t| t.kind == SyntaxTokenKind::Preproc),
        "Lua hash bang should produce Preproc token, got: {preproc:?}"
    );

    let label = syntax_tokens_for_line(
        "start: return 0;",
        DiffSyntaxLanguage::C,
        DiffSyntaxMode::Auto,
    );
    assert!(
        label.iter().any(|t| t.kind == SyntaxTokenKind::Label),
        "C label should produce Label token, got: {label:?}"
    );
}

#[test]
fn c_treesitter_uses_vendored_zed_query() {
    let preproc = syntax_tokens_for_line(
        "#define VALUE 42",
        DiffSyntaxLanguage::C,
        DiffSyntaxMode::Auto,
    );
    assert!(
        has_token_kind_and_text(
            "#define VALUE 42",
            &preproc,
            SyntaxTokenKind::Preproc,
            "#define"
        ),
        "C preprocessor directives should produce Preproc tokens, got: {preproc:?}"
    );

    let text = "struct Example { int field; };";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::C, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Keyword, "struct"),
        "C storage/type keywords should be captured, got: {tokens:?}"
    );
    assert!(
        has_token_kind_and_text(text, &tokens, SyntaxTokenKind::Property, "field"),
        "C field identifiers should produce Property tokens, got: {tokens:?}"
    );
}

#[test]
fn cpp_treesitter_uses_vendored_zed_query() {
    let concept_text = "template <typename T> concept Addable = requires(T a, T b) { a + b; };";
    let concept_tokens =
        syntax_tokens_for_line(concept_text, DiffSyntaxLanguage::Cpp, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(
            concept_text,
            &concept_tokens,
            SyntaxTokenKind::TypeInterface,
            "Addable"
        ),
        "C++ concepts should produce TypeInterface tokens, got: {concept_tokens:?}"
    );
    assert!(
        has_token_kind_and_text(
            concept_text,
            &concept_tokens,
            SyntaxTokenKind::Keyword,
            "requires"
        ),
        "C++ requires should produce Keyword tokens, got: {concept_tokens:?}"
    );

    let module_text = "export module math.core; import std;";
    let module_tokens =
        syntax_tokens_for_line(module_text, DiffSyntaxLanguage::Cpp, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(
            module_text,
            &module_tokens,
            SyntaxTokenKind::Keyword,
            "module"
        ),
        "C++ module declarations should produce Keyword tokens, got: {module_tokens:?}"
    );
    assert!(
        module_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Namespace),
        "C++ module names should produce Namespace tokens, got: {module_tokens:?}"
    );

    let static_assert_text = "static_assert(sizeof(int) > 0);";
    let static_assert_tokens = syntax_tokens_for_line(
        static_assert_text,
        DiffSyntaxLanguage::Cpp,
        DiffSyntaxMode::Auto,
    );
    assert!(
        has_token_kind_and_text(
            static_assert_text,
            &static_assert_tokens,
            SyntaxTokenKind::Function,
            "static_assert"
        ),
        "C++ static_assert should produce Function tokens, got: {static_assert_tokens:?}"
    );

    let operator_text = "auto cmp = lhs <=> rhs;";
    let operator_tokens =
        syntax_tokens_for_line(operator_text, DiffSyntaxLanguage::Cpp, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(
            operator_text,
            &operator_tokens,
            SyntaxTokenKind::Operator,
            "<=>"
        ),
        "C++ spaceship operators should produce Operator tokens, got: {operator_tokens:?}"
    );

    let preproc_text = "#include <vector>";
    let preproc_tokens =
        syntax_tokens_for_line(preproc_text, DiffSyntaxLanguage::Cpp, DiffSyntaxMode::Auto);
    assert!(
        has_token_kind_and_text(
            preproc_text,
            &preproc_tokens,
            SyntaxTokenKind::Preproc,
            "#include"
        ),
        "C++ preprocessor directives should produce Preproc tokens, got: {preproc_tokens:?}"
    );
}

#[test]
fn gitcommit_treesitter_captures_diff_change_kinds() {
    let text = [
        "Subject",
        "",
        "# Changes to be committed:",
        "# new file: src/new.rs",
        "# deleted: src/old.rs",
        "# modified: src/lib.rs",
    ]
    .join("\n");
    let document = prepare_test_document(DiffSyntaxLanguage::GitCommit, &text);

    let plus = syntax_tokens_for_prepared_document_line(document, 3)
        .expect("gitcommit added line should have prepared tokens");
    assert!(
        plus.iter().any(|t| t.kind == SyntaxTokenKind::DiffPlus),
        "gitcommit additions should produce DiffPlus tokens, got: {plus:?}"
    );

    let minus = syntax_tokens_for_prepared_document_line(document, 4)
        .expect("gitcommit removed line should have prepared tokens");
    assert!(
        minus.iter().any(|t| t.kind == SyntaxTokenKind::DiffMinus),
        "gitcommit removals should produce DiffMinus tokens, got: {minus:?}"
    );

    let delta = syntax_tokens_for_prepared_document_line(document, 5)
        .expect("gitcommit modified file line should have prepared tokens");
    assert!(
        delta.iter().any(|t| t.kind == SyntaxTokenKind::DiffDelta),
        "gitcommit modified files should produce DiffDelta tokens, got: {delta:?}"
    );
}

#[test]
fn markdown_inline_treesitter_captures_text_literal_and_markup_link() {
    let text = "[link](https://example.com) `code`";
    let tokens = syntax_tokens_for_line(
        text,
        DiffSyntaxLanguage::MarkdownInline,
        DiffSyntaxMode::Auto,
    );
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::MarkupLink),
        "Markdown inline link destination should produce MarkupLink token, got: {tokens:?}"
    );
    assert!(
        tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::TextLiteral),
        "Markdown inline code span should produce TextLiteral token, got: {tokens:?}"
    );
}

#[test]
fn markdown_prepared_document_captures_heading_marker_as_punctuation_special() {
    let document = prepare_test_document(DiffSyntaxLanguage::Markdown, "# Heading");
    let tokens = syntax_tokens_for_prepared_document_line(document, 0)
        .expect("markdown heading line should have prepared tokens");
    assert!(
        tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::PunctuationSpecial),
        "Markdown heading marker should remain PunctuationSpecial, got: {tokens:?}"
    );
}

#[test]
fn ruby_and_swift_treesitter_capture_regex_aliases() {
    let cases = [
        (DiffSyntaxLanguage::Ruby, "value = /foo+/"),
        (DiffSyntaxLanguage::Swift, "let pattern = /foo+/"),
    ];

    for (language, text) in cases {
        let tokens = syntax_tokens_for_line(text, language, DiffSyntaxMode::Auto);
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == SyntaxTokenKind::StringRegex),
            "{language:?} regex literal should produce StringRegex token, got: {tokens:?}"
        );
    }
}

#[test]
fn gitcommit_prepared_document_captures_path_symbol_and_trailer_tokens() {
    let text = [
        "Subject",
        "",
        "closes #123",
        "Signed-off-by: me@example.com",
        "# On branch feature/demo",
        "# Changes to be committed:",
        "# renamed: src/old.rs -> src/new.rs",
    ]
    .join("\n");
    let document = prepare_test_document(DiffSyntaxLanguage::GitCommit, &text);

    let trailer = syntax_tokens_for_prepared_document_line(document, 3)
        .expect("gitcommit trailer line should have prepared tokens");
    assert!(
        trailer.iter().any(|t| t.kind == SyntaxTokenKind::Property),
        "gitcommit trailer key should produce Property token, got: {trailer:?}"
    );

    let branch = syntax_tokens_for_prepared_document_line(document, 4)
        .expect("gitcommit branch line should have prepared tokens");
    assert!(
        branch
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::StringSpecial),
        "gitcommit branch line should produce StringSpecial token, got: {branch:?}"
    );

    let renamed = syntax_tokens_for_prepared_document_line(document, 6)
        .expect("gitcommit renamed file line should have prepared tokens");
    assert!(
        renamed.iter().any(|t| t.kind == SyntaxTokenKind::DiffDelta),
        "gitcommit renamed file line should produce DiffDelta token, got: {renamed:?}"
    );
    assert!(
        renamed
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::StringSpecial),
        "gitcommit renamed file path should produce StringSpecial token, got: {renamed:?}"
    );
}

#[test]
fn xml_treesitter_captures_markup_link_via_system_literal() {
    let text = "<!DOCTYPE root SYSTEM \"https://example.com/schema.dtd\">";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::Xml, DiffSyntaxMode::Auto);
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::MarkupLink),
        "XML system literal should produce MarkupLink token, got: {tokens:?}"
    );
}

#[test]
fn xml_heuristic_highlights_comment() {
    let text = "<!-- this is a comment -->";
    let tokens =
        syntax_tokens_for_line(text, DiffSyntaxLanguage::Xml, DiffSyntaxMode::HeuristicOnly);
    assert!(
        tokens.iter().any(|t| t.kind == SyntaxTokenKind::Comment),
        "XML heuristic should highlight <!-- --> comments"
    );
}

#[test]
fn yaml_auto_single_line_highlights_list_item_punctuation_and_strings() {
    let text = "      - \"scripts/windows/verify-signed-artifact.ps1\"";
    let tokens = syntax_tokens_for_line(text, DiffSyntaxLanguage::Yaml, DiffSyntaxMode::Auto);

    assert!(
        tokens
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::Punctuation && token.range == (6..7) }),
        "YAML single-line fallback should highlight the list dash: {tokens:?}"
    );
    assert!(
        tokens
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::String),
        "YAML single-line fallback should highlight quoted scalars: {tokens:?}"
    );
}

#[test]
fn yaml_auto_single_line_highlights_mapping_keys() {
    let top_level = syntax_tokens_for_line(
        "permissions:",
        DiffSyntaxLanguage::Yaml,
        DiffSyntaxMode::Auto,
    );
    assert!(
        top_level
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Property && token.range == (0..11)),
        "YAML single-line fallback should highlight top-level mapping keys: {top_level:?}"
    );

    let nested = syntax_tokens_for_line(
        "      - name: Validate workflow YAML",
        DiffSyntaxLanguage::Yaml,
        DiffSyntaxMode::Auto,
    );
    assert!(
        nested
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::Punctuation && token.range == (6..7) }),
        "YAML single-line fallback should still highlight list punctuation for list-item mappings: {nested:?}"
    );
    assert!(
        nested
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Property && token.range == (8..12)),
        "YAML single-line fallback should highlight mapping keys after a list dash: {nested:?}"
    );
}

#[test]
fn yaml_auto_single_line_highlights_mapping_punctuation_and_plain_scalars() {
    let required = syntax_tokens_for_line(
        "        required: false",
        DiffSyntaxLanguage::Yaml,
        DiffSyntaxMode::Auto,
    );
    assert!(
        required
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::Property && token.range == (8..16) }),
        "YAML fallback should highlight mapping keys: {required:?}"
    );
    assert!(
        required
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::Punctuation && token.range == (16..17) }),
        "YAML fallback should highlight mapping punctuation: {required:?}"
    );
    assert!(
        required
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::Boolean && token.range == (18..23) }),
        "YAML fallback should highlight boolean scalars: {required:?}"
    );

    let string_value = syntax_tokens_for_line(
        "        type: string",
        DiffSyntaxLanguage::Yaml,
        DiffSyntaxMode::Auto,
    );
    assert!(
        string_value
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::Punctuation && token.range == (12..13) }),
        "YAML fallback should highlight mapping punctuation for string values: {string_value:?}"
    );
    assert!(
        string_value
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::String && token.range == (14..20) }),
        "YAML fallback should highlight plain string scalars: {string_value:?}"
    );

    let expression_value = syntax_tokens_for_line(
        "      TAG: ${{ needs.prepare.outputs.tag }}",
        DiffSyntaxLanguage::Yaml,
        DiffSyntaxMode::Auto,
    );
    assert!(
        expression_value
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::Punctuation && token.range == (9..10) }),
        "YAML fallback should highlight mapping punctuation for expressions: {expression_value:?}"
    );
    assert!(
        expression_value
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::String && token.range == (11..43) }),
        "YAML fallback should highlight GitHub expression scalars as strings: {expression_value:?}"
    );
}

#[test]
fn yaml_heuristic_handles_malformed_and_unicode_scalars_without_invalid_ranges() {
    for text in [
        r#"emoji: "😀"#,
        "emoji: 😀 # note",
        "ключ: значение",
        "- 😀",
        "name: ",
        "name:#not-a-comment",
        "  - name: café",
        "script: |+9 trailing",
        "script: >-2",
    ] {
        let tokens = syntax_tokens_for_line_heuristic(text, DiffSyntaxLanguage::Yaml);
        assert_token_ranges_are_utf8_safe(text, &tokens);
    }
}

#[test]
fn yaml_auto_single_line_highlights_block_scalar_indicators_and_sequence_mapping_values() {
    let sequence_mapping = syntax_tokens_for_line(
        "      - name: Build release binary",
        DiffSyntaxLanguage::Yaml,
        DiffSyntaxMode::Auto,
    );
    assert!(
        sequence_mapping
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::Punctuation && token.range == (6..7) }),
        "YAML fallback should highlight list punctuation: {sequence_mapping:?}"
    );
    assert!(
        sequence_mapping
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::Property && token.range == (8..12) }),
        "YAML fallback should highlight sequence mapping keys: {sequence_mapping:?}"
    );
    assert!(
        sequence_mapping
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::Punctuation && token.range == (12..13) }),
        "YAML fallback should highlight sequence mapping punctuation: {sequence_mapping:?}"
    );
    assert!(
        sequence_mapping
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::String && token.range == (14..34) }),
        "YAML fallback should highlight sequence mapping scalar values: {sequence_mapping:?}"
    );

    let block_scalar = syntax_tokens_for_line(
        "        run: |",
        DiffSyntaxLanguage::Yaml,
        DiffSyntaxMode::Auto,
    );
    assert!(
        block_scalar
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::Punctuation && token.range == (11..12) }),
        "YAML fallback should highlight the mapping colon for block scalars: {block_scalar:?}"
    );
    assert!(
        block_scalar
            .iter()
            .any(|token| { token.kind == SyntaxTokenKind::Punctuation && token.range == (13..14) }),
        "YAML fallback should highlight block scalar indicators: {block_scalar:?}"
    );
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

/// The reason queries/ocaml_highlights.scm exists rather than a reference to
/// `tree_sitter_ocaml::HIGHLIGHTS_QUERY`: upstream names `(shebang)`, which the
/// interface grammar has no rule for, and one unknown node type fails the whole
/// query rather than the pattern that names it.
#[test]
fn ocaml_query_serves_both_grammars_and_upstream_does_not() {
    for language in [
        tree_sitter_ocaml::LANGUAGE_OCAML,
        tree_sitter_ocaml::LANGUAGE_OCAML_INTERFACE,
    ] {
        tree_sitter::Query::new(&language.into(), OCAML_HIGHLIGHTS_QUERY)
            .expect("the vendored query should compile against both OCaml grammars");
    }

    assert!(
        tree_sitter::Query::new(
            &tree_sitter_ocaml::LANGUAGE_OCAML_INTERFACE.into(),
            tree_sitter_ocaml::HIGHLIGHTS_QUERY,
        )
        .is_err(),
        "upstream's query now compiles against the interface grammar -- drop the \
         vendored copy and use `tree_sitter_ocaml::HIGHLIGHTS_QUERY` for both."
    );
}

// ---- Groovy ---------------------------------------------------------------

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

/// The quoting literals span the whole quoted form, so capturing the *node*
/// paints `'(alpha beta)` end to end. Upstream captures the one-character
/// marker instead. Capturing the node is cheap to reintroduce and invisible
/// without a test.
#[test]
fn clojure_quoted_form_paints_only_its_marker() {
    let line = "(def syms '(alpha beta))";
    let doc = prepare_test_document(DiffSyntaxLanguage::Clojure, line);

    let marker = token_kinds_for_line_fragment(doc, 0, line, "'");
    assert!(
        marker.contains(&SyntaxTokenKind::Operator),
        "the quote marker should be an operator: {marker:?}"
    );

    let quoted = token_kinds_for_line_fragment(doc, 0, line, "alpha");
    assert!(
        !quoted.contains(&SyntaxTokenKind::Operator)
            && !quoted.contains(&SyntaxTokenKind::PunctuationSpecial),
        "the quoted form itself should not take the marker's colour: {quoted:?}"
    );
}

/// queries/clojure_highlights.scm opens with a verbatim copy of the upstream
/// query and adds to it. A grammar bump that changes upstream leaves the copy
/// stale and silently diverging, which is the one failure mode a compile check
/// cannot see.

/// queries/clojure_highlights.scm opens with a verbatim copy of the upstream
/// query and adds to it. A grammar bump that changes upstream leaves the copy
/// stale and silently diverging, which is the one failure mode a compile check
/// cannot see.
#[test]
fn clojure_highlights_query_embeds_the_upstream_base_verbatim() {
    let upstream = query_rule_lines(tree_sitter_clojure_orchard::HIGHLIGHTS_QUERY);
    let vendored = query_rule_lines(CLOJURE_HIGHLIGHTS_QUERY);
    assert!(
        !upstream.is_empty(),
        "the upstream Clojure query should not be comment-only"
    );
    assert!(
        vendored
            .windows(upstream.len())
            .any(|window| window == upstream.as_slice()),
        "clojure_highlights.scm must contain tree_sitter_clojure_orchard::HIGHLIGHTS_QUERY \
         as a contiguous, in-order block. Mirror the upstream change into the \
         `--- upstream ---` section.\nexpected block:\n{upstream:#?}\nvendored:\n{vendored:#?}"
    );
}

// ---- Solidity -------------------------------------------------------------

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

/// If a grammar bump ships a query that compiles as-is, the vendored copy and
/// this test can both go.
#[test]
fn solidity_upstream_query_still_needs_the_vendored_fix() {
    assert!(
        tree_sitter::Query::new(
            &tree_sitter_solidity::LANGUAGE.into(),
            tree_sitter_solidity::HIGHLIGHT_QUERY,
        )
        .is_err(),
        "tree_sitter_solidity::HIGHLIGHT_QUERY now compiles -- drop \
         queries/solidity_highlights.scm and use the crate constant."
    );
}

// ---- Assembly -------------------------------------------------------------

const ASSEMBLY_FIXTURE: &[&str] = &[
    /*  0 */ "section .text",
    /*  1 */ "global run",
    /*  2 */ "run:",
    /*  3 */ "    mov eax, 1 ; seed",
    /*  4 */ "    add eax, edi",
    /*  5 */ "    ret",
];

/// A documented limitation, not a bug in the wiring: tree-sitter-asm only
/// admits a comment after an instruction, so a comment on its own line -- which
/// is most comments in real assembly -- puts the tree into error recovery.
///
/// Recovery is survivable (the instructions around it still highlight) and the
/// heuristic path, which this repo also runs for short lines and oversized
/// diffs, gets it right. The test pins both halves so a grammar bump that fixes
/// the parse shows up here rather than going unnoticed.
#[test]
fn assembly_standalone_comment_lines_fall_back_to_the_heuristic() {
    let source = "; set the return value\n    mov eax, 1\n    ret\n";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_asm::LANGUAGE.into())
        .expect("asm grammar should load");
    let tree = parser.parse(source, None).expect("asm should parse");
    assert!(
        tree.root_node().has_error(),
        "tree-sitter-asm now parses a standalone comment line -- update this test and \
         the note on the Assembly arm of heuristic_comment_config"
    );

    // The instructions after the bad line still highlight.
    let doc = prepare_test_document(DiffSyntaxLanguage::Assembly, source);
    let kinds = token_kinds_for_line_fragment(doc, 1, "    mov eax, 1", "mov");
    assert!(
        kinds.contains(&SyntaxTokenKind::Function),
        "error recovery should leave the following instructions intact: {kinds:?}"
    );

    // And the heuristic, which does not care what the grammar thinks, greys the
    // whole comment line.
    let heuristic = heuristic_tokens("; set the return value", DiffSyntaxLanguage::Assembly);
    assert!(
        heuristic
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Comment),
        "`;` is the heuristic's assembly line comment: {heuristic:?}"
    );
}

/// The identifier scanner starts on `_` or a letter, never `.`, so a GAS
/// directive reaches `is_keyword` as its bare tail, so a table spelling its
/// entries `".text"` and `".globl"` looks complete and matches nothing.
/// Nothing else in the suite would notice.

/// The identifier scanner starts on `_` or a letter, never `.`, so a GAS
/// directive reaches `is_keyword` as its bare tail, so a table spelling its
/// entries `".text"` and `".globl"` looks complete and matches nothing.
/// Nothing else in the suite would notice.
#[test]
fn assembly_gas_dot_directives_reach_the_keyword_table() {
    for (line, expected) in [
        ("    .text", "text"),
        ("    .data", "data"),
        ("    .bss", "bss"),
        ("    .globl main", "globl"),
        ("    .global main", "global"),
        ("    .align 4", "align"),
        ("    .long 1", "long"),
        ("    .quad 2", "quad"),
        ("    .short 3", "short"),
        ("    .byte 1", "byte"),
        ("    .word 1", "word"),
        ("    .ascii \"hi\"", "ascii"),
        ("    .section .rodata", "section"),
        // The NASM/MASM spellings, which carry no dot to begin with.
        ("section .text", "section"),
        ("global main", "global"),
        ("extern printf", "extern"),
    ] {
        let found = heuristic_keywords(line, DiffSyntaxLanguage::Assembly);
        assert!(
            found.contains(&expected),
            "{line:?} should yield the `{expected}` directive keyword: {found:?}"
        );
    }
}

/// `#` is an ARM immediate (`mov r0, #1`), not a comment. Giving the Assembly
/// arm `hash_comment: true` would grey out the operand of every such
/// instruction, which is why it shares nothing with the Python/Ruby arm.

/// `#` is an ARM immediate (`mov r0, #1`), not a comment. Giving the Assembly
/// arm `hash_comment: true` would grey out the operand of every such
/// instruction, which is why it shares nothing with the Python/Ruby arm.
#[test]
fn assembly_hash_immediate_is_not_a_comment() {
    let line = "    mov r0, #1";
    let tokens = heuristic_tokens(line, DiffSyntaxLanguage::Assembly);
    assert!(
        !tokens
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Comment),
        "an ARM immediate was greyed out as a comment: {tokens:?}"
    );
}

// ---- Svelte ---------------------------------------------------------------

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

/// The script and style bodies are the bulk of a `.svelte` file and neither is
/// reachable from the highlights query -- they arrive as injections or not at
/// all. The `lang="ts"` veto is what keeps the default javascript rule from
/// firing over the same `raw_text`; see the note in svelte_injections.scm.
#[test]
fn svelte_script_and_style_blocks_inject_their_languages() {
    let doc = prepare_test_document(DiffSyntaxLanguage::Svelte, &SVELTE_FIXTURE.join("\n"));

    let script = token_kinds_for_line_fragment(doc, 1, SVELTE_FIXTURE[1], "let");
    assert!(
        script.contains(&SyntaxTokenKind::Keyword),
        "`<script lang=\"ts\">` should inject TypeScript: {script:?}"
    );

    let style = token_kinds_for_line_fragment(doc, 11, SVELTE_FIXTURE[11], "color");
    assert!(
        style.contains(&SyntaxTokenKind::Property),
        "`<style>` should inject CSS: {style:?}"
    );
}

/// The `lang` veto in svelte_injections.scm, which is the whole reason the two
/// default rules carry a `#not-match?`. Without it a `<script lang="ts">` body
/// matches the default javascript rule *and* the typescript one over the same
/// `raw_text`; live.rs keeps both layers and interleaves their captures at the
/// same depth, so the block comes out coloured by whichever wrote last.

/// The `lang` veto in svelte_injections.scm, which is the whole reason the two
/// default rules carry a `#not-match?`. Without it a `<script lang="ts">` body
/// matches the default javascript rule *and* the typescript one over the same
/// `raw_text`; live.rs keeps both layers and interleaves their captures at the
/// same depth, so the block comes out coloured by whichever wrote last.
#[test]
fn svelte_script_with_lang_injects_exactly_one_language() {
    let text = "<script lang=\"ts\">\nconst x: number = 1;\n</script>\n";

    let lang: tree_sitter::Language = tree_sitter_svelte_ng::LANGUAGE.into();
    let query = tree_sitter::Query::new(&lang, SVELTE_INJECTIONS_QUERY)
        .expect("vendored Svelte injections.scm should compile");
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang)
        .expect("Svelte grammar should load");
    let tree = parser.parse(text, None).expect("script should parse");

    let mut cursor = tree_sitter::QueryCursor::new();
    cursor.set_match_limit(TS_QUERY_MATCH_LIMIT);
    let mut patterns = Vec::new();
    {
        let mut matches = cursor.matches(&query, tree.root_node(), text.as_bytes());
        tree_sitter::StreamingIterator::advance(&mut matches);
        while let Some(m) = matches.get() {
            patterns.push(m.pattern_index);
            tree_sitter::StreamingIterator::advance(&mut matches);
        }
    }
    assert_eq!(
        patterns.len(),
        1,
        "a `<script lang=\"ts\">` must match exactly one injection pattern, \
         matched {patterns:?}"
    );

    // ...and it must be the TypeScript one, so the annotation is typed.
    let doc = prepare_test_document(DiffSyntaxLanguage::Svelte, text);
    let tokens = syntax_tokens_for_prepared_document_line(doc, 1)
        .expect("script body should have prepared tokens");
    assert!(
        tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Type || t.kind == SyntaxTokenKind::TypeBuiltin),
        "`lang=\"ts\"` should inject TypeScript, so `: number` should be typed: {tokens:?}"
    );
}

/// The trap vue_injections.scm documents: the default rule is vetoed by
/// `#not-match? "\\slang\\s*="` whenever *any* `lang` is present, so
/// enumerating the servable values with `#any-of?` means every unlisted one --
/// `lang="js"`, `lang="css"`, and the unquoted `lang=ts` -- falls into a gap
/// and the whole block renders with no highlighting at all.
///
/// Forwarding the value as `@injection.language` is what closes it. Asserting
/// `is_some()` would be vacuous here: the broken version returned
/// `Some(vec![])`, not `None`.

/// The trap vue_injections.scm documents: the default rule is vetoed by
/// `#not-match? "\\slang\\s*="` whenever *any* `lang` is present, so
/// enumerating the servable values with `#any-of?` means every unlisted one --
/// `lang="js"`, `lang="css"`, and the unquoted `lang=ts` -- falls into a gap
/// and the whole block renders with no highlighting at all.
///
/// Forwarding the value as `@injection.language` is what closes it. Asserting
/// `is_some()` would be vacuous here: the broken version returned
/// `Some(vec![])`, not `None`.
#[test]
fn svelte_lang_values_outside_the_default_still_inject() {
    for (open_tag, body, expected) in [
        // `js` and `css` name grammars we have but are not the default value
        // for their element -- the exact pair the enumerated version dropped.
        (
            "<script lang=\"js\">",
            "const x = 1;",
            SyntaxTokenKind::Keyword,
        ),
        (
            "<style lang=\"css\">",
            "  .b { color: red; }",
            SyntaxTokenKind::Property,
        ),
        // Resolved through the alias table rather than by name.
        (
            "<script lang=\"mts\">",
            "const x = 1;",
            SyntaxTokenKind::Keyword,
        ),
        (
            "<style lang=\"pcss\">",
            "  .b { color: red; }",
            SyntaxTokenKind::Property,
        ),
        // The unquoted form the grammar permits.
        ("<script lang=ts>", "const x = 1;", SyntaxTokenKind::Keyword),
    ] {
        let close = if open_tag.starts_with("<script") {
            "</script>"
        } else {
            "</style>"
        };
        let text = format!("{open_tag}\n{body}\n{close}\n");
        let doc = prepare_test_document(DiffSyntaxLanguage::Svelte, &text);
        let tokens = syntax_tokens_for_prepared_document_line(doc, 1)
            .expect("the block body should have prepared tokens");
        assert!(
            tokens.iter().any(|token| token.kind == expected),
            "`{open_tag}` should still inject: expected {expected:?}, got {tokens:?}"
        );
    }
}

/// The other half of the same trade-off: a `lang` no grammar here can serve
/// injects nothing, and that must not disturb the host grammar. Same contract
/// as `vue_unknown_lang_attribute_does_not_silently_disable_highlighting`.

/// The other half of the same trade-off: a `lang` no grammar here can serve
/// injects nothing, and that must not disturb the host grammar. Same contract
/// as `vue_unknown_lang_attribute_does_not_silently_disable_highlighting`.
#[test]
fn svelte_unservable_lang_leaves_the_markup_alone() {
    let text = "<script lang=\"coffee\">\nx = 1\n</script>\n";
    let doc = prepare_test_document(DiffSyntaxLanguage::Svelte, text);
    let tokens = syntax_tokens_for_prepared_document_line(doc, 0)
        .expect("the opening tag should have prepared tokens");
    assert!(
        tokens
            .iter()
            .any(|token| token.kind == SyntaxTokenKind::Tag),
        "an unservable lang must not disturb the host grammar: {tokens:?}"
    );
}

/// The bug in `tree_sitter_svelte_ng::INJECTIONS_QUERY` that svelte_injections.scm
/// exists to avoid: its bare `(raw_text)` catch-all matches the body of `<style>`
/// too, so a stylesheet gets parsed as JavaScript.

/// The bug in `tree_sitter_svelte_ng::INJECTIONS_QUERY` that svelte_injections.scm
/// exists to avoid: its bare `(raw_text)` catch-all matches the body of `<style>`
/// too, so a stylesheet gets parsed as JavaScript.
#[test]
fn svelte_style_block_injects_css_not_javascript() {
    let text = "<style>\n  .btn { color: red; }\n</style>\n";
    let doc = prepare_test_document(DiffSyntaxLanguage::Svelte, text);
    let kinds = token_kinds_for_line_fragment(doc, 1, "  .btn { color: red; }", "color");
    assert!(
        kinds.contains(&SyntaxTokenKind::Property),
        "`color` is a CSS property, which the JavaScript grammar would never \
         produce: {kinds:?}"
    );
}

/// The counterpart to `vue_static_inline_styles_do_not_flood_the_injection_cache`.
/// A `.svelte` template injects per *expression*, not per file, so an ordinary
/// list render emits one layer per row. Without the bare-identifier guard in
/// svelte_injections.scm a 30-row list produced 30 cache entries against a cap
/// of 32, evicting everything else on its own.

/// The counterpart to `vue_static_inline_styles_do_not_flood_the_injection_cache`.
/// A `.svelte` template injects per *expression*, not per file, so an ordinary
/// list render emits one layer per row. Without the bare-identifier guard in
/// svelte_injections.scm a 30-row list produced 30 cache entries against a cap
/// of 32, evicting everything else on its own.
#[test]
fn svelte_bare_identifier_expressions_do_not_flood_the_injection_cache() {
    TS_INJECTION_CACHE.with(|cache| cache.borrow_mut().clear());

    let mut lines = vec!["<ul>".to_string()];
    for ix in 0..30 {
        lines.push(format!("  <li>{{row{ix}}}</li>"));
    }
    lines.push("</ul>".to_string());
    let line_count = lines.len();

    let doc = prepare_test_document(DiffSyntaxLanguage::Svelte, &lines.join("\n"));
    for line_ix in 0..line_count {
        let _ = syntax_tokens_for_prepared_document_line(doc, line_ix);
    }

    let cached = TS_INJECTION_CACHE.with(|cache| cache.borrow().len());
    assert_eq!(
        cached, 0,
        "a bare identifier gains nothing from a JavaScript parse, but {cached} cache \
         entries were created from {line_count} lines (cap is \
         {TS_INJECTION_CACHE_MAX_ENTRIES})"
    );

    TS_INJECTION_CACHE.with(|cache| cache.borrow_mut().clear());
}

/// ...and the guard must not be so broad that real expressions stop injecting.

/// ...and the guard must not be so broad that real expressions stop injecting.
#[test]
fn svelte_non_trivial_expressions_still_inject() {
    let line = "  <p>{count > 0 ? \"many\" : \"none\"}</p>";
    let doc = prepare_test_document(DiffSyntaxLanguage::Svelte, line);

    let kinds = token_kinds_for_line_fragment(doc, 0, line, "\"many\"");
    assert!(
        kinds.contains(&SyntaxTokenKind::String),
        "a real expression should still be parsed as JavaScript, so the string \
         literal inside it is a string: {kinds:?}"
    );
}

/// The same tripwire vue_highlights.scm carries, for the same reason: the
/// Svelte grammar is html-shaped, the base rules have to be present in the
/// file, and rule order decides which capture wins.

/// The same tripwire vue_highlights.scm carries, for the same reason: the
/// Svelte grammar is html-shaped, the base rules have to be present in the
/// file, and rule order decides which capture wins.
#[test]
fn svelte_highlights_query_embeds_the_html_base_verbatim() {
    let html_rules = query_rule_lines(HTML_HIGHLIGHTS_QUERY);
    let svelte_rules = query_rule_lines(SVELTE_HIGHLIGHTS_QUERY);
    assert!(
        !html_rules.is_empty(),
        "html_highlights.scm should not be comment-only"
    );
    assert!(
        svelte_rules
            .windows(html_rules.len())
            .any(|window| window == html_rules.as_slice()),
        "svelte_highlights.scm must contain queries/html_highlights.scm as a contiguous, \
         in-order block. Mirror the change into the `--- html base ---` section.\n\
         expected block:\n{html_rules:#?}\nsvelte rules:\n{svelte_rules:#?}"
    );
}

// ---- Heuristic fallback for the batch --------------------------------------

/// Three of the new languages spell something other than a string with `'`:
/// Haskell primes identifiers, OCaml opens type variables, Clojure quotes
/// forms. Left as `HeuristicSingleQuote::String` each one runs a string from
/// the tick to the end of the line -- the Nix bug, three more times.

/// Julia is the one language in the batch where `'` is both: `A'` is the
/// adjoint operator and `'c'` is a character literal. ValuePositionOnly tells
/// them apart by what precedes the tick.
#[test]
fn julia_adjoint_is_not_a_string_but_a_char_literal_is() {
    assert!(
        heuristic_string_spans("    b = A' * x", DiffSyntaxLanguage::Julia).is_empty(),
        "the adjoint operator opened a string: {:?}",
        heuristic_tokens("    b = A' * x", DiffSyntaxLanguage::Julia)
    );
    assert_eq!(
        heuristic_string_spans("    c = 'x'", DiffSyntaxLanguage::Julia),
        vec!["'x'"],
        "a character literal in value position is still a string"
    );
}

/// Every comment form the batch introduced. The heuristic runs in production
/// for lines past MAX_TREESITTER_LINE_BYTES and in HeuristicOnly mode, and
/// these arms are reached by nothing else.

/// Per the Haskell report a run of dashes is a comment only when it is *not*
/// followed by a symbol character; otherwise the whole run is an operator.
/// `line_comment: Some("--")` cannot express that, so it greyed `a --> b` from
/// the dashes to the end of the line -- the worst failure mode this path has,
/// because it hides code rather than mis-colouring it.
#[test]
fn haskell_operator_sections_starting_with_dashes_are_not_comments() {
    for line in [
        "  step = a --> b",
        "  merged = xs --| ys",
        "  shifted = a --< b",
        "  chained = f --. g",
    ] {
        let tokens = heuristic_tokens(line, DiffSyntaxLanguage::Haskell);
        assert!(
            !tokens
                .iter()
                .any(|token| token.kind == SyntaxTokenKind::Comment),
            "an operator section was greyed out as a comment in {line:?}: {tokens:?}"
        );
    }

    // ...while every genuine comment still is one, including the `---` run that
    // the naive "any symbol after `--`" rule would have broken.
    for line in [
        "-- plain",
        "--- ruled off",
        "  x = 1 -- trailing",
        "-- | haddock",
    ] {
        let tokens = heuristic_tokens(line, DiffSyntaxLanguage::Haskell);
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == SyntaxTokenKind::Comment),
            "{line:?} is a Haskell comment: {tokens:?}"
        );
    }
}

/// Neighbouring entries in four of the keyword tables, each gap visible as two
/// adjacent lines highlighting differently.

/// The other half of the Solidity fix: sized types are *uniformly* absent now.
/// Listing `uint256` alone meant `uint256 total;` highlighted and `uint8 flags;`
/// two lines below it did not.
#[test]
fn solidity_sized_types_are_uniformly_absent_from_the_keyword_table() {
    for line in [
        "    uint8 a;",
        "    uint256 b;",
        "    int128 c;",
        "    bytes32 d;",
    ] {
        let found = heuristic_keywords(line, DiffSyntaxLanguage::Solidity);
        assert!(
            found.is_empty(),
            "sized types should all behave alike on the heuristic path, but {line:?} \
             yielded {found:?}"
        );
    }

    // The base names still resolve, so the arm is not simply dead.
    assert!(heuristic_keywords("    uint x;", DiffSyntaxLanguage::Solidity).contains(&"uint"));
    assert!(
        heuristic_keywords("    address owner;", DiffSyntaxLanguage::Solidity).contains(&"address")
    );
}

/// The eleven keyword tables the batch added to `is_keyword`, none of which any
/// other test reaches: every other test in this section goes through
/// `prepare_test_document`, i.e. tree-sitter.

#[test]
fn nix_extension_is_supported() {
    for path in ["flake.nix", "pkgs/demo/default.nix", "nix/modules/web.nix"] {
        assert_eq!(
            diff_syntax_language_for_path(path),
            Some(DiffSyntaxLanguage::Nix),
            "{path} should resolve to the Nix grammar"
        );
    }
    assert_eq!(
        diff_syntax_language_for_code_fence_info("nix"),
        Some(DiffSyntaxLanguage::Nix),
    );
    // `flake.lock` is JSON and must keep resolving that way.
    assert_eq!(
        diff_syntax_language_for_path("flake.lock"),
        Some(DiffSyntaxLanguage::Json),
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
fn nix_specific_captures_survive_the_generic_identifier_rule() {
    let doc = prepare_nix_document(NIX_FIXTURE);

    let builtins = token_kinds_for_line_fragment(doc, 5, NIX_FIXTURE[5], "builtins");
    assert!(
        builtins.contains(&SyntaxTokenKind::VariableBuiltin)
            && !builtins.contains(&SyntaxTokenKind::Variable),
        "`builtins` must keep its builtin colour rather than falling back to the \
         blanket `(identifier) @variable` rule: {builtins:?}"
    );

    let applied = token_kinds_for_line_fragment(doc, 7, NIX_FIXTURE[7], "mkDerivation");
    assert!(
        applied.contains(&SyntaxTokenKind::Function)
            && !applied.contains(&SyntaxTokenKind::Variable),
        "an identifier in function-application position must read as a function: \
         {applied:?}"
    );

    let inherited = token_kinds_for_line_fragment(doc, 3, NIX_FIXTURE[3], "stdenv");
    assert!(
        inherited.contains(&SyntaxTokenKind::Property),
        "`inherit (pkgs) stdenv` names a property: {inherited:?}"
    );
}

/// An escape inside a string keeps its own colour.
///
/// Not an ordering guard, despite appearances: `normalize_non_overlapping_tokens`
/// hands each slice to the last *containing* capture in emission order, and the
/// cursor emits by node start byte, so a nested `(escape_sequence)` always beats
/// the `(string_expression)` around it whichever order their rules appear in.
/// Verified — this passes against upstream's ordering too. It is here to pin the
/// behaviour, not the query layout.

/// An escape inside a string keeps its own colour.
///
/// Not an ordering guard, despite appearances: `normalize_non_overlapping_tokens`
/// hands each slice to the last *containing* capture in emission order, and the
/// cursor emits by node start byte, so a nested `(escape_sequence)` always beats
/// the `(string_expression)` around it whichever order their rules appear in.
/// Verified — this passes against upstream's ordering too. It is here to pin the
/// behaviour, not the query layout.
#[test]
fn nix_escape_sequences_outrank_the_string_rule() {
    let lines = ["{ s = \"a\\nb\"; }"];
    let doc = prepare_nix_document(&lines);
    let escape = token_kinds_for_line_fragment(doc, 0, lines[0], "\\n");
    assert!(
        escape.contains(&SyntaxTokenKind::StringEscape),
        "`\\n` inside a string must outrank the enclosing `@string` capture: {escape:?}"
    );
}

/// The interior of `"demo v${version}"` is Nix code, not string text.
///
/// Like the escape test above, this holds by node position rather than by rule
/// order — the interpolated expression starts after the string does, so it wins
/// its own bytes regardless.

/// The interior of `"demo v${version}"` is Nix code, not string text.
///
/// Like the escape test above, this holds by node position rather than by rule
/// order — the interpolated expression starts after the string does, so it wins
/// its own bytes regardless.
#[test]
fn nix_interpolation_interior_is_not_flat_string() {
    let doc = prepare_nix_document(NIX_FIXTURE);
    let inner = token_kinds_for_line_fragment(doc, 9, NIX_FIXTURE[9], "version");
    assert!(
        !inner.is_empty() && !inner.contains(&SyntaxTokenKind::String),
        "the expression inside `${{…}}` should be highlighted as code, not as part \
         of the surrounding string: {inner:?}"
    );
}

/// `buildPhase = '' … ''` is shell script, and the combined Bash injection is
/// what makes it read as one. Only the injected layer has a concept of `if`.

/// `buildPhase = '' … ''` is shell script, and the combined Bash injection is
/// what makes it read as one. Only the injected layer has a concept of `if`.
#[test]
fn nix_build_phase_is_highlighted_as_bash() {
    let doc = prepare_nix_document(NIX_FIXTURE);

    let conditional = token_kinds_for_line_fragment(doc, 12, NIX_FIXTURE[12], "if");
    assert!(
        conditional.contains(&SyntaxTokenKind::KeywordControl)
            || conditional.contains(&SyntaxTokenKind::Keyword),
        "`if` inside buildPhase should come from the injected Bash layer, not read \
         as string text: {conditional:?}"
    );

    // And the injection stays inside the indented string: the Nix binding on
    // the line above is still Nix.
    let binding = token_kinds_for_line_fragment(doc, 10, NIX_FIXTURE[10], "buildPhase");
    assert!(
        binding.contains(&SyntaxTokenKind::Property),
        "`buildPhase` is a Nix attrpath, not part of the shell script: {binding:?}"
    );
}

#[test]
fn nix_injection_targets_resolve_to_working_grammars() {
    let lang: tree_sitter::Language = tree_sitter_nix::LANGUAGE.into();
    let query = tree_sitter::Query::new(&lang, NIX_INJECTIONS_QUERY)
        .expect("nix_injections.scm should compile");
    let mut checked = 0usize;
    for pattern_ix in 0..query.pattern_count() {
        for setting in query.property_settings(pattern_ix) {
            if setting.key.as_ref() != "injection.language" {
                continue;
            }
            let Some(value) = setting.value.as_deref() else {
                continue;
            };
            let target = injection_language_from_name(value).unwrap_or_else(|| {
                panic!("nix_injections.scm names an unknown injection language {value:?}")
            });
            assert!(
                tree_sitter_highlight_spec(target).is_some(),
                "nix_injections.scm injects {value:?} but no grammar is wired for \
                 {target:?}, so the injection would silently no-op"
            );
            checked += 1;
        }
    }
    assert_eq!(
        checked, 4,
        "expected the four curated bash rules; upstream's comment-marked \
         arbitrary-language rule is deliberately not ported"
    );
}

#[test]
fn nix_spec_warmup_reaches_bash_through_a_set_directive() {
    let spec = tree_sitter_highlight_spec(DiffSyntaxLanguage::Nix).expect("nix spec");
    let injection_query = spec.injection_query.as_ref().expect("nix injection query");
    let reaches_bash = (0..injection_query.pattern_count()).any(|pattern_ix| {
        injection_query
            .property_settings(pattern_ix)
            .iter()
            .filter(|setting| setting.key.as_ref() == "injection.language")
            .any(|setting| {
                setting
                    .value
                    .as_deref()
                    .and_then(injection_language_from_name)
                    == Some(DiffSyntaxLanguage::Bash)
            })
    });
    assert!(
        reaches_bash,
        "warm_reachable_highlight_specs must be able to see the bash target, or the \
         Bash query compile lands on the draw path"
    );
}

#[test]
fn nix_grammar_is_abi_compatible_with_workspace_tree_sitter() {
    let nix: tree_sitter::Language = tree_sitter_nix::LANGUAGE.into();
    let abi = nix.abi_version();
    assert!(
        (tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION..=tree_sitter::LANGUAGE_VERSION)
            .contains(&abi),
        "tree-sitter-nix ABI {abi} is outside the range this tree-sitter supports \
         ({}..={})",
        tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION,
        tree_sitter::LANGUAGE_VERSION,
    );
}

#[test]
fn nix_grammar_parses_a_flake() {
    let source = concat!(
        "{\n",
        "  description = \"demo\";\n",
        "  inputs.nixpkgs.url = \"github:NixOS/nixpkgs/nixos-unstable\";\n",
        "  outputs = { self, nixpkgs }: {\n",
        "    packages.x86_64-linux.default =\n",
        "      nixpkgs.legacyPackages.x86_64-linux.hello;\n",
        "  };\n",
        "}\n",
    );
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_nix::LANGUAGE.into())
        .expect("nix grammar should load into the workspace tree-sitter");
    let tree = parser.parse(source, None).expect("flake.nix should parse");
    assert!(
        !tree.root_node().has_error(),
        "the nix grammar produced an ERROR node for a well-formed flake: {}",
        tree.root_node().to_sexp(),
    );
}

// ---- Nunjucks / Jinja2 ----------------------------------------------------

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

#[test]
fn jinja_extension_is_supported() {
    for path in [
        "templates/index.njk",
        "templates/base.html.j2",
        "templates/macros.jinja",
        "templates/macros.jinja2",
        "templates/page.twig",
        "templates/page.html.dj",
    ] {
        assert_eq!(
            diff_syntax_language_for_path(path),
            Some(DiffSyntaxLanguage::Jinja),
            "{path} should resolve to the Jinja grammar"
        );
    }
    // The same table backs markdown fence info.
    for fence in ["njk", "jinja", "jinja2", "twig", "nunjucks"] {
        assert_eq!(
            diff_syntax_language_for_code_fence_info(fence),
            Some(DiffSyntaxLanguage::Jinja),
            "```{fence} should resolve to the Jinja grammar"
        );
    }
}

/// A `.j2` says the file is templated, not that it is markup. Resolving a shell
/// or config template to the HTML-injecting reading hands the HTML grammar
/// `cat <<EOF` and `2>&1`, which open bogus elements.

/// The shell-template shape that motivated the split, end to end.
#[test]
fn shell_bodied_jinja_template_does_not_colour_redirects_as_tags() {
    let lines = [
        /* 0 */ "#!/bin/sh",
        /* 1 */ "{% if debug %}",
        /* 2 */ "cat <<EOF > {{ target }}",
        /* 3 */ "  value=1",
        /* 4 */ "EOF",
        /* 5 */ "{% endif %}",
        /* 6 */ "run --flag 2>&1 < input",
    ];
    let doc = prepare_test_document(DiffSyntaxLanguage::JinjaText, &lines.join("\n"));

    for (line_ix, fragment) in [(2usize, "EOF"), (6, "input")] {
        let kinds = token_kinds_for_line_fragment(doc, line_ix, lines[line_ix], fragment);
        assert!(
            !kinds.contains(&SyntaxTokenKind::Tag),
            "`{fragment}` on line {line_ix} was coloured as an HTML tag: {kinds:?}"
        );
    }

    // The template tags themselves still highlight -- only the injection is gone.
    let endif = token_kinds_for_line_fragment(doc, 5, lines[5], "endif");
    assert!(
        endif.contains(&SyntaxTokenKind::KeywordControl),
        "template keywords must survive the split: {endif:?}"
    );
}

/// The injected HTML must stay off the template tags, which the Jinja
/// grammar owns. See `combined_injection_gaps`.
#[test]
fn jinja_html_injection_does_not_bleed_onto_template_tags() {
    let doc = prepare_jinja_document(JINJA_TEMPLATE_FIXTURE);
    let kinds = token_kinds_for_line_fragment(doc, 4, JINJA_TEMPLATE_FIXTURE[4], "endfor");
    assert!(
        !kinds.contains(&SyntaxTokenKind::Tag),
        "`{{% endfor %}}` sits in a gap between two HTML ranges; an HTML element \
         node spanning it must not colour it as a tag: {kinds:?}"
    );
}

#[test]
fn jinja_injection_targets_resolve_to_working_grammars() {
    let lang: tree_sitter::Language = tree_sitter_jinja_dialects::LANGUAGE.into();
    let query = tree_sitter::Query::new(&lang, JINJA_INJECTIONS_QUERY)
        .expect("jinja_injections.scm should compile");
    let mut checked = 0usize;
    for pattern_ix in 0..query.pattern_count() {
        for setting in query.property_settings(pattern_ix) {
            if setting.key.as_ref() != "injection.language" {
                continue;
            }
            let Some(value) = setting.value.as_deref() else {
                continue;
            };
            let target = injection_language_from_name(value).unwrap_or_else(|| {
                panic!("jinja_injections.scm names an unknown injection language {value:?}")
            });
            assert!(
                tree_sitter_highlight_spec(target).is_some(),
                "jinja_injections.scm injects {value:?} but no grammar is wired for \
                 {target:?}, so the injection would silently no-op"
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "expected at least one `#set! injection.language`"
    );
}

/// Warm-up reads targets off the compiled query, and only sees `#set!`
/// literals. If the HTML target ever moved into an `@injection.language`
/// capture, the ~0.5ms HTML spec compile would move back onto the draw path.

/// Warm-up reads targets off the compiled query, and only sees `#set!`
/// literals. If the HTML target ever moved into an `@injection.language`
/// capture, the ~0.5ms HTML spec compile would move back onto the draw path.
#[test]
fn jinja_spec_warmup_reaches_html_through_a_set_directive() {
    let spec = tree_sitter_highlight_spec(DiffSyntaxLanguage::Jinja).expect("jinja spec");
    let injection_query = spec
        .injection_query
        .as_ref()
        .expect("jinja injection query");
    let reaches_html = (0..injection_query.pattern_count()).any(|pattern_ix| {
        injection_query
            .property_settings(pattern_ix)
            .iter()
            .filter(|setting| setting.key.as_ref() == "injection.language")
            .any(|setting| {
                setting
                    .value
                    .as_deref()
                    .and_then(injection_language_from_name)
                    == Some(DiffSyntaxLanguage::Html)
            })
    });
    assert!(
        reaches_html,
        "warm_reachable_highlight_specs must be able to see the html target"
    );
}

#[test]
fn jinja_injection_query_stays_under_the_match_limit_on_a_dense_template() {
    let mut lines = vec!["<ul>".to_string()];
    for ix in 0..120 {
        lines.push(format!(
            "  {{% if show{ix} %}}<li class=\"r{ix}\">{{{{ row{ix}.label | title }}}}</li>{{% endif %}}"
        ));
    }
    lines.push("</ul>".to_string());
    let text = lines.join("\n");

    let lang: tree_sitter::Language = tree_sitter_jinja_dialects::LANGUAGE.into();
    let query = tree_sitter::Query::new(&lang, JINJA_INJECTIONS_QUERY)
        .expect("jinja_injections.scm should compile");
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang)
        .expect("jinja grammar should load");
    let tree = parser
        .parse(&text, None)
        .expect("dense template should parse");

    let mut cursor = tree_sitter::QueryCursor::new();
    cursor.set_match_limit(TS_QUERY_MATCH_LIMIT);
    let mut matched = 0usize;
    {
        let mut matches = cursor.matches(&query, tree.root_node(), text.as_bytes());
        tree_sitter::StreamingIterator::advance(&mut matches);
        while matches.get().is_some() {
            matched += 1;
            tree_sitter::StreamingIterator::advance(&mut matches);
        }
    }

    assert!(
        !cursor.did_exceed_match_limit(),
        "the Jinja injection query overflowed the {TS_QUERY_MATCH_LIMIT}-match \
         in-progress pool on a {}-line template. A combined group that loses ranges \
         assembles a different HTML document, so the engine drops the whole group and \
         the template renders with no HTML highlighting at all",
        lines.len(),
    );
    assert!(matched > 0, "the dense template should produce matches");
}

/// The grammar is a young crates.io release binding through
/// `tree-sitter-language`, so a tree-sitter bump could outrun it.

/// The grammar is a young crates.io release binding through
/// `tree-sitter-language`, so a tree-sitter bump could outrun it.
#[test]
fn jinja_grammar_is_abi_compatible_with_workspace_tree_sitter() {
    let jinja: tree_sitter::Language = tree_sitter_jinja_dialects::LANGUAGE.into();
    let abi = jinja.abi_version();
    assert!(
        (tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION..=tree_sitter::LANGUAGE_VERSION)
            .contains(&abi),
        "tree-sitter-jinja-dialects ABI {abi} is outside the range this tree-sitter \
         supports ({}..={})",
        tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION,
        tree_sitter::LANGUAGE_VERSION,
    );
}

#[test]
fn jinja_grammar_parses_every_dialect_it_claims() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_jinja_dialects::LANGUAGE.into())
        .expect("jinja grammar should load into the workspace tree-sitter");
    // One sample per dialect the crate advertises, since a single grammar
    // serving all of njk/j2/twig/dj is the reason it was chosen.
    for (dialect, source) in [
        ("jinja2", "{% for x in xs %}{{ x|e }}{% endfor %}\n"),
        ("nunjucks", "{% set n = 1 %}{{ n + 1 }}\n"),
        ("twig", "{% if a is not empty %}{{ a.b }}{% endif %}\n"),
        (
            "django",
            "{% extends \"base.html\" %}{% block body %}{% endblock %}\n",
        ),
    ] {
        let tree = parser
            .parse(source, None)
            .unwrap_or_else(|| panic!("{dialect} sample should parse"));
        assert!(
            !tree.root_node().has_error(),
            "{dialect} sample produced an ERROR node: {}",
            tree.root_node().to_sexp(),
        );
    }
}

// ---- `#set! injection.combined` ------------------------------------------

/// The inventory tripwire.
///
/// Combined injections change how a grammar's whole document is assembled, so
/// a grammar bump that quietly introduces the directive must not slip through
/// review. F#'s `xml_doc` rule is the only one in the tree today; it arrived
/// with the upstream `tree_sitter_fsharp::INJECTIONS_QUERY` rather than being
/// written here.

/// F# XML doc comments are the one in-tree consumer of `injection.combined`.
///
/// `xml_doc` is a per-line token, so before combined support each `///` line
/// was its own XML document: `<summary>` on one line and `</summary>` on
/// another never met, and each cost an entry in the 32-slot injection cache.
#[test]
fn fsharp_xml_doc_comment_is_highlighted_as_one_xml_document() {
    let lines = [
        /* 0 */ "/// <summary>",
        /* 1 */ "/// Adds two numbers.",
        /* 2 */ "/// </summary>",
        /* 3 */ "let add x y = x + y",
    ];
    let doc = prepare_test_document(DiffSyntaxLanguage::FSharp, &lines.join("\n"));

    let closing = token_kinds_for_line_fragment(doc, 2, lines[2], "summary");
    assert!(
        closing.contains(&SyntaxTokenKind::Tag),
        "`</summary>` closes a tag opened two lines earlier, which only parses \
         when the three xml_doc lines are one document: {closing:?}"
    );

    // And the layer stays inside its own ranges: the following line is F#.
    let keyword = token_kinds_for_line_fragment(doc, 3, lines[3], "let");
    assert!(
        keyword.contains(&SyntaxTokenKind::Keyword),
        "the combined XML layer leaked past the doc comment onto `let`: {keyword:?}"
    );
}

/// Combined layers must not touch the per-node injection cache at all.
///
/// The 32-slot LRU is keyed by a single node's content hash, which a combined
/// layer does not have -- its identity is a *set* of ranges. Feeding it one
/// entry per constituent node is what F# used to do: 200 `///` lines meant 200
/// entries into a 32-slot cache, evicting everything
/// `vue_static_inline_styles_do_not_flood_the_injection_cache` depends on.
///
/// This is not a claim that combined parses are memoised elsewhere. They are
/// not: each of the N/64 chunks pays its own on first build, and clipping is
/// what keeps that cost proportional to the window.

/// A `(text)`-style combined rule fires once per node, so this is the query
/// most likely to overflow the in-progress match pool. Overflow is worse for a
/// combined layer than a single one: tree-sitter discards matches silently, and
/// a missing range changes the document the injected grammar assembles.
#[test]
fn fsharp_xml_doc_injection_stays_under_the_match_limit_on_a_long_doc_comment() {
    let mut lines = vec!["/// <summary>".to_string()];
    for ix in 0..200 {
        lines.push(format!("/// line {ix}"));
    }
    lines.push("/// </summary>".to_string());
    let text = lines.join("\n");

    let lang: tree_sitter::Language = tree_sitter_fsharp::LANGUAGE_FSHARP.into();
    let query = tree_sitter::Query::new(&lang, tree_sitter_fsharp::INJECTIONS_QUERY)
        .expect("fsharp injections.scm should compile");
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&lang).expect("fsharp grammar");
    let tree = parser.parse(&text, None).expect("doc comment should parse");

    let mut cursor = tree_sitter::QueryCursor::new();
    cursor.set_match_limit(TS_QUERY_MATCH_LIMIT);
    let mut matched = 0usize;
    {
        let mut matches = cursor.matches(&query, tree.root_node(), text.as_bytes());
        tree_sitter::StreamingIterator::advance(&mut matches);
        while matches.get().is_some() {
            matched += 1;
            tree_sitter::StreamingIterator::advance(&mut matches);
        }
    }

    assert!(
        !cursor.did_exceed_match_limit(),
        "the F# injection query overflowed the {TS_QUERY_MATCH_LIMIT}-match in-progress \
         pool on a {}-line doc comment; a combined group that loses ranges assembles a \
         different document, so the whole group is dropped when this happens",
        lines.len(),
    );
    assert!(matched > 0, "the doc comment should produce matches at all");
}
