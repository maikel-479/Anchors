use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    C,
    Cpp,
    Rust,
    Python,
    Go,
    JavaScript,
    TypeScript,
    Tsx,
    Html,
    Css,
    Json,
    Unknown,
}

impl Lang {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_ascii_lowercase().as_str() {
            "c" | "h" => Self::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "c++" => Self::Cpp,
            "rs" => Self::Rust,
            "py" | "pyw" => Self::Python,
            "go" => Self::Go,
            "js" | "mjs" | "cjs" => Self::JavaScript,
            "ts" | "mts" | "cts" => Self::TypeScript,
            "tsx" => Self::Tsx,
            "html" | "htm" | "xhtml" => Self::Html,
            "css" => Self::Css,
            "json" | "jsonc" => Self::Json,
            _ => Self::Unknown,
        }
    }

    pub fn from_language_id(id: &str) -> Self {
        match id.to_ascii_lowercase().as_str() {
            "c" => Self::C,
            "cpp" | "c++" => Self::Cpp,
            "rust" => Self::Rust,
            "python" => Self::Python,
            "go" => Self::Go,
            "javascript" => Self::JavaScript,
            "typescript" => Self::TypeScript,
            "typescriptreact" | "tsx" => Self::Tsx,
            "html" => Self::Html,
            "css" => Self::Css,
            "json" | "jsonc" => Self::Json,
            _ => Self::Unknown,
        }
    }

    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => Self::from_extension(ext),
            None => Self::Unknown,
        }
    }

    pub fn ts_language(&self) -> Option<tree_sitter::Language> {
        match self {
            Self::C => Some(tree_sitter_c::LANGUAGE.into()),
            Self::Cpp => Some(tree_sitter_cpp::LANGUAGE.into()),
            Self::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
            Self::Python => Some(tree_sitter_python::LANGUAGE.into()),
            Self::Go => Some(tree_sitter_go::LANGUAGE.into()),
            Self::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
            Self::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            Self::Tsx => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
            Self::Html | Self::Css | Self::Json => None,
            Self::Unknown => None,
        }
    }

    pub fn query_source(&self) -> &'static str {
        match self {
            Self::Rust => "[(line_comment) (block_comment)] @comment",
            Self::Python | Self::C | Self::Cpp | Self::Go | Self::JavaScript | Self::TypeScript | Self::Tsx => "(comment) @comment",
            Self::Html | Self::Css | Self::Json => "(comment) @comment",
            Self::Unknown => "(comment) @comment",
        }
    }

    pub fn delimiter_table(&self) -> DelimTable {
        match self {
            Self::Python => DelimTable { line: &["#"], block: &[] },
            Self::Html => DelimTable { line: &[], block: &[("<!--", "-->")] },
            Self::Css => DelimTable { line: &[], block: &[("/*", "*/")] },
            Self::Json => DelimTable { line: &["//"], block: &[("/*", "*/")] },
            _ => DelimTable { line: &["//"], block: &[("/*", "*/")] },
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Rust => "rust",
            Self::Python => "python",
            Self::Go => "go",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Tsx => "typescriptreact",
            Self::Html => "html",
            Self::Css => "css",
            Self::Json => "json",
            Self::Unknown => "unknown",
        }
    }
}

pub struct DelimTable {
    pub line: &'static [&'static str],
    pub block: &'static [(&'static str, &'static str)],
}
