use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// tansu のコマンドライン。サブコマンドの実装は後続の issue が足す。
#[derive(Debug, Parser)]
#[command(name = "tansu", version, about, arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Web 画面と HTTP API のサーバーを起動する
    Serve,
    /// MCP サーバー (stdio) を起動する
    Mcp,
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Command::Serve => "serve",
            Command::Mcp => "mcp",
        }
    }
}

// clap の `parse()` は `--version` / `--help` の時に `process::exit` で終了するため、
// `try_parse()` で受けて終了コードを main が返す。
fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(1));
        }
    };
    eprintln!(
        "{} は未実装です (進捗は documents/DIRECTION.md「必要な機能」)",
        cli.command.name()
    );
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;

    #[test]
    fn version_flag_is_handled_by_clap() {
        let error = Cli::try_parse_from(["tansu", "--version"]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::DisplayVersion);
        assert_eq!(error.exit_code(), 0);
    }

    #[test]
    fn subcommands_parse() {
        assert!(matches!(
            Cli::try_parse_from(["tansu", "serve"]).unwrap().command,
            Command::Serve
        ));
        assert!(matches!(
            Cli::try_parse_from(["tansu", "mcp"]).unwrap().command,
            Command::Mcp
        ));
    }
}
