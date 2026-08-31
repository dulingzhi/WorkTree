use super::*;

/// Byte ranges of the HTML runs in the combined-injection fixture, i.e. what a
/// template grammar's `(text)` nodes would capture.
fn combined_test_ranges(text: &str) -> Vec<Range<usize>> {
    let for_tag = text.find("{% for x in xs %}").expect("for tag");
    let li = text.find("<li>").expect("li");
    let endfor = text.find("{% endfor %}").expect("endfor");
    let after_endfor = endfor + "{% endfor %}".len();
    merge_sorted_injection_ranges(vec![0..for_tag, li..endfor, after_endfor..text.len()])
}

// ---- Combined-injection scoping -------------------------------------------

#[test]
fn fenced_code_info_aliases_are_supported() {
    assert_eq!(
        diff_syntax_language_for_code_fence_info("rust"),
        Some(DiffSyntaxLanguage::Rust)
    );
    assert_eq!(
        diff_syntax_language_for_code_fence_info("language-typescript title=\"main.ts\""),
        Some(DiffSyntaxLanguage::TypeScript)
    );
    assert_eq!(
        diff_syntax_language_for_code_fence_info("{.shell}"),
        Some(DiffSyntaxLanguage::Bash)
    );
    assert_eq!(
        diff_syntax_language_for_code_fence_info("jsonc"),
        Some(DiffSyntaxLanguage::Json)
    );
    assert_eq!(
        diff_syntax_language_for_code_fence_info("shellscript"),
        Some(DiffSyntaxLanguage::Bash)
    );
    assert_eq!(
        diff_syntax_language_for_code_fence_info("pwsh"),
        Some(DiffSyntaxLanguage::PowerShell)
    );
    assert_eq!(
        diff_syntax_language_for_code_fence_info("ps1"),
        Some(DiffSyntaxLanguage::PowerShell)
    );
    assert_eq!(
        diff_syntax_language_for_code_fence_info("objective-c"),
        Some(DiffSyntaxLanguage::ObjectiveC)
    );
    assert_eq!(
        diff_syntax_language_for_code_fence_info("go.mod"),
        Some(DiffSyntaxLanguage::GoMod)
    );
    assert_eq!(
        diff_syntax_language_for_code_fence_info("go.work"),
        Some(DiffSyntaxLanguage::GoWork)
    );
    assert_eq!(
        diff_syntax_language_for_code_fence_info("diff"),
        Some(DiffSyntaxLanguage::Diff)
    );
    assert_eq!(
        diff_syntax_language_for_code_fence_info("regex"),
        Some(DiffSyntaxLanguage::Regex)
    );
    assert_eq!(
        diff_syntax_language_for_code_fence_info("jsdoc"),
        Some(DiffSyntaxLanguage::Jsdoc)
    );
    assert_eq!(
        diff_syntax_language_for_code_fence_info("foo/bar/baz.rb"),
        Some(DiffSyntaxLanguage::Ruby)
    );
    assert_eq!(
        diff_syntax_language_for_code_fence_info("src/components/button.tsx"),
        Some(DiffSyntaxLanguage::Tsx)
    );
}

/// Templates are mostly prose, and an unconditional single-quote rule painted
/// the rest of the line from the first `It's`.
#[test]
fn markup_prose_apostrophes_do_not_open_a_string() {
    for language in [
        DiffSyntaxLanguage::Jinja,
        DiffSyntaxLanguage::Html,
        DiffSyntaxLanguage::Vue,
        DiffSyntaxLanguage::Xml,
    ] {
        for line in [
            "  <p>It's a test</p>",
            "  <p>don't panic</p>",
            "  <li>{{ user.name }}'s profile</li>",
        ] {
            assert!(
                heuristic_string_spans(line, language).is_empty(),
                "{language:?} treated a prose apostrophe as a quote in {line:?}: {:?}",
                heuristic_tokens(line, language)
            );
        }
    }
}

/// ... while a `'` in value position is still a quote, which is why the rule is
/// positional rather than a flat "markup has no single quotes".

/// ... while a `'` in value position is still a quote, which is why the rule is
/// positional rather than a flat "markup has no single quotes".
#[test]
fn markup_single_quoted_values_are_still_strings() {
    assert_eq!(
        heuristic_string_spans("  <div class='card'>", DiffSyntaxLanguage::Html),
        vec!["'card'"]
    );
    assert_eq!(
        heuristic_string_spans("  {{ x|default('n/a') }}", DiffSyntaxLanguage::Jinja),
        vec!["'n/a'"]
    );
    assert_eq!(
        heuristic_string_spans("  {% if y == 'z' %}", DiffSyntaxLanguage::Jinja),
        vec!["'z'"]
    );
}

/// The positional rule must not leak into languages where `'` really does open
/// a string anywhere -- Rust byte and char literals are the sharp case.

#[test]
fn injection_cache_reuses_parsed_injection_across_chunks() {
    // Create an HTML document with a <script> block that spans multiple chunks
    // (> 64 lines). The injection cache should parse it once and reuse across chunks.
    let mut lines = Vec::new();
    lines.push("<html><body>".to_string());
    lines.push("<script>".to_string());
    for ix in 0..(TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS + 20) {
        lines.push(format!("const value_{ix} = {ix};"));
    }
    lines.push("</script>".to_string());
    lines.push("</body></html>".to_string());

    let doc = prepare_test_document(DiffSyntaxLanguage::Html, &lines.join("\n"));

    // Request a line from the first chunk (inside the script block)
    let first_chunk_line = 5;
    let tokens_a = syntax_tokens_for_prepared_document_line(doc, first_chunk_line)
        .expect("tokens for first chunk line should be available");
    assert!(
        tokens_a.iter().any(|t| t.kind == SyntaxTokenKind::Keyword),
        "first chunk should have JavaScript keyword tokens via injection, got: {tokens_a:?}"
    );

    // Request a line from the second chunk (also inside the script block)
    let second_chunk_line = TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS + 2;
    let tokens_b = syntax_tokens_for_prepared_document_line(doc, second_chunk_line)
        .expect("tokens for second chunk line should be available");
    assert!(
        tokens_b.iter().any(|t| t.kind == SyntaxTokenKind::Keyword),
        "second chunk should also have JavaScript keyword tokens (cached injection), got: {tokens_b:?}"
    );
}

#[test]
fn injection_cache_content_hash_distinguishes_different_documents() {
    // Two HTML documents that produce <script> injections at similar byte
    // positions but with different JavaScript content. The content_hash on
    // TreesitterInjectionMatch should prevent the second document from
    // reusing cached tokens from the first.
    TS_INJECTION_CACHE.with(|cache| cache.borrow_mut().clear());

    let doc_a = prepare_test_document(
        DiffSyntaxLanguage::Html,
        "<html><body><script>\nconst alpha = 42;\n</script></body></html>",
    );

    // Fetch tokens from doc A's injection line to populate cache
    let tokens_a =
        syntax_tokens_for_prepared_document_line(doc_a, 1).expect("doc A should have tokens");
    assert!(
        tokens_a.iter().any(|t| t.kind == SyntaxTokenKind::Keyword),
        "doc A injection line should have keyword token, got: {tokens_a:?}"
    );

    // Doc B: different JS content at a similar structure but different text
    let doc_b = prepare_test_document(
        DiffSyntaxLanguage::Html,
        "<html><body><script>\nlet beta = \"hello\";\n</script></body></html>",
    );

    let tokens_b =
        syntax_tokens_for_prepared_document_line(doc_b, 1).expect("doc B should have tokens");
    assert!(
        tokens_b.iter().any(|t| t.kind == SyntaxTokenKind::Keyword),
        "doc B injection line should have keyword token, got: {tokens_b:?}"
    );
    // The token sets should differ since the JS content differs.
    // With the content hash, doc B gets its own injection parse.
    let a_kinds: Vec<_> = tokens_a.iter().map(|t| (t.range.clone(), t.kind)).collect();
    let b_kinds: Vec<_> = tokens_b.iter().map(|t| (t.range.clone(), t.kind)).collect();
    assert_ne!(
        a_kinds, b_kinds,
        "different JS content should produce different token sets"
    );

    TS_INJECTION_CACHE.with(|cache| cache.borrow_mut().clear());
}

#[test]
fn injected_web_helper_languages_capture_basic_tokens() {
    let regex_text = "(foo|bar)+";
    let regex_tokens =
        syntax_tokens_for_line(regex_text, DiffSyntaxLanguage::Regex, DiffSyntaxMode::Auto);
    assert!(
        regex_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Operator),
        "Regex syntax should capture operators, got: {regex_tokens:?}"
    );

    let jsdoc_text = "@param {string} name";
    let jsdoc_tokens =
        syntax_tokens_for_line(jsdoc_text, DiffSyntaxLanguage::Jsdoc, DiffSyntaxMode::Auto);
    assert!(
        jsdoc_tokens
            .iter()
            .any(|t| t.kind == SyntaxTokenKind::Keyword),
        "JSDoc syntax should capture tags as keywords, got: {jsdoc_tokens:?}"
    );
}

/// The inventory tripwire.
///
/// Combined injections change how a grammar's whole document is assembled, so
/// a grammar bump that quietly introduces the directive must not slip through
/// review. F#'s `xml_doc` rule is the only one in the tree today; it arrived
/// with the upstream `tree_sitter_fsharp::INJECTIONS_QUERY` rather than being
/// written here.
#[test]
fn combined_injection_declarations_are_exactly_the_known_set() {
    let mut declared = Vec::new();
    for lang in all_supported_languages() {
        let Some(spec) = tree_sitter_highlight_spec(lang) else {
            continue;
        };
        for (pattern_ix, combined) in spec.injection_combined_patterns.iter().enumerate() {
            if *combined {
                declared.push((lang, pattern_ix));
            }
        }
        assert_eq!(
            spec.has_combined_injections,
            spec.injection_combined_patterns.iter().any(|c| *c),
            "{lang:?} has a stale has_combined_injections flag"
        );
    }
    assert_eq!(
        declared,
        vec![
            // queries/jinja_injections.scm -- the HTML around the template tags.
            (DiffSyntaxLanguage::Jinja, 0),
            // queries/nix_injections.scm -- bash in script/hook attributes.
            (DiffSyntaxLanguage::Nix, 0),
            (DiffSyntaxLanguage::Nix, 1),
            (DiffSyntaxLanguage::Nix, 2),
            (DiffSyntaxLanguage::Nix, 3),
            // Upstream tree_sitter_fsharp::INJECTIONS_QUERY -- `xml_doc` lines.
            (DiffSyntaxLanguage::FSharp, 3),
        ],
        "the set of grammars declaring `#set! injection.combined` changed. Every entry \
         here parses all its matches as one document via set_included_ranges, so a new \
         one needs the gap-clipping and cache behaviour reviewed -- it is not a \
         drop-in.\nfound: {declared:?}"
    );
}

// The one-range cases are the point: a single included range is the shape
// every non-combined injection has, and both helpers have to leave it alone.
#[allow(clippy::single_range_in_vec_init)]
#[test]
fn combined_injection_gaps_are_the_complement_within_the_window() {
    assert_eq!(combined_injection_gaps(0..100, &[]), vec![0..100]);
    assert!(combined_injection_gaps(0..100, &[0..100]).is_empty());
    assert_eq!(
        combined_injection_gaps(0..100, &[10..20, 30..40]),
        vec![0..10, 20..30, 40..100]
    );
    // Range flush against each edge produces no leading/trailing gap.
    assert_eq!(combined_injection_gaps(0..100, &[0..20]), vec![20..100]);
    assert_eq!(combined_injection_gaps(0..100, &[80..100]), vec![0..80]);
    // Ranges reaching outside the window are clipped to it, not extrapolated.
    assert_eq!(combined_injection_gaps(20..80, &[0..30]), vec![30..80]);
    assert_eq!(combined_injection_gaps(20..80, &[70..200]), vec![20..70]);
    assert!(combined_injection_gaps(20..80, &[0..200]).is_empty());
    // A range entirely outside contributes nothing and does not swallow the window.
    assert_eq!(combined_injection_gaps(20..80, &[200..300]), vec![20..80]);
}

/// Two halves of an HTML element split across a host-grammar tag must parse as
/// one element, and the injected grammar must not colour the host bytes
/// between them.
///
/// HTML stands in for the eventual template grammar here so the test needs no
/// new dependency. The ranges are the same shape a `(text) @injection.content`
/// rule produces on a real template.

/// Two halves of an HTML element split across a host-grammar tag must parse as
/// one element, and the injected grammar must not colour the host bytes
/// between them.
///
/// HTML stands in for the eventual template grammar here so the test needs no
/// new dependency. The ranges are the same shape a `(text) @injection.content`
/// rule produces on a real template.
#[test]
fn combined_injection_parses_disjoint_ranges_as_one_document() {
    let text = "<ul>\n{% for x in xs %}<li>hi</li>{% endfor %}\n</ul>\n";
    let input = treesitter_document_input_from_text(text);
    let bytes = text.as_bytes();
    let ranges = combined_test_ranges(text);

    let spec = tree_sitter_highlight_spec(DiffSyntaxLanguage::Html).expect("html spec");
    let tree = parse_combined_injection_tree(spec, bytes, input.line_starts.as_ref(), &ranges)
        .expect("combined parse should succeed");

    assert_eq!(
        tree.root_node().start_byte(),
        ranges[0].start,
        "a tree parsed with included_ranges reports document offsets"
    );
    assert!(
        !tree.root_node().has_error(),
        "the <ul> opened before `{{% for %}}` should close after `{{% endfor %}}` when the \
         three text runs are parsed as one document: {}",
        tree.root_node().to_sexp(),
    );
}

/// The other half: nodes straddling two included ranges report a byte range
/// covering the host bytes in between, so their captures have to be clipped.

/// The other half: nodes straddling two included ranges report a byte range
/// covering the host bytes in between, so their captures have to be clipped.
#[test]
fn combined_injection_tokens_do_not_bleed_into_the_gaps() {
    let text = "<ul>\n{% for x in xs %}<li>hi</li>{% endfor %}\n</ul>\n";
    let input = treesitter_document_input_from_text(text);
    let bytes = text.as_bytes();
    let ranges = combined_test_ranges(text);
    let line_starts = input.line_starts.as_ref();

    let spec = tree_sitter_highlight_spec(DiffSyntaxLanguage::Html).expect("html spec");
    let tree = parse_combined_injection_tree(spec, bytes, line_starts, &ranges)
        .expect("combined parse should succeed");

    let line_count = line_starts.len();
    let mut tokens = collect_treesitter_document_line_tokens_for_line_window(
        &tree,
        spec,
        bytes,
        line_starts,
        0,
        line_count,
    );
    let window_end = line_region_end_byte(line_starts, bytes.len(), line_count - 1);
    for gap in combined_injection_gaps(0..window_end, &ranges) {
        subtract_absolute_range_from_document_tokens(line_starts, bytes, 0, &mut tokens, gap);
    }

    // Line 1 is `{% for x in xs %}<li>hi</li>{% endfor %}`. Only the `<li>hi</li>`
    // slice belongs to HTML; both template tags are host-grammar bytes.
    let line_start = line_starts[1];
    let html_start = text.find("<li>").expect("li") - line_start;
    let html_end = text.find("{% endfor %}").expect("endfor") - line_start;
    for token in &tokens[1] {
        assert!(
            token.range.start >= html_start && token.range.end <= html_end,
            "injected HTML token {:?} escaped its included range \
             ({html_start}..{html_end}) into a `{{% … %}}` gap",
            token.range,
        );
    }
    assert!(
        !tokens[1].is_empty(),
        "clipping should not have removed the genuine <li> tokens as well"
    );
}

/// An 8-column table row used to produce 513 ranges in one 64-line chunk, one
/// over the ceiling, and the whole chunk lost its HTML.

/// The clip region is the window plus a margin on both sides, and the margin is
/// load-bearing rather than decorative -- see the constant.
#[test]
fn combined_injection_clip_region_pads_the_window_on_both_sides() {
    let text = dense_jinja_table(400, 2);
    let input = treesitter_document_input_from_text(&text);
    let line_starts = input.line_starts.as_ref();
    let len = text.len();

    let start_line_ix = 200usize;
    let end_line_ix = start_line_ix + TS_DOCUMENT_LINE_TOKEN_CHUNK_ROWS;
    let region = combined_injection_clip_region(line_starts, len, start_line_ix, end_line_ix);
    let window_start = line_starts[start_line_ix];
    let window_end = line_region_end_byte(line_starts, len, end_line_ix - 1);

    assert!(
        region.start < window_start && region.end > window_end,
        "clip region {region:?} must strictly contain the window \
         {window_start}..{window_end}"
    );
    assert_eq!(
        window_start - region.start,
        TS_COMBINED_INJECTION_CONTEXT_MARGIN_BYTES,
        "leading margin"
    );

    // ... and still bounded, which is what makes the ceilings window-scoped.
    assert!(
        region.end - region.start
            < window_end - window_start + 2 * TS_COMBINED_INJECTION_CONTEXT_MARGIN_BYTES + 1,
        "clip region must not grow past window + 2 * margin"
    );

    // At the top of the document the margin runs out rather than underflowing.
    let head = combined_injection_clip_region(line_starts, len, 0, 8);
    assert_eq!(head.start, 0, "no underflow at the start of the document");
}

/// A cut that touches nothing must leave the line's tokens exactly as they were,
/// and must not reallocate to do it.

/// Pins the ordering rather than a symptom: no in-tree grammar declares both
/// kinds over one span yet, but with combined applied first an overlapping
/// single would delete its tokens and repaint only part of the span.
#[test]
fn combined_injection_groups_are_applied_after_the_single_ones() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/view/rows/diff_text/syntax/prepared.rs"
    ))
    .expect("prepared.rs should be readable");
    let body_start = source
        .find("fn apply_injection_query_tokens_for_document")
        .expect("the function that applies both kinds of layer");
    let body = &source[body_start..];
    let body_end = body.find("\n}\n").expect("the end of the function");
    let body = &body[..body_end];

    let singles_at = body
        .find("for injection in &injections.singles")
        .expect("the singles loop");
    let combined_at = body
        .find("for group in &injections.combined")
        .expect("the combined loop");
    assert!(
        singles_at < combined_at,
        "the singles loop must run before the combined one, or a single's \
         subtraction erases combined tokens nothing repaints"
    );
}

/// F# XML doc comments are the one in-tree consumer of `injection.combined`.
///
/// `xml_doc` is a per-line token, so before combined support each `///` line
/// was its own XML document: `<summary>` on one line and `</summary>` on
/// another never met, and each cost an entry in the 32-slot injection cache.

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
#[test]
fn combined_injections_do_not_consume_the_per_node_injection_cache() {
    TS_INJECTION_CACHE.with(|cache| cache.borrow_mut().clear());

    let mut lines = vec!["/// <summary>".to_string()];
    for ix in 0..200 {
        lines.push(format!("/// line {ix}"));
    }
    lines.push("/// </summary>".to_string());
    lines.push("let add x y = x + y".to_string());
    let line_count = lines.len();

    let doc = prepare_test_document(DiffSyntaxLanguage::FSharp, &lines.join("\n"));
    for line_ix in 0..line_count {
        let _ = syntax_tokens_for_prepared_document_line(doc, line_ix);
    }

    let cached = TS_INJECTION_CACHE.with(|cache| cache.borrow().len());
    assert_eq!(
        cached, 0,
        "a combined layer's identity is a set of ranges, not one node's content \
         hash, so it must not enter TS_INJECTION_CACHE (cap \
         {TS_INJECTION_CACHE_MAX_ENTRIES}); {line_count} lines of xml doc comment \
         created {cached} entries"
    );

    TS_INJECTION_CACHE.with(|cache| cache.borrow_mut().clear());
}

/// The failure this would cause is invisible and global.
///
/// `TS_PARSER` is pooled and its included ranges are sticky; `with_ts_parser`
/// can skip `set_language` entirely on the fast path, so a combined parse that
/// forgot to clear them would truncate the *next* root parse on this thread —
/// for any language, with no error anywhere. Asserted behaviourally so it
/// survives tree-sitter API changes.

/// The failure this would cause is invisible and global.
///
/// `TS_PARSER` is pooled and its included ranges are sticky; `with_ts_parser`
/// can skip `set_language` entirely on the fast path, so a combined parse that
/// forgot to clear them would truncate the *next* root parse on this thread —
/// for any language, with no error anywhere. Asserted behaviourally so it
/// survives tree-sitter API changes.
#[test]
fn combined_injection_parse_clears_the_pooled_parsers_included_ranges() {
    let fsharp = ["/// <summary>", "/// x", "/// </summary>", "let x = 1"];
    let _ = prepare_test_document(DiffSyntaxLanguage::FSharp, &fsharp.join("\n"));

    let mut rust_lines = Vec::new();
    for ix in 0..300 {
        rust_lines.push(format!("fn f{ix}() -> u32 {{ {ix} }}"));
    }
    let last_ix = rust_lines.len() - 1;
    let last_line = rust_lines[last_ix].clone();
    let doc = prepare_test_document(DiffSyntaxLanguage::Rust, &rust_lines.join("\n"));

    let kinds = token_kinds_for_line_fragment(doc, last_ix, &last_line, "fn");
    assert!(
        kinds.contains(&SyntaxTokenKind::Keyword),
        "the last line of a 300-line Rust document lost its tokens after a combined \
         injection ran on this thread -- the pooled parser's included ranges were not \
         cleared, so the root parse was truncated: {kinds:?}"
    );
}

/// A `(text)`-style combined rule fires once per node, so this is the query
/// most likely to overflow the in-progress match pool. Overflow is worse for a
/// combined layer than a single one: tree-sitter discards matches silently, and
/// a missing range changes the document the injected grammar assembles.

#[test]
fn injection_cache_lru_eviction_preserves_recent_entries() {
    TS_INJECTION_CACHE.with(|cache| cache.borrow_mut().clear());

    // Fill the cache to max capacity with distinct entries, using the
    // global counter so access values are monotonically ordered.
    for i in 0..TS_INJECTION_CACHE_MAX_ENTRIES {
        let key = TreesitterInjectionMatch {
            language: DiffSyntaxLanguage::JavaScript,
            byte_start: i * 100,
            byte_end: i * 100 + 50,
            content_hash: i as u64,
        };
        let access = next_injection_access();
        TS_INJECTION_CACHE.with(|cache| {
            cache.borrow_mut().insert(
                key,
                CachedInjectionTokens {
                    all_line_tokens: vec![],
                    injection_line_starts: vec![],
                    injection_start_line_ix: 0,
                    last_access: access,
                },
            );
        });
    }

    // Access the first entry to make it "recent" (higher counter than all others).
    let first_key = TreesitterInjectionMatch {
        language: DiffSyntaxLanguage::JavaScript,
        byte_start: 0,
        byte_end: 50,
        content_hash: 0,
    };
    TS_INJECTION_CACHE.with(|cache| {
        if let Some(entry) = cache.borrow_mut().get_mut(&first_key) {
            entry.last_access = next_injection_access();
        }
    });

    // Now insert one more to trigger eviction.
    let overflow_key = TreesitterInjectionMatch {
        language: DiffSyntaxLanguage::JavaScript,
        byte_start: 99900,
        byte_end: 99950,
        content_hash: 99999,
    };
    let access = next_injection_access();
    TS_INJECTION_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= TS_INJECTION_CACHE_MAX_ENTRIES {
            let mut entries: Vec<_> = cache.iter().map(|(k, v)| (*k, v.last_access)).collect();
            entries.sort_unstable_by_key(|(_, a)| *a);
            let evict_count = entries.len() / 2;
            for (key, _) in entries.into_iter().take(evict_count) {
                cache.remove(&key);
            }
        }
        cache.insert(
            overflow_key,
            CachedInjectionTokens {
                all_line_tokens: vec![],
                injection_line_starts: vec![],
                injection_start_line_ix: 0,
                last_access: access,
            },
        );
    });

    TS_INJECTION_CACHE.with(|cache| {
        let cache = cache.borrow();
        // The recently-accessed first entry should survive eviction.
        assert!(
            cache.contains_key(&first_key),
            "recently accessed entry should survive LRU eviction"
        );
        // The new entry should be present.
        assert!(
            cache.contains_key(&overflow_key),
            "newly inserted entry should be present"
        );
        // Cache should be below max.
        assert!(
            cache.len() <= TS_INJECTION_CACHE_MAX_ENTRIES,
            "cache should not exceed max entries"
        );
    });

    TS_INJECTION_CACHE.with(|cache| cache.borrow_mut().clear());
}
