pub mod config;
pub mod extract;
pub mod lang;
pub mod scan;
pub mod tag;

pub use config::{Config, Severity};
pub use lang::Lang;
pub use scan::{scan, Anchor, ByteRange, Scan};
pub use tag::{Span, SpanKind, TagForm};
