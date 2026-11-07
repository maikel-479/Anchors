use comment_annotations_core::Config;
use std::path::Path;

pub fn check(path: Option<String>) -> i32 {
    let cfg = load(path);
    match cfg {
        Ok(_) => { println!("OK"); 0 }
        Err(e) => { eprintln!("error: {e}"); 2 }
    }
}

pub fn print(path: Option<String>) -> i32 {
    match load(path) {
        Ok(cfg) => {
            match toml::to_string(&cfg) {
                Ok(s) => { print!("{s}"); 0 }
                Err(e) => { eprintln!("error: serialize: {e}"); 2 }
            }
        }
        Err(e) => { eprintln!("error: {e}"); 2 }
    }
}

fn load(path: Option<String>) -> Result<Config, String> {
    if let Some(p) = path {
        Config::from_file(Path::new(&p)).map_err(|e| e.to_string())
    } else {
        let local = Path::new("anchors.toml");
        let local_old = Path::new("comment-annotations.toml");
        let local2 = Path::new(".anchors.toml");
        if local.exists() {
            Config::from_file(local).map_err(|e| e.to_string())
        } else if local_old.exists() {
            Config::from_file(local_old).map_err(|e| e.to_string())
        } else if local2.exists() {
            Config::from_file(local2).map_err(|e| e.to_string())
        } else {
            Ok(Config::default())
        }
    }
}
