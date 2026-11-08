use canc_core::config::{InlineMode, Severity, UnknownTags};
use canc_core::Config;

/// A config that declares one custom tag must keep the default tag set
/// (merged by name), not replace it.
#[test]
fn partial_tags_extend_defaults() {
    let cfg = Config::from_toml_str(
        r#"
        [[tags]]
        name = "PERF"
        class = "perf"
        severity = "hint"
        symbol = false
        "#,
    )
    .unwrap();
    let names = cfg.tag_names_set();
    for default in ["NOTE", "TODO", "FIXME", "BUG", "HACK"] {
        assert!(
            names.contains(&default.to_string()),
            "default tag {default} lost; tags = {names:?}"
        );
    }
    assert!(names.contains(&"PERF".to_string()));
    // Overlay by name: redefining FIXME keeps the default class/severity.
    let cfg = Config::from_toml_str(
        r#"
        [[tags]]
        name = "FIXME"
        class = "fixme"
        severity = "error"
        "#,
    )
    .unwrap();
    let (_, sev, _) = cfg.lookup_tag("FIXME").unwrap();
    assert_eq!(sev, Severity::Error);
}

/// Fields omitted from a file keep their defaults (or global values when
/// merged via load_for_file) — a one-knob TOML must not zero the rest.
#[test]
fn partial_syntax_keeps_other_fields() {
    let cfg = Config::from_toml_str(
        r#"
        [syntax]
        conventional = false
        "#,
    )
    .unwrap();
    assert!(!cfg.syntax.conventional);
    // unspecified knobs keep defaults:
    assert_eq!(cfg.syntax.inline, InlineMode::All);
    assert!(cfg.syntax.delimited);
    assert_eq!(cfg.syntax.unknown_tags, UnknownTags::Ignore);
}

/// Global ↔ project merge: project syntax overrides only the fields it sets;
/// global custom syntax survives otherwise.
#[test]
fn merge_overrides_global_fieldwise() {
    let dir = std::env::temp_dir().join(format!("canc-cfg-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let global = dir.join("global.toml");
    let project = dir.join("anchors.toml");
    std::fs::write(
        &global,
        r#"
        [syntax]
        inline = "off"
        conventional = false
        "#,
    )
    .unwrap();
    std::fs::write(
        &project,
        r#"
        [[tags]]
        name = "PERF"
        class = "perf"
        "#,
    )
    .unwrap();

    let cfg = Config::load_for_file(&project.join("some.rs"), Some(&global)).unwrap();
    // From global:
    assert_eq!(cfg.syntax.inline, InlineMode::Off);
    // Project flow-through didn't set these: global values persist.
    assert!(!cfg.syntax.conventional);
    // Project tags extend defaults:
    assert!(cfg.tag_names_set().contains(&"PERF".to_string()));
    assert!(cfg.tag_names_set().contains(&"TODO".to_string()));

    // A project file that sets one syntax field overrides just that field.
    std::fs::write(&project, "[syntax]\ninline = \"tagged\"\n").unwrap();
    let cfg = Config::load_for_file(&project.join("some.rs"), Some(&global)).unwrap();
    assert_eq!(cfg.syntax.inline, InlineMode::Tagged);
    assert!(!cfg.syntax.conventional); // global survives

    std::fs::remove_dir_all(&dir).ok();
}

/// `load_for_file` falls back to defaults when neither project nor global
/// configs exist.
#[test]
fn load_for_file_defaults() {
    let dir = std::env::temp_dir().join(format!("canc-nocfg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("x.rs");
    let missing_global = dir.join("does-not-exist.toml");
    let cfg = Config::load_for_file(&file, Some(&missing_global)).unwrap();
    assert!(cfg.tag_names_set().contains(&"NOTE".to_string()));
    std::fs::remove_dir_all(&dir).ok();
}
