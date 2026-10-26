//! `tansu mcp` の MCP サーバー (stdio)。CLI のサブコマンドと 1 対 1 の tool を公開する入口で、処理は `tansu-core` を呼ぶだけにする。
//! tool の入力は CLI の引数と同じ名前 (snake_case) の JSON、出力は CLI の `--json` と同じ形の文字列 (`documents/cli-contract.md`)。
//! 失敗は CLI の stderr の 1 行と同じ理由を本文にした `isError: true` の結果で返す。

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use tansu_core::config::Settings;
use tansu_core::db::{self, rows, schema};
use tansu_core::qa::import as qa_import;
use tansu_core::qa::record::{self as qa_record, ItemSelector, RecordRequest};
use tansu_core::qa::{self, pr_summary, verified as qa_verified};
use tansu_core::{Error, export, named_json, usage};

/// stdio で MCP サーバーを動かし、クライアントが接続を閉じるまで待つ。stdout はプロトコルが使うため、ほかの出力は stderr に出す。
pub async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    TansuServer
        .serve(rmcp::transport::stdio())
        .await?
        .waiting()
        .await?;
    Ok(())
}

/// tansu の MCP サーバー。状態を持たず、tool の呼び出しのたびに `TANSU_HOME` の設定を読んで DB を開く
/// (CLI の 1 回の実行と同じ。起動時の環境変数の誤りでサーバーごと落とさず、tool の失敗として理由を返すため)。
#[derive(Debug, Clone)]
struct TansuServer;

#[tool_handler(name = "tansu")]
impl rmcp::ServerHandler for TansuServer {}

/// `db_create` の入力。
#[derive(Debug, Deserialize, JsonSchema)]
struct DbCreateParams {
    /// データベースの名前
    name: String,
}

/// `db_schema` の入力。
#[derive(Debug, Deserialize, JsonSchema)]
struct DbSchemaParams {
    /// データベースの名前
    name: String,
    /// 足すプロパティ。`<プロパティ名>:<型>[:<選択肢,...>|:<相手の表>]`。型は text / number / checkbox / url / date / select / multi_select / relation
    #[serde(default)]
    add: Vec<String>,
    /// 同じ値の行を 2 つ持てなくするプロパティの名前
    #[serde(default)]
    unique: Vec<String>,
    /// 外すプロパティの名前 (行の値からも消す)
    #[serde(default)]
    remove: Vec<String>,
}

/// `query` の入力。
#[derive(Debug, Deserialize, JsonSchema)]
struct QueryParams {
    /// データベースの名前
    database: String,
    /// 絞り込み。`<プロパティ名>=<値>` (等価。multi_select / relation は含む) か `<プロパティ名>^=<値>` (前方一致。text / url / date)。すべてに一致する行を返す
    #[serde(default, rename = "where")]
    filters: Vec<String>,
    /// 並び。`<プロパティ名>[:asc|:desc]` (text / number / checkbox / url / date)。無ければ追加した順
    sort: Option<String>,
    /// 返す行の上限
    limit: Option<u64>,
}

/// `upsert` の入力。
#[derive(Debug, Deserialize, JsonSchema)]
struct UpsertParams {
    /// データベースの名前
    database: String,
    /// 行の JSON (`{"id": ..., "body": ..., "values": {<プロパティ名>: <値>}}`)。JSON の文字列で渡してもよい
    json: serde_json::Value,
}

/// `delete` の入力。
#[derive(Debug, Deserialize, JsonSchema)]
struct DeleteParams {
    /// データベースの名前
    database: String,
    /// 行の ID
    row_id: String,
}

/// `export` の書き出しの形。
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ExportFormat {
    /// 見出しと表 (行の本文は表の後ろ)
    Markdown,
    /// 定義と行の JSON
    Json,
}

/// `export` の入力。
#[derive(Debug, Deserialize, JsonSchema)]
struct ExportParams {
    /// データベースの名前
    database: String,
    /// 書き出しの形
    format: ExportFormat,
}

/// `qa_import` の入力。
#[derive(Debug, Deserialize, JsonSchema)]
struct QaImportParams {
    /// リポジトリのルート (配下の QA.md をすべて探す) か QA.md のファイルのパス
    paths: Vec<PathBuf>,
    /// true なら書き込まずに、取り込んだ時と同じ結果 (件数・行の扱いの内訳・本文に残した行・どこにも入らなかった行) を返す
    #[serde(default)]
    dry_run: bool,
}

/// `qa_record` の入力。項目は `item` か、`repo` / `feature` / `item_name` の組のどちらかで指定する。
#[derive(Debug, Deserialize, JsonSchema)]
struct QaRecordParams {
    /// 記録する item の行 ID
    item: Option<String>,
    /// 記録する項目の project の owner/repo
    repo: Option<String>,
    /// 記録する項目の feature 名
    feature: Option<String>,
    /// 記録する項目の項目名
    item_name: Option<String>,
    /// 結果 (ok / ng / later / skip)
    result: String,
    /// 検証した commit の full SHA
    commit: String,
    /// メモ
    note: Option<String>,
    /// エビデンスの画像の URL
    #[serde(default)]
    image_url: Vec<String>,
    /// 検証環境
    env: Option<String>,
    /// 冪等キー。再試行で同じ実行を二重に記録しないために渡す
    run_id: Option<String>,
}

/// `qa_verified` の入力。
#[derive(Debug, Deserialize, JsonSchema)]
struct QaVerifiedParams {
    /// project の owner/repo
    repo: String,
    /// 判定の基準にする commit の full SHA
    commit: String,
    /// この commit から `commit` までに feature のディレクトリが変わった feature だけを判定する (PR の merge-base を渡す)
    diff_base: Option<String>,
}

/// `qa_export` の書き出しの形。
#[derive(Debug, Deserialize, JsonSchema)]
enum QaExportFormat {
    /// PR body に貼る、feature ごとの項目・結果・確認日・画像・commit の表
    #[serde(rename = "pr-summary")]
    PrSummary,
}

/// `qa_export` の入力。
#[derive(Debug, Deserialize, JsonSchema)]
struct QaExportParams {
    /// project の owner/repo
    repo: String,
    /// 書き出しの形
    format: QaExportFormat,
    /// この commit の子孫 (この commit 自身は含めない) で記録した run だけを出す
    since: Option<String>,
}

#[tool_router]
impl TansuServer {
    /// すべてのデータベースの定義を JSON の配列で返す
    #[tool]
    async fn db_list(&self) -> Result<String, String> {
        respond("db_list", async {
            Ok(to_json(&schema::list_databases(&open().await?).await?)?)
        })
        .await
    }

    /// データベースを作る (同じ名前が既にあれば何もしない)。定義を JSON で返す
    #[tool]
    async fn db_create(
        &self,
        Parameters(params): Parameters<DbCreateParams>,
    ) -> Result<String, String> {
        respond("db_create", async {
            Ok(to_json(
                &schema::create_database(&open().await?, &params.name).await?,
            )?)
        })
        .await
    }

    /// プロパティを足す・unique にする・外す (add → unique → remove の順)。指定が無ければ今の定義を返す。変えた後の定義を JSON で返す
    #[tool]
    async fn db_schema(
        &self,
        Parameters(params): Parameters<DbSchemaParams>,
    ) -> Result<String, String> {
        respond("db_schema", async {
            Ok(to_json(
                &schema::change_schema(
                    &open().await?,
                    &params.name,
                    &params.add,
                    &params.unique,
                    &params.remove,
                )
                .await?,
            )?)
        })
        .await
    }

    /// 行を絞り込んで JSON の配列で返す。`where` はすべてに一致する行を返す。`sort` が無ければ追加した順
    #[tool]
    async fn query(&self, Parameters(params): Parameters<QueryParams>) -> Result<String, String> {
        respond("query", async {
            let (database, rows) = rows::query(
                &open().await?,
                &params.database,
                &params.filters,
                params.sort.as_deref(),
                params.limit,
            )
            .await?;
            Ok(to_json(
                &rows
                    .iter()
                    .map(|row| named_json::row_to_json(&database, row))
                    .collect::<Vec<_>>(),
            )?)
        })
        .await
    }

    /// 行を書く。`id` があればその行、無ければ unique のプロパティの値が一致する行を更新し、無ければ足す。書いた行を JSON で返す
    #[tool]
    async fn upsert(&self, Parameters(params): Parameters<UpsertParams>) -> Result<String, String> {
        respond("upsert", async {
            let (database, row) = rows::upsert(
                &open().await?,
                &params.database,
                &match params.json {
                    serde_json::Value::String(json) => serde_json::from_str(&json)?,
                    json => json,
                },
            )
            .await?;
            Ok(to_json(&named_json::row_to_json(&database, &row))?)
        })
        .await
    }

    /// 行を消す。relation でこの行を参照している値からも外す。item の行は、その項目だけを持つ run も消す。行が無ければ何もしない。成功すると空の結果を返す
    #[tool]
    async fn delete(&self, Parameters(params): Parameters<DeleteParams>) -> Result<(), String> {
        respond("delete", async {
            qa::delete(&open().await?, &params.database, &params.row_id).await
        })
        .await
    }

    /// データベースの定義と全行を書き出す (markdown か JSON)
    #[tool]
    async fn export(&self, Parameters(params): Parameters<ExportParams>) -> Result<String, String> {
        respond("export", async {
            let (database, rows) =
                rows::query(&open().await?, &params.database, &[], None, None).await?;
            Ok(match params.format {
                ExportFormat::Markdown => export::to_markdown(&database, &rows),
                ExportFormat::Json => to_json(&export::to_json(&database, &rows))?,
            })
        })
        .await
    }

    /// QA.md 群を QA 層の 4 つの表 (project / feature / item / run) に取り込み、件数・行の扱いの内訳・本文に残した行・どこにも入らなかった行を JSON で返す。同じファイルを 2 回取り込んでも行は増えない
    #[tool]
    async fn qa_import(
        &self,
        Parameters(params): Parameters<QaImportParams>,
    ) -> Result<String, String> {
        respond("qa_import", async {
            // dry-run は DB を開かない (開くとファイルの作成とマイグレーションで書き込みが起きる)
            Ok(to_json(&if params.dry_run {
                qa_import::dry_run(&params.paths)?
            } else {
                qa_import::import(&open().await?, &params.paths).await?
            })?)
        })
        .await
    }

    /// QA の実行結果を run に 1 行足し、書いた run の行を JSON で返す (実行のたびに増える。同じ run_id の run は 1 行にする)
    #[tool]
    async fn qa_record(
        &self,
        Parameters(params): Parameters<QaRecordParams>,
    ) -> Result<String, String> {
        respond("qa_record", async {
            let item = match (params.item, params.repo, params.feature, params.item_name) {
                (Some(item), None, None, None) => ItemSelector::Id(item),
                (None, Some(repo), Some(feature), Some(item_name)) => ItemSelector::Name {
                    repo,
                    feature,
                    item_name,
                },
                _ => {
                    return Err(Error::InvalidInput(
                        "item か、repo / feature / item_name の組のどちらか一方を渡してください"
                            .into(),
                    ));
                }
            };
            let (database, row) = qa_record::record(
                &open().await?,
                &RecordRequest {
                    item,
                    result: params.result,
                    commit: params.commit,
                    note: params.note,
                    image_urls: params.image_url,
                    env: params.env,
                    run_id: params.run_id,
                },
            )
            .await?;
            Ok(to_json(&named_json::row_to_json(&database, &row))?)
        })
        .await
    }

    /// commit を基準に、再テストが要る feature と未検証の item を JSON で返す。再テストが要る時も失敗にはせず、`ok` が false の結果を返す
    #[tool]
    async fn qa_verified(
        &self,
        Parameters(params): Parameters<QaVerifiedParams>,
    ) -> Result<String, String> {
        respond("qa_verified", async {
            Ok(to_json(
                &qa_verified::verified(
                    &open().await?,
                    &params.repo,
                    &params.commit,
                    params.diff_base.as_deref(),
                )
                .await?,
            )?)
        })
        .await
    }

    /// QA の記録を PR body に貼る markdown で書き出す
    #[tool]
    async fn qa_export(
        &self,
        Parameters(params): Parameters<QaExportParams>,
    ) -> Result<String, String> {
        respond("qa_export", async {
            let QaExportFormat::PrSummary = params.format;
            Ok(
                pr_summary::pr_summary(&open().await?, &params.repo, params.since.as_deref())
                    .await?
                    .trim_end()
                    .to_owned(),
            )
        })
        .await
    }
}

/// tool 1 回分を実行する。利用記録に `mcp_<tool>` を 1 行足してから `operation` を待ち、失敗は理由の文字列にする。
/// 利用記録を書けなくても本来の操作は止めず、警告を stderr に出す (CLI と同じ。設定を読めない時は同じエラーを操作が返すので警告を重ねない)。
async fn respond<T>(
    tool: &str,
    operation: impl Future<Output = Result<T, Error>>,
) -> Result<T, String> {
    if let Ok(settings) = Settings::from_env()
        && let Err(error) = usage::record(&settings.home, &format!("mcp_{tool}"))
    {
        eprintln!("tansu: 警告: 利用記録を書けませんでした: {error}");
    }
    operation.await.map_err(|error| error.to_string())
}

/// `TANSU_HOME` / `TANSU_DB_URL` の設定で DB を開く。
async fn open() -> Result<libsql::Connection, Error> {
    db::open(&Settings::from_env()?).await
}

/// 結果の整形した JSON (CLI の出力と同じ)。
fn to_json(value: &impl serde::Serialize) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(value)
}
