//! QA 層。汎用の表 (`db`) の上に、project → feature → item → run の 4 つのデータベースを載せる
//! (`documents/PROJECT.md`「QA 層」、`documents/data-model.md`「QA 層」)。SQL は書かず、`db` の関数を呼ぶ。
//! CLI の引数・JSON の形・exit code の契約は `documents/cli-contract.md`。

pub mod import;
pub mod pr_summary;
pub mod qa_md;
pub mod record;
pub mod verified;

use std::path::Path;
use std::process::Command;

use libsql::Connection;

use crate::db::{rows, schema};
use crate::error::Error;
use crate::model::{Database, Id, Row, Value};
use crate::named_json;

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
        &[
            "project:relation:project",
            "name:text",
            "verification:text",
            "path:text",
        ],
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
/// 前の版で作った表に足りないプロパティ (feature の `path` 等) もここで足す。
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

/// データベース `database_name` で `filters` に一致する最初の行の ID。無ければ `None`。
pub(crate) async fn find_row_id(
    connection: &Connection,
    database_name: &str,
    filters: &[String],
) -> Result<Option<Id>, Error> {
    let (_, rows) = rows::query_in(connection, database_name, filters, None, Some(1)).await?;
    Ok(rows.into_iter().next().map(|row| row.id))
}

/// 1 つの project (リポジトリ) の行と、QA 層の残り 3 つの表の定義と行。`qa verified` / `qa export` が読む範囲。
pub(crate) struct RepoTables {
    /// データベース `project` の定義。
    pub project_database: Database,
    /// 指定した `repo` の project の行。
    pub project: Row,
    /// データベース `feature` の定義。
    pub feature_database: Database,
    /// この project の feature の行 (追加した順)。
    pub features: Vec<Row>,
    /// データベース `item` の定義。
    pub item_database: Database,
    /// すべての project の item の行 (追加した順。feature の relation で絞って使う)。
    pub items: Vec<Row>,
    /// データベース `run` の定義。
    pub run_database: Database,
    /// すべての project の run の行 (追加した順。item の relation で絞って使う)。
    pub runs: Vec<Row>,
}

impl RepoTables {
    /// project の checkout の絶対パス (`qa import` が書いた `path`)。
    pub fn project_path(&self) -> Result<&Path, Error> {
        text_of(&self.project_database, &self.project, "path")
            .map(Path::new)
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "project `{}` に checkout のパス (path) がありません。qa import をやり直してください",
                    self.project.id.0
                ))
            })
    }

    /// feature `feature` の item の行 (追加した順)。
    pub fn items_of(&self, feature: &Row) -> Vec<&Row> {
        self.items
            .iter()
            .filter(|item| relation_of(&self.item_database, item, "feature").contains(&feature.id))
            .collect()
    }

    /// item `item` の run の行と、その追加した順の番号 ([`RepoTables::runs`] の位置)。
    pub fn runs_of<'a>(&'a self, item: &'a Row) -> impl Iterator<Item = (usize, &'a Row)> + 'a {
        self.runs
            .iter()
            .enumerate()
            .filter(move |(_, run)| relation_of(&self.run_database, run, "item").contains(&item.id))
    }

    /// `runs` のうち最後に記録された run。`recorded_at` が後のもの ([`recorded_instant`] の順)、同じ値
    /// (`recorded_at` は秒の精度) なら後に追加されたもの。`recorded_at` が無い run はどの値よりも前とみなす。
    pub fn latest<'a>(&self, runs: impl Iterator<Item = (usize, &'a Row)>) -> Option<&'a Row> {
        runs.max_by_key(|(index, run)| {
            (
                recorded_instant(text_of(&self.run_database, run, "recorded_at")),
                *index,
            )
        })
        .map(|(_, run)| run)
    }
}

/// `repo` の project と、feature / item / run の表を読む。`connection` は読み取りのトランザクション。
/// project が無ければ [`Error::InvalidInput`]。
pub(crate) async fn load_repo(connection: &Connection, repo: &str) -> Result<RepoTables, Error> {
    let (project_database, projects) = rows::query_in(
        connection,
        PROJECT,
        &[format!("repo={repo}")],
        None,
        Some(1),
    )
    .await?;
    let project = projects.into_iter().next().ok_or_else(|| {
        Error::InvalidInput(format!(
            "project `{repo}` はありません (qa import で取り込んだリポジトリの owner/repo を指定してください)"
        ))
    })?;
    let (feature_database, features) = rows::query_in(
        connection,
        FEATURE,
        &[format!("project={}", project.id.0)],
        None,
        None,
    )
    .await?;
    let (item_database, items) = rows::query_in(connection, ITEM, &[], None, None).await?;
    let (run_database, runs) = rows::query_in(connection, RUN, &[], None, None).await?;
    Ok(RepoTables {
        project_database,
        project,
        feature_database,
        features,
        item_database,
        items,
        run_database,
        runs,
    })
}

/// 行 `row` のプロパティ `property_name` の値 (名前は定義 `database` で ID に引く)。プロパティか値が無ければ `None`。
fn property_value<'a>(database: &Database, row: &'a Row, property_name: &str) -> Option<&'a Value> {
    row.values.get(&database.property(property_name)?.id)
}

/// text / url / date のプロパティ `property_name` の値。無ければ `None`。
pub(crate) fn text_of<'a>(
    database: &Database,
    row: &'a Row,
    property_name: &str,
) -> Option<&'a str> {
    match property_value(database, row, property_name)? {
        Value::Text(text) | Value::Url(text) | Value::Date(text) => Some(text),
        _ => None,
    }
}

/// relation のプロパティ `property_name` が指す行の ID。無ければ空。
pub(crate) fn relation_of<'a>(database: &Database, row: &'a Row, property_name: &str) -> &'a [Id] {
    match property_value(database, row, property_name) {
        Some(Value::Relation(row_ids)) => row_ids,
        _ => &[],
    }
}

/// feature / item の行の `name`。無ければ行 ID (`upsert` で名前を付けずに足した行も、どの行かを示せるように)。
pub(crate) fn row_name(database: &Database, row: &Row) -> String {
    text_of(database, row, "name")
        .unwrap_or(&row.id.0)
        .to_owned()
}

/// select のプロパティ `property_name` で選んだ選択肢の name。無ければ `None`。
pub(crate) fn select_name_of(
    database: &Database,
    row: &Row,
    property_name: &str,
) -> Option<String> {
    let property = database.property(property_name)?;
    named_json::value_to_json(&property.kind, row.values.get(&property.id)?)
        .as_str()
        .map(str::to_owned)
}

/// 使われている中で最も東の時差 (UTC+14。キリバスのライン諸島)。日付だけの `recorded_at` を、その日付が始まる
/// 最も早い時刻に置くために使う。
const EARLIEST_UTC_OFFSET_SECONDS: i32 = 14 * 60 * 60;

/// run の `recorded_at` を比べるための UTC の時刻。時刻付きの値 (`qa record` の run) はその時刻。
/// 日付だけの値 (`qa import` の run。QA.md の確認日で時差が分からない) は、どの時差で書かれた同じ日付の時刻よりも
/// 前になるよう、その日付が UTC+14 で始まる時刻に置く (同じ日の朝に記録した run を、取り込みの run より前と
/// 取り違えないため)。無い・読めない値は `None` (どの値よりも前)。
fn recorded_instant(recorded_at: Option<&str>) -> Option<chrono::DateTime<chrono::Utc>> {
    let recorded_at = recorded_at?;
    if let Ok(date) = chrono::NaiveDate::parse_from_str(recorded_at, "%Y-%m-%d") {
        return date
            .and_hms_opt(0, 0, 0)?
            .and_local_timezone(chrono::FixedOffset::east_opt(EARLIEST_UTC_OFFSET_SECONDS)?)
            .single()
            .map(|date_time| date_time.with_timezone(&chrono::Utc));
    }
    chrono::DateTime::parse_from_rfc3339(recorded_at)
        .ok()
        .map(|date_time| date_time.with_timezone(&chrono::Utc))
}

/// SHA-1 の commit の full SHA の桁数。
const SHA1_HEX_LENGTH: usize = 40;
/// SHA-256 のリポジトリ (`git init --object-format=sha256`) の commit の full SHA の桁数。
const SHA256_HEX_LENGTH: usize = 64;

/// `commit` が full SHA (40 桁か 64 桁の 16 進) なら小文字にして返す。そうでなければ [`Error::InvalidInput`]。
/// 短縮 SHA や `HEAD` のような ref を受け付けないのは、記録した後にリポジトリが進むと別の commit を指し得るため
/// (castle の run-qa も記録が full SHA でない時は検証済みとみなさない)。
pub(crate) fn full_sha(commit: &str) -> Result<String, Error> {
    if matches!(commit.len(), SHA1_HEX_LENGTH | SHA256_HEX_LENGTH)
        && commit
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Ok(commit.to_ascii_lowercase());
    }
    Err(Error::InvalidInput(format!(
        "commit `{commit}` は full SHA (40 桁か 64 桁の 16 進) ではありません (`git rev-parse HEAD` の値を渡してください)"
    )))
}

/// `directory` で `git <args>` を実行し、stdout を返す。git を起動できない時と失敗した時は [`Error::Git`]。
pub(crate) fn git(directory: &Path, args: &[&str]) -> Result<String, Error> {
    // `--literal-pathspecs`: feature のディレクトリ名に `*` `[` などがあっても、glob ではなくそのままのパスとして扱う
    let output = Command::new("git")
        .arg("--literal-pathspecs")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .map_err(|error| Error::Git(format!("git {}: {error}", args.join(" "))))?;
    if !output.status.success() {
        return Err(Error::Git(format!(
            "git {} ({}): {}",
            args.join(" "),
            directory.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout).map_err(|error| {
        Error::Git(format!(
            "git {} の出力が UTF-8 ではありません: {error}",
            args.join(" ")
        ))
    })
}

/// `directory` のリポジトリで `rev` が指す commit の full SHA。解決できなければ `None`。
pub(crate) fn resolve_commit(directory: &Path, rev: &str) -> Option<String> {
    git(
        directory,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ],
    )
    .ok()
    .map(|sha| sha.trim().to_owned())
}

/// URL を markdown のリンク・画像の行き先 (`[..](ここ)`) に書く形にする。`)` などを含む URL でもリンクが途中で
/// 終わらないよう `<...>` で囲み、その中で使えない `<` `>` 改行と、`>` のエスケープと読まれる `\` を
/// パーセントエンコードする (CommonMark の link destination)。`qa import` が取り込んだ URL も通るため、
/// `qa record` の入力検査に頼らない。
pub(crate) fn markdown_link_destination(url: &str) -> String {
    format!(
        "<{}>",
        url.replace('\\', "%5C")
            .replace('<', "%3C")
            .replace('>', "%3E")
            .replace('\r', "%0D")
            .replace('\n', "%0A")
    )
}

/// `git <args> -z` の出力 (NUL 区切りのファイルのパスの一覧) から、QA.md を除いて返す。QA.md は記録と定義を持つ
/// ファイルで、その変更は feature の実装の変更ではないため (castle の run-qa の `affected_features.sh` も除いている)。
/// NUL 区切りで受けるのは、改行区切りの出力では git が日本語などを含むパスを引用符付きでエスケープする
/// (`core.quotePath` の既定) ため。
pub(crate) fn files_without_qa_md(output: &str) -> Vec<String> {
    output
        .split('\0')
        .filter(|path| !path.is_empty() && *path != "QA.md" && !path.ends_with("/QA.md"))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_sha_accepts_only_40_or_64_hex_digits() {
        let sha1 = "A1B2C3D4E5F60718293A4B5C6D7E8F90AB12CD34";
        assert_eq!(full_sha(sha1).unwrap(), sha1.to_ascii_lowercase());
        assert!(full_sha(&"a".repeat(64)).is_ok());
        for commit in ["a1b2c3d", "HEAD", &"g".repeat(40), &"a".repeat(41)] {
            assert!(full_sha(commit).is_err(), "{commit}");
        }
    }

    #[test]
    fn recorded_instant_orders_dates_and_times_across_offsets() {
        let day = recorded_instant(Some("2026-10-11"));
        // UTC では前日 (2026-10-10T23:00:00Z) だが、書かれた日付は同じ日なので日付だけの値より後
        let tokyo_morning = recorded_instant(Some("2026-10-11T08:00:00+09:00"));
        let tokyo = recorded_instant(Some("2026-10-11T09:00:00+09:00"));
        let utc = recorded_instant(Some("2026-10-11T01:00:00Z"));
        assert!(day < tokyo_morning);
        assert!(tokyo_morning < tokyo);
        assert!(tokyo < utc);
        assert!(utc < recorded_instant(Some("2026-10-12")));
        assert!(recorded_instant(None) < day);
        // 時刻付きどうしは、書かれた日付ではなく UTC の時刻で比べる (2026-10-11T15:30:00Z < 2026-10-11T23:00:00Z)
        assert!(
            recorded_instant(Some("2026-10-12T00:30:00+09:00"))
                < recorded_instant(Some("2026-10-11T23:00:00+00:00"))
        );
    }

    #[test]
    fn link_destination_keeps_the_link_closed() {
        assert_eq!(
            markdown_link_destination("https://e.com/a).png"),
            "<https://e.com/a).png>"
        );
        assert_eq!(
            markdown_link_destination("https://e.com/<a>\\\n"),
            "<https://e.com/%3Ca%3E%5C%0A>"
        );
    }

    #[test]
    fn qa_md_files_are_not_changes_of_the_feature() {
        assert_eq!(
            files_without_qa_md("features/a/QA.md\0features/a/画面.rs\0QA.md\0"),
            ["features/a/画面.rs"]
        );
    }
}
