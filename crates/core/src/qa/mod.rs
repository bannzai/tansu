//! QA 層。汎用の表 (`db`) の上に、project → feature → item → run の 4 つのデータベースを載せる
//! (`documents/PROJECT.md`「QA 層」、`documents/data-model.md`「QA 層」)。SQL は書かず、`db` の関数を呼ぶ。

pub mod import;
pub mod qa_md;

use libsql::Connection;

use crate::db::schema;
use crate::error::Error;

/// データベース `project` の名前 (リポジトリ)。
pub const PROJECT: &str = "project";
/// データベース `feature` の名前 (QA.md 1 ファイル)。
pub const FEATURE: &str = "feature";
/// データベース `item` の名前 (チェック項目)。
pub const ITEM: &str = "item";
/// データベース `run` の名前 (実行結果の記録)。
pub const RUN: &str = "run";

/// QA 層の 4 つのデータベースの定義。(名前, `db schema --add` の指定, `--unique` にするプロパティ)。
/// relation の相手が先に無いと作れないため、この順で作る。automation は union のプロパティが汎用層に無いので
/// select `automation` + text `automation_detail` の 2 列に平たくする (`documents/data-model.md`「QA 層」)。
const DATABASES: &[(&str, &[&str], &[&str])] = &[
    (PROJECT, &["repo:text", "path:text"], &["repo"]),
    (
        FEATURE,
        &["project:relation:project", "name:text", "verification:text"],
        &[],
    ),
    (
        ITEM,
        &[
            "feature:relation:feature",
            "name:text",
            "expected:text",
            "automation:select:auto,manual,todo",
            "automation_detail:text",
            "spec_ids:multi_select",
            "labels:multi_select",
        ],
        &[],
    ),
    (
        RUN,
        &[
            "item:relation:item",
            "result:select:ok,ng,later,skip",
            "note:text",
            "recorded_at:date",
            "source:select:record,import",
            "image_url:url",
            "env:text",
            "commit:text",
            "run_id:text",
        ],
        &["run_id"],
    ),
];

/// QA 層の 4 つのデータベースとプロパティを作る。既にあれば何もしない (何度呼んでも同じ状態になる)。
/// `connection` は書き込みロックを取ったトランザクション。
pub(crate) async fn ensure_schema(connection: &Connection) -> Result<(), Error> {
    for (name, add, unique) in DATABASES {
        schema::create_database_in(connection, name).await?;
        let add: Vec<String> = add.iter().map(|spec| (*spec).to_owned()).collect();
        let unique: Vec<String> = unique.iter().map(|name| (*name).to_owned()).collect();
        schema::change_schema_in(connection, name, &add, &unique, &[]).await?;
    }
    Ok(())
}
