use super::*;

#[test]
fn vendored_rust_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    let source = RUST_HIGHLIGHTS_QUERY;
    tree_sitter::Query::new(&lang, source).expect("vendored Rust highlights.scm should compile");
}

#[test]
fn vendored_css_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_css::LANGUAGE.into();
    let source = CSS_HIGHLIGHTS_QUERY;
    tree_sitter::Query::new(&lang, source).expect("vendored CSS highlights.scm should compile");
}

#[test]
fn vendored_bash_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_bash::LANGUAGE.into();
    tree_sitter::Query::new(&lang, BASH_HIGHLIGHTS_QUERY)
        .expect("vendored Bash highlights.scm should compile");
}

#[test]
fn vendored_html_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_html::LANGUAGE.into();
    let source = HTML_HIGHLIGHTS_QUERY;
    tree_sitter::Query::new(&lang, source).expect("vendored HTML highlights.scm should compile");
}

#[test]
fn vendored_html_injections_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_html::LANGUAGE.into();
    tree_sitter::Query::new(&lang, HTML_INJECTIONS_QUERY)
        .expect("vendored HTML injections.scm should compile");
}

/// The Vue grammar is the one grammar we vendor rather than pull from
/// crates.io, so nothing external will tell us when it stops matching the
/// workspace `tree-sitter`. It binds through `tree-sitter-language`, which
/// means a tree-sitter upgrade only stays safe while this holds.

/// The Vue grammar is the one grammar we vendor rather than pull from
/// crates.io, so nothing external will tell us when it stops matching the
/// workspace `tree-sitter`. It binds through `tree-sitter-language`, which
/// means a tree-sitter upgrade only stays safe while this holds.
#[test]
fn vendored_vue_grammar_is_abi_compatible_with_workspace_tree_sitter() {
    let vue: tree_sitter::Language = tree_sitter_vue::LANGUAGE.into();
    let abi = vue.abi_version();
    assert!(
        (tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION..=tree_sitter::LANGUAGE_VERSION)
            .contains(&abi),
        "vendored Vue grammar ABI {abi} is outside the range this tree-sitter supports \
         ({}..={}); regenerate vendor/tree-sitter-vue with a newer tree-sitter-cli",
        tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION,
        tree_sitter::LANGUAGE_VERSION,
    );
}

// There is deliberately no test comparing the vendored ABI against the
// crates.io grammars'. Being *older* than tree-sitter-html is not a defect
// -- ABI versions stay supported across a wide range, which is exactly what
// the test above checks. Asserting `vue.abi >= html.abi` would instead turn
// any routine `cargo update` that bumps an unrelated grammar into a red CI
// while Vue still parses perfectly.

#[test]
fn vendored_vue_grammar_parses_with_workspace_tree_sitter() {
    let source = concat!(
        "<template>\n",
        "  <p v-if=\"ok\">{{ msg }}</p>\n",
        "</template>\n",
        "\n",
        "<script setup lang=\"ts\">\n",
        "const msg: string = 'hi';\n",
        "</script>\n",
    );
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_vue::LANGUAGE.into())
        .expect("vendored Vue grammar should load into the workspace tree-sitter");
    let tree = parser
        .parse(source, None)
        .expect("vendored Vue grammar should parse an SFC");
    assert!(
        !tree.root_node().has_error(),
        "vendored Vue grammar produced an ERROR node for a well-formed SFC: {}",
        tree.root_node().to_sexp(),
    );
}

/// Every language the Vue injections name has to be a language this
/// repository actually ships a grammar for, or the injection silently
/// no-ops. Reading the targets back off the compiled query keeps this
/// honest when the query changes.

#[test]
fn vendored_vue_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_vue::LANGUAGE.into();
    tree_sitter::Query::new(&lang, VUE_HIGHLIGHTS_QUERY)
        .expect("vendored Vue highlights.scm should compile");
}

#[test]
fn vendored_vue_injections_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_vue::LANGUAGE.into();
    tree_sitter::Query::new(&lang, VUE_INJECTIONS_QUERY)
        .expect("vendored Vue injections.scm should compile");
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
fn vendored_javascript_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_javascript::LANGUAGE.into();
    tree_sitter::Query::new(&lang, JAVASCRIPT_HIGHLIGHTS_QUERY)
        .expect("vendored JavaScript highlights.scm should compile against JS grammar");
}

#[test]
fn vendored_javascript_injections_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_javascript::LANGUAGE.into();
    tree_sitter::Query::new(&lang, JAVASCRIPT_INJECTIONS_QUERY)
        .expect("vendored JavaScript injections.scm should compile against JS grammar");
}

#[test]
fn vendored_typescript_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    tree_sitter::Query::new(&lang, TYPESCRIPT_HIGHLIGHTS_QUERY)
        .expect("vendored TypeScript highlights.scm should compile");
}

#[test]
fn vendored_typescript_injections_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    tree_sitter::Query::new(&lang, TYPESCRIPT_INJECTIONS_QUERY)
        .expect("vendored TypeScript injections.scm should compile");
}

#[test]
fn vendored_tsx_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TSX.into();
    tree_sitter::Query::new(&lang, TSX_HIGHLIGHTS_QUERY)
        .expect("vendored TSX highlights.scm should compile");
}

#[test]
fn vendored_tsx_injections_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TSX.into();
    tree_sitter::Query::new(&lang, TSX_INJECTIONS_QUERY)
        .expect("vendored TSX injections.scm should compile");
}

#[test]
fn vendored_go_queries_compile() {
    let lang: tree_sitter::Language = tree_sitter_go::LANGUAGE.into();
    tree_sitter::Query::new(&lang, GO_HIGHLIGHTS_QUERY)
        .expect("vendored Go highlights.scm should compile");
    tree_sitter::Query::new(&lang, GO_INJECTIONS_QUERY)
        .expect("vendored Go injections.scm should compile");
}

#[test]
fn vendored_json_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_json::LANGUAGE.into();
    tree_sitter::Query::new(&lang, JSON_HIGHLIGHTS_QUERY)
        .expect("vendored JSON highlights.scm should compile");
}

#[test]
fn vendored_python_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    tree_sitter::Query::new(&lang, PYTHON_HIGHLIGHTS_QUERY)
        .expect("vendored Python highlights.scm should compile");
}

#[test]
fn vendored_yaml_queries_compile() {
    let lang: tree_sitter::Language = tree_sitter_yaml::LANGUAGE.into();
    tree_sitter::Query::new(&lang, YAML_HIGHLIGHTS_QUERY)
        .expect("vendored YAML highlights.scm should compile");
    tree_sitter::Query::new(&lang, YAML_INJECTIONS_QUERY)
        .expect("vendored YAML injections.scm should compile");
}

#[test]
fn vendored_csharp_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_c_sharp::LANGUAGE.into();
    tree_sitter::Query::new(&lang, CSHARP_HIGHLIGHTS_QUERY)
        .expect("vendored C# highlights.scm should compile");
}

#[test]
fn vendored_c_queries_compile() {
    let lang: tree_sitter::Language = tree_sitter_c::LANGUAGE.into();
    tree_sitter::Query::new(&lang, C_HIGHLIGHTS_QUERY)
        .expect("vendored C highlights.scm should compile");
    tree_sitter::Query::new(&lang, C_INJECTIONS_QUERY)
        .expect("vendored C injections.scm should compile");
}

#[test]
fn vendored_cpp_queries_compile() {
    let lang: tree_sitter::Language = tree_sitter_cpp::LANGUAGE.into();
    tree_sitter::Query::new(&lang, CPP_HIGHLIGHTS_QUERY)
        .expect("vendored C++ highlights.scm should compile");
    tree_sitter::Query::new(&lang, CPP_INJECTIONS_QUERY)
        .expect("vendored C++ injections.scm should compile");
}

#[test]
fn vendored_injected_web_language_queries_compile() {
    let jsdoc_lang: tree_sitter::Language = tree_sitter_jsdoc::LANGUAGE.into();
    tree_sitter::Query::new(&jsdoc_lang, JSDOC_HIGHLIGHTS_QUERY)
        .expect("vendored JSDoc highlights.scm should compile");

    let regex_lang: tree_sitter::Language = tree_sitter_regex::LANGUAGE.into();
    tree_sitter::Query::new(&regex_lang, REGEX_HIGHLIGHTS_QUERY)
        .expect("vendored regex highlights.scm should compile");
}

#[test]
fn vendored_repo_queries_compile() {
    let markdown_lang: tree_sitter::Language = tree_sitter_md::LANGUAGE.into();
    tree_sitter::Query::new(&markdown_lang, MARKDOWN_HIGHLIGHTS_QUERY)
        .expect("Markdown block highlights.scm should compile");
    tree_sitter::Query::new(&markdown_lang, MARKDOWN_INJECTIONS_QUERY)
        .expect("Markdown block injections.scm should compile");

    let markdown_inline_lang: tree_sitter::Language = tree_sitter_md::INLINE_LANGUAGE.into();
    tree_sitter::Query::new(&markdown_inline_lang, MARKDOWN_INLINE_HIGHLIGHTS_QUERY)
        .expect("Markdown inline highlights.scm should compile");

    let diff_lang: tree_sitter::Language = tree_sitter_diff::LANGUAGE.into();
    tree_sitter::Query::new(&diff_lang, tree_sitter_diff::HIGHLIGHTS_QUERY)
        .expect("Diff highlights.scm should compile");

    let gitcommit_lang: tree_sitter::Language = tree_sitter_gitcommit::LANGUAGE.into();
    tree_sitter::Query::new(&gitcommit_lang, GITCOMMIT_HIGHLIGHTS_QUERY)
        .expect("Git commit highlights.scm should compile");

    let gomod_lang: tree_sitter::Language = tree_sitter_gomod::LANGUAGE.into();
    tree_sitter::Query::new(&gomod_lang, GOMOD_HIGHLIGHTS_QUERY)
        .expect("go.mod highlights.scm should compile");

    let gowork_lang: tree_sitter::Language = tree_sitter_gowork::LANGUAGE.into();
    tree_sitter::Query::new(&gowork_lang, GOWORK_HIGHLIGHTS_QUERY)
        .expect("go.work highlights.scm should compile");
}

#[test]
fn vendored_xml_query_compiles() {
    let lang: tree_sitter::Language = tree_sitter_xml::LANGUAGE_XML.into();
    tree_sitter::Query::new(&lang, XML_HIGHLIGHTS_QUERY)
        .expect("XML highlights.scm should compile against XML grammar");
}

#[test]
fn vendored_capture_names_are_supported_or_ignored() {
    assert_capture_names_are_supported(tree_sitter_rust::LANGUAGE.into(), RUST_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(tree_sitter_html::LANGUAGE.into(), HTML_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(tree_sitter_vue::LANGUAGE.into(), VUE_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(tree_sitter_css::LANGUAGE.into(), CSS_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(tree_sitter_bash::LANGUAGE.into(), BASH_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(
        tree_sitter_javascript::LANGUAGE.into(),
        JAVASCRIPT_HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_python::LANGUAGE.into(),
        PYTHON_HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(tree_sitter_go::LANGUAGE.into(), GO_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(tree_sitter_json::LANGUAGE.into(), JSON_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(tree_sitter_yaml::LANGUAGE.into(), YAML_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        TYPESCRIPT_HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_typescript::LANGUAGE_TSX.into(),
        TSX_HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(tree_sitter_xml::LANGUAGE_XML.into(), XML_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(tree_sitter_c::LANGUAGE.into(), C_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(tree_sitter_cpp::LANGUAGE.into(), CPP_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(
        tree_sitter_c_sharp::LANGUAGE.into(),
        CSHARP_HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_java::LANGUAGE.into(),
        tree_sitter_java::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_php::LANGUAGE_PHP.into(),
        tree_sitter_php::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_ruby::LANGUAGE.into(),
        tree_sitter_ruby::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_toml_ng::LANGUAGE.into(),
        tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_lua::LANGUAGE.into(),
        tree_sitter_lua::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_make::LANGUAGE.into(),
        tree_sitter_make::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_kotlin_sg::LANGUAGE.into(),
        tree_sitter_kotlin_sg::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_zig::LANGUAGE.into(),
        tree_sitter_zig::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        dekobon_tree_sitter_groovy::LANGUAGE.into(),
        dekobon_tree_sitter_groovy::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_clojure_orchard::LANGUAGE.into(),
        CLOJURE_HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_elixir::LANGUAGE.into(),
        tree_sitter_elixir::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_erlang::LANGUAGE.into(),
        tree_sitter_erlang::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_haskell::LANGUAGE.into(),
        tree_sitter_haskell::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(tree_sitter_julia::LANGUAGE.into(), JULIA_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(
        tree_sitter_ocaml::LANGUAGE_OCAML.into(),
        OCAML_HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_solidity::LANGUAGE.into(),
        SOLIDITY_HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_asm::LANGUAGE.into(),
        tree_sitter_asm::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_svelte_ng::LANGUAGE.into(),
        SVELTE_HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_bicep::LANGUAGE.into(),
        tree_sitter_bicep::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_objc::LANGUAGE.into(),
        tree_sitter_objc::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_fsharp::LANGUAGE_FSHARP.into(),
        tree_sitter_fsharp::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_powershell::LANGUAGE.into(),
        POWERSHELL_HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_swift::LANGUAGE.into(),
        tree_sitter_swift::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(tree_sitter_jsdoc::LANGUAGE.into(), JSDOC_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(tree_sitter_regex::LANGUAGE.into(), REGEX_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(
        tree_sitter_r::LANGUAGE.into(),
        tree_sitter_r::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_dart::LANGUAGE.into(),
        tree_sitter_dart::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_scala::LANGUAGE.into(),
        tree_sitter_scala::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_sequel::LANGUAGE.into(),
        tree_sitter_sequel::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(tree_sitter_md::LANGUAGE.into(), MARKDOWN_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(
        tree_sitter_md::INLINE_LANGUAGE.into(),
        MARKDOWN_INLINE_HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_diff::LANGUAGE.into(),
        tree_sitter_diff::HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(
        tree_sitter_gitcommit::LANGUAGE.into(),
        GITCOMMIT_HIGHLIGHTS_QUERY,
    );
    assert_capture_names_are_supported(tree_sitter_gomod::LANGUAGE.into(), GOMOD_HIGHLIGHTS_QUERY);
    assert_capture_names_are_supported(
        tree_sitter_gowork::LANGUAGE.into(),
        GOWORK_HIGHLIGHTS_QUERY,
    );
}
