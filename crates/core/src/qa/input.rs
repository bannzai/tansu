//! Web 画面の QA の入力ビューが読む範囲。1 つの feature の item と各 item の最後の run、project の checkout の HEAD
//! (`documents/PROJECT.md`「Web 画面」)。入力した結果は `qa record` と同じ [`super::record::record`] で書く。

use libsql::Connection;
use serde::Serialize;

use super::{load_repo, resolve_commit, row_name};
use crate::db::finish;
use crate::error::Error;
use crate::named_json;

/// QA の入力ビューが 1 つの feature を入力する時に読む内容。
#[derive(Debug, Serialize)]
pub struct InputSession {
    /// project の `owner/repo`。
    pub repo: String,
    /// feature 名。
    pub feature: String,
    /// project の checkout の HEAD の full SHA (入力した run の `commit` になる)。checkout のパスが無いか、
    /// git で解決できなければ `None` (その時は記録できない)。
    pub head_commit: Option<String>,
    /// feature の item (取り込んだ順) と、その最後の run。
    pub items: Vec<InputItem>,
}

/// 入力ビューの 1 項目。
#[derive(Debug, Serialize)]
pub struct InputItem {
    /// item の行 ([`named_json::row_to_json`] の形)。
    pub item: serde_json::Value,
    /// item の最後の run の行 ([`named_json::row_to_json`] の形)。run が無ければ `None`。
    /// 「最後」は `qa verified` と同じ規則 (`documents/cli-contract.md`)。
    pub latest_run: Option<serde_json::Value>,
}

/// project `repo` の feature `feature_name` の入力ビューの内容を読む。DB には書かない。
/// project か feature が無ければ [`Error::InvalidInput`]、QA 層の表が無ければ [`Error::DatabaseNotFound`]。
pub async fn session(
    connection: &Connection,
    repo: &str,
    feature_name: &str,
) -> Result<InputSession, Error> {
    // 4 つの表を同じスナップショットで読む (読むだけなので書き込みロックは取らない)
    let transaction = connection.transaction().await?;
    let result = load_repo(&transaction, repo).await;
    let tables = finish(transaction, result).await?;
    let feature = tables
        .features
        .iter()
        .find(|feature| row_name(&tables.feature_database, feature) == feature_name)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "project `{repo}` に feature `{feature_name}` はありません"
            ))
        })?;
    Ok(InputSession {
        repo: repo.to_owned(),
        feature: feature_name.to_owned(),
        head_commit: tables
            .project_path()
            .ok()
            .and_then(|checkout| resolve_commit(checkout, "HEAD")),
        items: tables
            .items_of(feature)
            .into_iter()
            .map(|item| InputItem {
                item: named_json::row_to_json(&tables.item_database, item),
                latest_run: tables
                    .latest(tables.runs_of(item))
                    .map(|run| named_json::row_to_json(&tables.run_database, run)),
            })
            .collect(),
    })
}
