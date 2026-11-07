use comment_annotations_core::{Config, Lang, scan, SpanKind, TagForm};

#[test]
fn worked_example_cpp() {
    let src = "#include <iostream>\n\n// =NOTE=: This is how you start a c++ file\nint main(){\n// This is called the *main* function.\n}\n";
    let cfg = Config::default();
    let result = scan(Lang::Cpp, src, &cfg);
    assert_eq!(result.anchors.len(), 1, "anchors: {:?}", result.anchors);
    let a = &result.anchors[0];
    assert_eq!(a.tag, "NOTE");
    assert_eq!(a.form, TagForm::Delimited);
    assert_eq!(a.text, "This is how you start a c++ file");
    // line is 0-based (2)
    assert_eq!(a.line, 2);
    // anchor text is the tag match
    let anchor_text = &src[a.range.start..a.range.end];
    assert!(anchor_text.contains("NOTE"), "anchor span text={anchor_text:?}");
    // one emphasis span for *main* (commonmark default)
    assert_eq!(result.spans.len(), 1, "spans: {:?}", result.spans);
    assert_eq!(result.spans[0].kind, SpanKind::Emphasis);
    let span_text = &src[result.spans[0].range.clone()];
    assert_eq!(span_text, "*main*");
}

#[test]
fn conventional_tags() {
    let src = "// NOTE: hello\n// TODO(alice): fix this\n// FIXME: broken\n// RANDOM: not a tag\n";
    let cfg = Config::default();
    let result = scan(Lang::Rust, src, &cfg);
    let tags: Vec<_> = result.anchors.iter().map(|a| a.tag.as_str()).collect();
    assert_eq!(tags, vec!["NOTE", "TODO", "FIXME"]);
    assert_eq!(result.anchors[1].meta.as_deref(), Some("alice"));
    assert_eq!(result.anchors[1].class, "todo");
}

#[test]
fn inline_strong_emphasis_code() {
    let src = "// NOTE: this is **bold** and *italic* and `code span` ok\n";
    let cfg = Config::default();
    let result = scan(Lang::Cpp, src, &cfg);
    let kinds: Vec<_> = result.spans.iter().map(|s| s.kind).collect();
    assert!(kinds.contains(&SpanKind::Strong), "kinds={kinds:?}");
    assert!(kinds.contains(&SpanKind::Emphasis), "kinds={kinds:?}");
    assert!(kinds.contains(&SpanKind::Code), "kinds={kinds:?}");
}

#[test]
fn no_false_positive_multiplication() {
    let src = "// NOTE: a * b and int *p, *q\n// * bullet item\n";
    let cfg = Config::default();
    let result = scan(Lang::Cpp, src, &cfg);
    assert!(result.spans.is_empty(), "spans: {:?}", result.spans);
}

#[test]
fn python_comments() {
    let src = "# NOTE: python note\n# TODO(bob): do it\nx = 1  # FIXME: inside\n";
    let cfg = Config::default();
    let result = scan(Lang::Python, src, &cfg);
    let tags: Vec<_> = result.anchors.iter().map(|a| a.tag.as_str()).collect();
    assert_eq!(tags, vec!["NOTE", "TODO", "FIXME"]);
}

#[test]
fn block_comment_per_line() {
    let src = "/*\n * NOTE: block note\n */\nint x;\n";
    let cfg = Config::default();
    let result = scan(Lang::C, src, &cfg);
    assert_eq!(result.anchors.len(), 1, "anchors: {:?}", result.anchors);
    assert_eq!(result.anchors[0].tag, "NOTE");
}

#[test]
fn star_bold_dialect() {
    let cfg = Config::from_toml_str(r#"[syntax]
emphasis = "star-bold"
"#).unwrap();
    let src = "// NOTE: this is *main* function\n";
    let result = scan(Lang::Cpp, src, &cfg);
    assert_eq!(result.spans.len(), 1);
    assert_eq!(result.spans[0].kind, SpanKind::Strong);
}

#[test]
fn inline_off() {
    let cfg = Config::from_toml_str(r#"[syntax]
inline = "off"
"#).unwrap();
    let src = "// NOTE: this is *main* function\n";
    let result = scan(Lang::Cpp, src, &cfg);
    assert!(result.spans.is_empty());
}

#[test]
fn non_ascii_offsets() {
    let src = "// NOTE: café ☕ *main*\n";
    let cfg = Config::default();
    let result = scan(Lang::Cpp, src, &cfg);
    assert_eq!(result.anchors.len(), 1);
    // span should be valid byte range
    let span_text = &src[result.spans[0].range.clone()];
    assert_eq!(span_text, "*main*");
}

#[test]
fn unknown_tags_highlight() {
    let cfg = Config::from_toml_str(r#"[syntax]
unknown_tags = "highlight"
"#).unwrap();
    let src = "// FROB: mysterious\n";
    let result = scan(Lang::Cpp, src, &cfg);
    assert_eq!(result.anchors.len(), 1);
    assert_eq!(result.anchors[0].tag, "FROB");
}

#[test]
fn language_detection() {
    assert_eq!(Lang::from_extension("rs"), Lang::Rust);
    assert_eq!(Lang::from_extension("py"), Lang::Python);
    assert_eq!(Lang::from_language_id("typescriptreact"), Lang::Tsx);
    assert_eq!(Lang::from_language_id("cpp"), Lang::Cpp);
}

#[test]
fn config_defaults_match_plan() {
    let cfg = Config::default();
    assert_eq!(cfg.syntax.conventional, true);
    assert_eq!(cfg.syntax.delimited, true);
    let note = cfg.lookup_tag("NOTE").unwrap();
    assert_eq!(note.1, comment_annotations_core::Severity::Off);
    let todo = cfg.lookup_tag("TODO").unwrap();
    assert_eq!(todo.1, comment_annotations_core::Severity::Hint);
    let fixme = cfg.lookup_tag("FIXME").unwrap();
    assert_eq!(fixme.1, comment_annotations_core::Severity::Information);
}

#[test]
fn empty_and_no_comments() {
    let cfg = Config::default();
    let r = scan(Lang::Cpp, "int main() {}\n", &cfg);
    assert!(r.anchors.is_empty());
    assert!(r.spans.is_empty());
}

#[test]
fn extension_languages_match_core_table() {
    let manifest = std::fs::read_to_string("../../extension/extension.toml").unwrap();
    for lang in Lang::all() {
        let zed = lang.zed_name();
        assert!(manifest.contains(&format!("\"{zed}\"")), "extension.toml missing language {zed}");
        let lsp = lang.name();
        assert!(manifest.contains(&format!("\"{zed}\" = \"{lsp}\"")), "extension.toml missing language_id {zed} -> {lsp}");
    }
}
