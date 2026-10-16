//! `tansu qa record`: QA を実行した結果を run に 1 行足す。実行のたびに増える (同じ項目を 2 回確認すれば 2 行)。
//! 再試行で同じ実行が二重に入らないよう、呼び出し側が `run_id` を渡した時は同じ `run_id` の run を 1 行にする
//! (`documents/PROJECT.md`「制約」の冪等)。

use libsql::Connection;
use serde_json::json;

use super::{
    FEATURE, IMPORT_RUN_ID_PREFIX, ITEM, PROJECT, RUN, ensure_schema, find_row_id, full_sha,
    markdown_link_destination,
};
use crate::db::{begin, finish, now, rows};
use crate::error::Error;
use crate::model::{Database, Id, Row};

/// `qa record` で記録する項目の指定。
#[derive(Debug, Clone)]
pub enum ItemSelector {
    /// item の行 ID (`--item`)。
    Id(String),
    /// project の `repo`・feature 名・項目名 (`--repo` / `--feature` / `--item-name`)。agent が ID を探さずに済むように。
    Name {
        /// project の `owner/repo`。
        repo: String,
        /// feature 名。
        feature: String,
        /// 項目名。
        item_name: String,
    },
}

/// `qa record` の入力 (CLI の引数と同じ)。
#[derive(Debug, Clone)]
pub struct RecordRequest {
    /// 記録する項目。
    pub item: ItemSelector,
    /// 結果。run の `result` の選択肢 (ok / ng / later / skip)。大文字でもよい。
    pub result: String,
    /// 検証した commit の full SHA。
    pub commit: String,
    /// メモ。
    pub note: Option<String>,
    /// エビデンスの画像の URL。最初の 1 つを `image_url` に、すべてを run の本文に書く。
    pub image_urls: Vec<String>,
    /// 検証環境。
    pub env: Option<String>,
    /// 冪等キー。無ければ `record:<新しい ID>` にする (実行のたびに 1 行足す)。
    pub run_id: Option<String>,
}

/// `request` の run を 1 行書き、書いた run の行を定義とともに返す。QA 層の 4 つの表が無ければ作る。
///
/// run の値: `item` は指定した項目、`source` は `record`、`recorded_at` は今の時刻 (時差付き ISO 8601)、
/// `image_url` は最初の画像、本文は画像を 1 行 1 つの markdown の画像。同じ `run_id` の run があれば、その行を
/// `request` の内容で置き換える (渡さなかった値は消す)。
pub async fn record(
    connection: &Connection,
    request: &RecordRequest,
) -> Result<(Database, Row), Error> {
    let commit = full_sha(&request.commit)?;
    let run_id = match &request.run_id {
        None => format!("record:{}", Id::generate().0),
        Some(run_id) if run_id.is_empty() || run_id.starts_with(IMPORT_RUN_ID_PREFIX) => {
            return Err(Error::InvalidInput(format!(
                "--run-id `{run_id}` は使えません (空でなく、`{IMPORT_RUN_ID_PREFIX}` で始まらない ID を渡してください)"
            )));
        }
        Some(run_id) => run_id.clone(),
    };
    if let Some(image_url) = request.image_urls.iter().find(|url| !is_single_token(url)) {
        return Err(Error::InvalidInput(format!(
            "--image-url `{}` は使えません (空白・改行・制御文字を含まない URL を渡してください)",
            image_url.escape_debug()
        )));
    }
    let transaction = begin(connection).await?;
    let result = async {
        ensure_schema(&transaction).await?;
        let item_id = item_id(&transaction, &request.item).await?;
        rows::upsert_in(
            &transaction,
            RUN,
            &json!({
                "body": request
                    .image_urls
                    .iter()
                    .map(|image_url| format!("![]({})\n", markdown_link_destination(image_url)))
                    .collect::<String>(),
                "values": {
                    "run_id": run_id,
                    "item": [item_id],
                    "result": request.result.to_ascii_lowercase(),
                    "note": request.note,
                    "recorded_at": now(),
                    "source": "record",
                    "image_url": request.image_urls.first(),
                    "env": request.env,
                    "commit": commit,
                },
            }),
        )
        .await
    }
    .await;
    finish(transaction, result).await
}

/// `text` が空でなく、空白・改行・制御文字を含まないか。画像の URL は本文の markdown の画像と
/// `qa export --format pr-summary` の表のセルに入るため、markdown を崩す文字を含む値を受け付けない。
fn is_single_token(text: &str) -> bool {
    !text.is_empty()
        && !text
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
}

/// `selector` が指す item の行 ID。無ければ [`Error::InvalidInput`]。`connection` は書き込みのトランザクション。
async fn item_id(connection: &Connection, selector: &ItemSelector) -> Result<Id, Error> {
    match selector {
        ItemSelector::Id(item_id) => {
            let (_, items) = rows::query_in(connection, ITEM, &[], None, None).await?;
            items
                .into_iter()
                .find(|item| item.id.0 == *item_id)
                .map(|item| item.id)
                .ok_or_else(|| Error::InvalidInput(format!("item `{item_id}` はありません")))
        }
        ItemSelector::Name {
            repo,
            feature,
            item_name,
        } => {
            let project_id = find_row_id(connection, PROJECT, &[format!("repo={repo}")])
                .await?
                .ok_or_else(|| Error::InvalidInput(format!("project `{repo}` はありません")))?;
            let feature_id = find_row_id(
                connection,
                FEATURE,
                &[
                    format!("project={}", project_id.0),
                    format!("name={feature}"),
                ],
            )
            .await?
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "project `{repo}` に feature `{feature}` はありません"
                ))
            })?;
            // 同じ名前の項目は `qa import` が別の item として残すため、名前で 1 つに決まらない時は黙って選ばない
            let (_, items) = rows::query_in(
                connection,
                ITEM,
                &[
                    format!("feature={}", feature_id.0),
                    format!("name={item_name}"),
                ],
                None,
                Some(2),
            )
            .await?;
            match <[Row; 1]>::try_from(items) {
                Ok([item]) => Ok(item.id),
                Err(items) if items.is_empty() => Err(Error::InvalidInput(format!(
                    "project `{repo}` の feature `{feature}` に項目 `{item_name}` はありません"
                ))),
                Err(_) => Err(Error::InvalidInput(format!(
                    "project `{repo}` の feature `{feature}` に項目 `{item_name}` が 2 つ以上あります (--item に item の行 ID を渡してください)"
                ))),
            }
        }
    }
}
