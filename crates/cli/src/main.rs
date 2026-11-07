mod anchors;
mod config_cmd;
mod server;
mod init_zed;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "canc", version, about = "Comment tags and inline emphasis as structured anchors")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the LSP server over stdio
    Serve,
    /// List anchors found in files
    Anchors {
        #[arg(value_name = "PATH")]
        paths: Vec<String>,
        #[arg(long, value_enum, default_value_t = anchors::OutputFormat::Text)]
        format: anchors::OutputFormat,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        deny: Option<String>,
        #[arg(long)]
        spans: bool,
        #[arg(long)]
        no_ignore: bool,
        #[arg(long)]
        config: Option<String>,
    },
    /// Print Zed settings snippet (semantic_token_rules)
    InitZed {
        #[arg(long)]
        config: Option<String>,
    },
    /// Validate / print config
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    Check {
        #[arg(value_name = "PATH")]
        path: Option<String>,
    },
    Print {
        #[arg(value_name = "PATH")]
        path: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        // `serve` is the default when invoked with no arguments — Zed may
        // launch the binary via the `lsp.binary.path` override without the
        // extension's `serve` arg, so bare invocation must start the server.
        None | Some(Commands::Serve) => server::run(),
        Some(Commands::Anchors { paths, format, tag, deny, spans, no_ignore, config }) => {
            std::process::exit(anchors::run(paths, format, tag, deny, spans, no_ignore, config));
        }
        Some(Commands::InitZed { config }) => {
            std::process::exit(init_zed::run(config));
        }
        Some(Commands::Config { action }) => {
            let code = match action {
                ConfigAction::Check { path } => config_cmd::check(path),
                ConfigAction::Print { path } => config_cmd::print(path),
            };
            std::process::exit(code);
        }
    }
}
