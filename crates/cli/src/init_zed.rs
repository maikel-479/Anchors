use canc_core::Config;
use std::path::Path;

pub fn run(config_path: Option<String>) -> i32 {
    let cfg = if let Some(p) = config_path {
        match Config::from_file(Path::new(&p)) {
            Ok(c) => c,
            Err(e) => { eprintln!("error: {e}"); return 2; }
        }
    } else {
        // Check local config file if present
        let local = Path::new("anchors.toml");
        let local_old = Path::new("comment-annotations.toml");
        let local2 = Path::new(".anchors.toml");
        if local.exists() {
            match Config::from_file(local) { Ok(c) => c, Err(e) => { eprintln!("error: {e}"); return 2; } }
        } else if local_old.exists() {
            match Config::from_file(local_old) { Ok(c) => c, Err(e) => { eprintln!("error: {e}"); return 2; } }
        } else if local2.exists() {
            match Config::from_file(local2) { Ok(c) => c, Err(e) => { eprintln!("error: {e}"); return 2; } }
        } else {
            Config::default()
        }
    };

    let rules = generate_rules(&cfg);
    println!("{}", serde_json::to_string_pretty(&rules).unwrap());
    0
}

pub fn generate_rules(cfg: &Config) -> Vec<serde_json::Value> {
    let mut rules = Vec::new();
    for tag in &cfg.tags {
        let color = color_for_class(&tag.class);
        rules.push(serde_json::json!({
            "token_type": "tag",
            "token_modifiers": [tag.class],
            "foreground_color": color,
            "font_weight": "bold"
        }));
    }
    rules.push(serde_json::json!({
        "token_type": "span",
        "token_modifiers": ["strong"],
        "font_weight": "bold"
    }));
    // Italic alone is invisible when the theme already italicizes comments,
    // so emphasis also gets an underline — remove it if your theme's comments
    // are not italic and the underline feels noisy.
    rules.push(serde_json::json!({
        "token_type": "span",
        "token_modifiers": ["emphasis"],
        "font_style": "italic",
        "underline": true
    }));
    // `_..._` is deliberately upright: `font_style: normal` undoes the
    // theme's comment italics, so it reads as underlined-only text.
    rules.push(serde_json::json!({
        "token_type": "span",
        "token_modifiers": ["underscore"],
        "font_style": "normal",
        "underline": true
    }));
    rules.push(serde_json::json!({
        "token_type": "span",
        "token_modifiers": ["code"],
        "background_color": "#292e42"
    }));
    rules
}

fn color_for_class(class: &str) -> &'static str {
    match class {
        "note" => "#7aa2f7",
        "todo" => "#e0af68",
        "fixme" => "#f7768e",
        "bug" => "#f7768e",
        "hack" => "#bb9af7",
        _ => "#9aa5f4",
    }
}
