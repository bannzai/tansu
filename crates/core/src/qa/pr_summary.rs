//! `tansu qa export --format pr-summary`: PR body に貼る「何をどう確認したか」の markdown
//! (`documents/PROJECT.md`「QA.md の廃止後も残す性質」の 2)。形は `documents/cli-contract.md`。

use std::collections::BTreeMap;

use libsql::Connection;

use super::{git, load_repo, resolve_commit, row_name, select_name_of, text_of};
use crate::db::finish;
use crate::error::Error;
use crate::export::escape_cell;

/// 表の commit の列に出す短縮 SHA の桁数。`git log --oneline` の既定の最短の桁数 (`core.abbrev` の既定の 7) に合わせた。
const SHORT_SHA_LENGTH: usize = 7;

/// `recorded_at` (`2026-10-11` か RFC 3339) の先頭の日付 `YYYY-MM-DD` の長さ。日時は記録した時差での日付になる。
const DATE_LENGTH: usize = "YYYY-MM-DD".len();

/// 値が無いセルに書く文字。空のセルより「記録が無い」ことが読み取りやすいため。
const EMPTY_CELL: &str = "—";

/// project `repo` の各 item の最後の run を、feature ごとの表にした markdown を返す。DB には書かない。
///
/// - 列は 項目・結果・確認日 (`recorded_at` の日付)・画像 (`image_url`)・commit (短縮 SHA)
/// - 「最後の run」は `recorded_at` の時刻が後のもの、同じ時刻なら後に追加されたもの
/// - `since` を渡すと、commit が `since` の子孫 (`since` 自身は含めない) の run だけを見る。commit が無い・リポジトリで
///   解決できない run は見ない
/// - run の無い item と、表に行の無い feature は出さない
pub async fn pr_summary(
    connection: &Connection,
    repo: &str,
    since: Option<&str>,
) -> Result<String, Error> {
    // 4 つの表を同じスナップショットで読む (読むだけなので書き込みロックは取らない)
    let transaction = connection.transaction().await?;
    let result = load_repo(&transaction, repo).await;
    let tables = finish(transaction, result).await?;
    let since = match since {
        Some(since) => {
            let checkout = tables.project_path()?;
            Some((
                checkout,
                resolve_commit(checkout, since).ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "--since `{since}` は {} のリポジトリの commit として解決できません",
                        checkout.display()
                    ))
                })?,
            ))
        }
        None => None,
    };
    // 同じ commit の run が多いので、子孫かどうかの判定 (git の実行) を commit ごとに 1 回にする
    let mut after_since: BTreeMap<String, bool> = BTreeMap::new();
    let mut is_after_since = |commit: Option<&str>| match (&since, commit) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some((checkout, since)), Some(commit)) => {
            *after_since.entry(commit.to_owned()).or_insert_with(|| {
                // `merge-base --is-ancestor` は祖先なら exit 0、そうでなければ exit 1、解決できない commit はエラーで、
                // どれも 0 以外は「子孫でない」として扱う
                commit != since.as_str()
                    && git(checkout, &["merge-base", "--is-ancestor", since.as_str(), commit]).is_ok()
            })
        }
    };

    let mut markdown = format!("## QA の記録 ({})\n", escape_cell(repo));
    let mut has_rows = false;
    for feature in &tables.features {
        let mut table_rows = Vec::new();
        for item in tables.items_of(feature) {
            let Some(run) = tables.latest(tables.runs_of(item).filter(|(_, run)| {
                is_after_since(text_of(&tables.run_database, run, "commit"))
            })) else {
                continue;
            };
            let result = select_name_of(&tables.run_database, run, "result");
            table_rows.push(format!(
                "| {} | {} | {} | {} | {} |",
                escape_cell(&row_name(&tables.item_database, item)),
                result.as_deref().map(result_label).unwrap_or(EMPTY_CELL),
                text_of(&tables.run_database, run, "recorded_at")
                    .and_then(|recorded_at| recorded_at.get(..DATE_LENGTH))
                    .unwrap_or(EMPTY_CELL),
                text_of(&tables.run_database, run, "image_url").map_or_else(
                    || EMPTY_CELL.to_owned(),
                    // 表の区切りの `|` と、リンクの終わりと読まれる `>` を URL の中で使わない形にする
                    |image_url| format!(
                        "[画像](<{}>)",
                        image_url.replace('|', "%7C").replace('>', "%3E")
                    )
                ),
                text_of(&tables.run_database, run, "commit").map_or_else(
                    || EMPTY_CELL.to_owned(),
                    |commit| format!("`{}`", commit.get(..SHORT_SHA_LENGTH).unwrap_or(commit))
                ),
            ));
        }
        if table_rows.is_empty() {
            continue;
        }
        has_rows = true;
        markdown.push_str(&format!(
            "\n### {}\n\n| 項目 | 結果 | 確認日 | 画像 | commit |\n| --- | --- | --- | --- | --- |\n{}\n",
            escape_cell(&row_name(&tables.feature_database, feature)),
            table_rows.join("\n")
        ));
    }
    if !has_rows {
        markdown.push_str("\n記録された run はありません\n");
    }
    Ok(markdown)
}

/// run の `result` の選択肢の name を、PR を読む人向けの表記にする。知らない選択肢はそのまま出す。
fn result_label(result: &str) -> &str {
    match result {
        "ok" => "OK",
        "ng" => "NG",
        "later" => "あとで",
        "skip" => "スキップ",
        other => other,
    }
}
