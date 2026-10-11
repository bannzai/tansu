use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use tansu_core::config::Settings;
use tansu_core::db::{self, rows, schema};
use tansu_core::qa::import::{self as qa_import, ImportReport};
use tansu_core::{export, named_json};

/// tansu のコマンドライン。DB の読み書きはすべて tansu_core の関数を呼ぶ。
/// 成功時は結果を stdout に出し (JSON は整形して出す)、失敗時は stderr に 1 行を出して exit 1 で終わる。
#[derive(Debug, Parser)]
#[command(name = "tansu", version, about, arg_required_else_help = true)]
struct Cli {
    /// 実行するサブコマンド。
    #[command(subcommand)]
    command: Command,
}

/// tansu のサブコマンド。
#[derive(Debug, Subcommand)]
enum Command {
    /// データベース (表) とプロパティの定義を作る・変える・一覧する
    #[command(subcommand)]
    Db(DbCommand),
    /// 行を絞り込んで JSON の配列で出す。--where は複数指定でき、すべてに一致する行を返す。--sort が無ければ追加した順
    Query {
        /// データベースの名前
        database: String,
        /// 出力を JSON にする (今は JSON だけを出す)
        #[arg(long)]
        json: bool,
        /// `<プロパティ名>=<値>` (等価。multi_select / relation は含む) か `<プロパティ名>^=<値>` (前方一致。text / url / date)
        #[arg(long = "where", value_name = "プロパティ=値")]
        filters: Vec<String>,
        /// `<プロパティ名>[:asc|:desc]` (text / number / checkbox / url / date)
        #[arg(long, value_name = "プロパティ[:desc]")]
        sort: Option<String>,
        /// 返す行の上限
        #[arg(long)]
        limit: Option<u64>,
    },
    /// 行を書く。`id` があればその行、無ければ unique のプロパティの値が一致する行を更新し、無ければ足す。書いた行を JSON で出す
    Upsert {
        /// データベースの名前
        database: String,
        /// 行の JSON (`{"id": ..., "body": ..., "values": {<プロパティ名>: <値>}}`)。省略すると stdin から読む
        #[arg(long, value_name = "行の JSON")]
        json: Option<String>,
    },
    /// 行を消す。relation でこの行を参照している値からも外す。行が無ければ何もしない
    Delete {
        /// データベースの名前
        database: String,
        /// 行の ID
        #[arg(allow_hyphen_values = true)]
        row_id: String,
    },
    /// データベースの定義と全行を書き出す
    Export {
        /// データベースの名前
        database: String,
        #[arg(long, value_enum)]
        format: ExportFormat,
    },
    /// QA 層 (project / feature / item / run の表) の操作
    #[command(subcommand)]
    Qa(QaCommand),
    /// Web 画面と HTTP API のサーバーを起動する
    Serve,
    /// MCP サーバー (stdio) を起動する
    Mcp,
}

/// `tansu qa` のサブコマンド。
#[derive(Debug, Subcommand)]
enum QaCommand {
    /// QA.md 群を取り込む。4 つの表が無ければ作る。同じファイルを 2 回取り込んでも行は増えない
    Import {
        /// リポジトリのルート (配下の QA.md をすべて探す) か QA.md のファイル
        #[arg(required = true, value_name = "パス")]
        paths: Vec<PathBuf>,
        /// 取り込んだ件数と読み飛ばした行を JSON で出す
        #[arg(long)]
        json: bool,
        /// 書き込まずに、取り込んだ時と同じ件数と読み飛ばした行を出す
        #[arg(long)]
        dry_run: bool,
    },
}

/// `tansu db` のサブコマンド。
#[derive(Debug, Subcommand)]
enum DbCommand {
    /// データベースを作る (同じ名前が既にあれば何もしない)。定義を JSON で出す
    Create {
        /// データベースの名前
        name: String,
    },
    /// プロパティを足す・unique にする・外す (add → unique → remove の順)。指定が無ければ今の定義を出す。変えた後の定義を JSON で出す
    Schema {
        /// データベースの名前
        name: String,
        /// `<プロパティ名>:<型>[:<選択肢,...>|:<相手の表>]`。型は text / number / checkbox / url / date / select / multi_select / relation
        #[arg(long, value_name = "名前:型[:...]")]
        add: Vec<String>,
        /// 同じ値の行を 2 つ持てなくするプロパティの名前
        #[arg(long, value_name = "プロパティ名")]
        unique: Vec<String>,
        /// 外すプロパティの名前 (行の値からも消す)
        #[arg(long, value_name = "プロパティ名")]
        remove: Vec<String>,
    },
    /// すべてのデータベースの定義を JSON の配列で出す
    List,
}

/// `export` の書き出しの形。
#[derive(Debug, Clone, Copy, ValueEnum)]
enum ExportFormat {
    /// 見出しと表 (行の本文は表の後ろ)
    Markdown,
    /// 定義と行の JSON
    Json,
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
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Box::<dyn std::error::Error>::from)
        .and_then(|runtime| runtime.block_on(run(cli.command)));
    match result {
        Ok(output) => match output.map_or(Ok(()), |output| {
            writeln!(std::io::stdout().lock(), "{output}")
        }) {
            // 読み手 (`| head` 等) が先に閉じたのは、要る分を読み終えた合図なので成功にする
            // (`println!` は書けないと panic して exit 101 になる)
            Ok(()) => ExitCode::SUCCESS,
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("tansu: 結果を出力できませんでした: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("tansu: {}", error.to_string().replace('\n', " "));
            ExitCode::FAILURE
        }
    }
}

/// サブコマンドを実行し、stdout に出す文字列を返す (出すものが無ければ `None`)。
async fn run(command: Command) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if matches!(command, Command::Serve | Command::Mcp) {
        return Err(format!(
            "{} は未実装です (進捗は documents/DIRECTION.md「必要な機能」)",
            command.name()
        )
        .into());
    }
    let connection = db::open(&Settings::from_env()?).await?;
    let output = match command {
        Command::Db(DbCommand::Create { name }) => {
            to_json(&schema::create_database(&connection, &name).await?)?
        }
        Command::Db(DbCommand::Schema {
            name,
            add,
            unique,
            remove,
        }) => to_json(&schema::change_schema(&connection, &name, &add, &unique, &remove).await?)?,
        Command::Db(DbCommand::List) => to_json(&schema::list_databases(&connection).await?)?,
        Command::Query {
            database,
            json,
            filters,
            sort,
            limit,
        } => {
            if !json {
                return Err(
                    "query は --json を付けて実行してください (出力は JSON だけです)".into(),
                );
            }
            let (database, rows) =
                rows::query(&connection, &database, &filters, sort.as_deref(), limit).await?;
            to_json(
                &rows
                    .iter()
                    .map(|row| named_json::row_to_json(&database, row))
                    .collect::<Vec<_>>(),
            )?
        }
        Command::Upsert { database, json } => {
            let json = match json {
                Some(json) => json,
                None => {
                    let mut stdin = String::new();
                    std::io::stdin().read_to_string(&mut stdin)?;
                    stdin
                }
            };
            let (database, row) =
                rows::upsert(&connection, &database, &serde_json::from_str(&json)?).await?;
            to_json(&named_json::row_to_json(&database, &row))?
        }
        Command::Delete { database, row_id } => {
            rows::delete(&connection, &database, &row_id).await?;
            return Ok(None);
        }
        Command::Export { database, format } => {
            let (database, rows) = rows::query(&connection, &database, &[], None, None).await?;
            match format {
                ExportFormat::Markdown => export::to_markdown(&database, &rows),
                ExportFormat::Json => to_json(&export::to_json(&database, &rows))?,
            }
        }
        Command::Qa(QaCommand::Import {
            paths,
            json,
            dry_run,
        }) => {
            let report = qa_import::import(&connection, &paths, dry_run).await?;
            if json {
                to_json(&report)?
            } else {
                import_summary(&report)
            }
        }
        Command::Serve | Command::Mcp => unreachable!("serve / mcp は DB を開く前に返している"),
    };
    Ok(Some(output))
}

/// `qa import` の `--json` 無しの出力。件数を 1 行と、読み飛ばした行を 1 行ずつ。
fn import_summary(report: &ImportReport) -> String {
    let mut summary = format!(
        "{}project: {}, feature: {}, item: {}, run: {} ({} ファイル)",
        if report.dry_run { "[dry-run] " } else { "" },
        report.counts.project,
        report.counts.feature,
        report.counts.item,
        report.counts.run,
        report.files.len()
    );
    if !report.skipped.is_empty() {
        summary.push_str(&format!("\n読み飛ばした行: {} 行", report.skipped.len()));
        for skipped in &report.skipped {
            summary.push_str(&format!(
                "\n  {}:{}: {} ({})",
                skipped.file, skipped.line, skipped.content, skipped.reason
            ));
        }
    }
    summary
}

/// stdout に出す整形した JSON。
fn to_json(value: &impl serde::Serialize) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(value)
}

impl Command {
    /// コマンドラインに打つサブコマンド名。
    fn name(&self) -> &'static str {
        match self {
            Command::Db(_) => "db",
            Command::Query { .. } => "query",
            Command::Upsert { .. } => "upsert",
            Command::Delete { .. } => "delete",
            Command::Export { .. } => "export",
            Command::Qa(_) => "qa",
            Command::Serve => "serve",
            Command::Mcp => "mcp",
        }
    }
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
        let Command::Query {
            filters,
            sort,
            limit,
            ..
        } = Cli::try_parse_from([
            "tansu",
            "query",
            "tasks",
            "--json",
            "--where",
            "status=done",
            "--where",
            "title^=W",
            "--sort",
            "points:desc",
            "--limit",
            "2",
        ])
        .unwrap()
        .command
        else {
            panic!("query として解釈されていない");
        };
        assert_eq!(filters, ["status=done", "title^=W"]);
        assert_eq!(sort.as_deref(), Some("points:desc"));
        assert_eq!(limit, Some(2));
    }

    #[test]
    fn qa_import_takes_paths_and_flags() {
        let Command::Qa(QaCommand::Import {
            paths,
            json,
            dry_run,
        }) = Cli::try_parse_from(["tansu", "qa", "import", "--json", "--dry-run", "a", "b/QA.md"])
            .unwrap()
            .command
        else {
            panic!("qa import として解釈されていない");
        };
        assert_eq!(paths, [PathBuf::from("a"), PathBuf::from("b/QA.md")]);
        assert!(json);
        assert!(dry_run);
        assert!(Cli::try_parse_from(["tansu", "qa", "import"]).is_err());
    }

    #[test]
    fn row_id_may_start_with_hyphen() {
        let Command::Delete { row_id, .. } =
            Cli::try_parse_from(["tansu", "delete", "tasks", "-abc"])
                .unwrap()
                .command
        else {
            panic!("delete として解釈されていない");
        };
        assert_eq!(row_id, "-abc");
    }
}
