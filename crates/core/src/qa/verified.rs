//! `tansu qa verified`: commit を基準に、再テストが要る feature と未検証の item を返す
//! (`documents/PROJECT.md`「QA.md の廃止後も残す性質」の 1)。castle の release-app / auto-merge-pr のゲートが呼ぶ。
//! 判定の規則と JSON の形は `documents/cli-contract.md`。

use std::path::Path;

use libsql::Connection;
use serde::Serialize;

use super::{
    RepoTables, files_without_qa_md, full_sha, git, load_repo, resolve_commit, row_name,
    select_name_of, text_of,
};
use crate::db::finish;
use crate::error::Error;
use crate::model::{Id, Row};

/// run の `result` のうち、その commit で feature を確かめたとみなす結果。NG / あとで は確かめ終えていない。
const VERIFIED_RESULTS: [&str; 2] = ["ok", "skip"];

/// 2 つの commit の間で変わったファイルのパスを出す `git diff` の引数 (後ろに commit と pathspec を足す)。
/// - `--relative`: checkout が git のルートの下のディレクトリでも、パスを feature の `path` と同じ checkout 基準にする
/// - `--no-renames`: リネームを削除と追加に分け、移動元の feature のディレクトリも変更に数える
///   (リネームとして検出されると `--name-only` は移動先しか出さない)
/// - `-z`: 日本語などを含むパスをエスケープせずに NUL 区切りで出す ([`files_without_qa_md`])
const DIFF_NAMES: &[&str] = &["diff", "--name-only", "--relative", "--no-renames", "-z"];

/// `qa verified` の結果 (`--json` の出力)。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VerifiedReport {
    /// 指定した project の `owner/repo`。
    pub repo: String,
    /// 指定した commit (full SHA)。
    pub commit: String,
    /// 対象のすべての feature で `changed_files` と `unverified_items` が空なら true。
    pub ok: bool,
    /// 対象の feature (追加した順)。
    pub features: Vec<FeatureVerification>,
}

/// 1 つの feature の判定。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeatureVerification {
    /// feature 名。
    pub feature: String,
    /// この feature の item の run のうち、ok / skip で commit を持つ最後の run の commit。無ければ null。
    pub last_verified_commit: Option<String>,
    /// `last_verified_commit` から指定した commit までに変わった、feature のディレクトリの下のファイル
    /// (project の checkout からの相対パス。QA.md を除く)。
    pub changed_files: Vec<String>,
    /// run が 1 つも無い item。
    pub unverified_items: Vec<UnverifiedItem>,
}

/// run が 1 つも無い item。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnverifiedItem {
    /// item の行 ID (`qa record --item` に渡せる)。
    pub id: Id,
    /// 項目名。
    pub name: String,
}

/// project `repo` の feature ごとに、`commit` の時点で再テストが要るかを判定する。DB には書かない。
///
/// `diff_base` を渡すと、`git diff --name-only --no-renames <diff_base> <commit>` で feature のディレクトリに変更がある feature だけを
/// 判定の対象にする (対象外の feature は結果に含めず、`ok` にも数えない)。
pub async fn verified(
    connection: &Connection,
    repo: &str,
    commit: &str,
    diff_base: Option<&str>,
) -> Result<VerifiedReport, Error> {
    let commit = full_sha(commit)?;
    // 4 つの表を同じスナップショットで読む (読むだけなので書き込みロックは取らない)
    let transaction = connection.transaction().await?;
    let result = load_repo(&transaction, repo).await;
    let tables = finish(transaction, result).await?;
    let checkout = tables.project_path()?;
    if resolve_commit(checkout, &commit).as_deref() != Some(commit.as_str()) {
        return Err(Error::InvalidInput(format!(
            "commit `{commit}` は {} のリポジトリにありません (fetch してから実行してください)",
            checkout.display()
        )));
    }
    let base_changes = match diff_base {
        Some(diff_base) => {
            let base = resolve_commit(checkout, diff_base).ok_or_else(|| {
                Error::InvalidInput(format!(
                    "--diff-base `{diff_base}` は {} のリポジトリの commit として解決できません",
                    checkout.display()
                ))
            })?;
            Some(files_without_qa_md(&git(
                checkout,
                &[DIFF_NAMES, &[base.as_str(), commit.as_str()]].concat(),
            )?))
        }
        None => None,
    };

    let mut features = Vec::new();
    for feature in &tables.features {
        let directory = feature_path(&tables, feature)?;
        if let Some(base_changes) = &base_changes
            && !base_changes
                .iter()
                .any(|file| !directory.is_empty() && file.starts_with(&format!("{directory}/")))
        {
            continue;
        }
        let items = tables.items_of(feature);
        let last_verified_commit = tables
            .latest(
                items
                    .iter()
                    .flat_map(|item| tables.runs_of(item))
                    .filter(|(_, run)| {
                        text_of(&tables.run_database, run, "commit").is_some()
                            && select_name_of(&tables.run_database, run, "result")
                                .is_some_and(|result| VERIFIED_RESULTS.contains(&result.as_str()))
                    }),
            )
            .and_then(|run| text_of(&tables.run_database, run, "commit"))
            .map(str::to_owned);
        features.push(FeatureVerification {
            feature: row_name(&tables.feature_database, feature),
            changed_files: if items.is_empty() {
                // 項目の無い feature には確かめるものが無い (再テストを求めると、記録できないまま判定が通らなくなる)
                Vec::new()
            } else {
                changed_files(
                    checkout,
                    directory,
                    last_verified_commit.as_deref(),
                    &commit,
                )?
            },
            unverified_items: items
                .iter()
                .filter(|item| tables.runs_of(item).next().is_none())
                .map(|item| UnverifiedItem {
                    id: item.id.clone(),
                    name: row_name(&tables.item_database, item),
                })
                .collect(),
            last_verified_commit,
        });
    }
    Ok(VerifiedReport {
        repo: repo.to_owned(),
        commit,
        ok: features
            .iter()
            .all(|feature| feature.changed_files.is_empty() && feature.unverified_items.is_empty()),
        features,
    })
}

/// feature の `path` (QA.md のディレクトリ。ルート直下は空文字)。値が無ければ [`Error::InvalidInput`]
/// (`path` を持たない版の `qa import` で取り込んだ行)。
fn feature_path<'a>(tables: &'a RepoTables, feature: &'a Row) -> Result<&'a str, Error> {
    text_of(&tables.feature_database, feature, "path").ok_or_else(|| {
        Error::InvalidInput(format!(
            "feature `{}` にディレクトリ (path) がありません。qa import をやり直してください",
            row_name(&tables.feature_database, feature)
        ))
    })
}

/// feature のディレクトリ `directory` の下で、`last_verified_commit` から `commit` までに変わったファイル (QA.md を除く)。
/// ルート直下の feature (`directory` が空) は diff しない (どのファイルの変更もその feature の変更にしてしまうため)。
/// `last_verified_commit` が無い・full SHA でない・リポジトリで解決できない (rebase で消えた等) 時は、確かめた時点が分からないので
/// `commit` の時点のディレクトリの全ファイルを返す。
fn changed_files(
    checkout: &Path,
    directory: &str,
    last_verified_commit: Option<&str>,
    commit: &str,
) -> Result<Vec<String>, Error> {
    if directory.is_empty() {
        return Ok(Vec::new());
    }
    // ls-tree は既定でカレントディレクトリ (checkout) 基準のパスを出すので、diff の `--relative` と揃う
    // full SHA (大文字も含む) だけを確かめた時点として使う。短縮 SHA は後で別の commit を指し得るため
    // 解決できないものと同じに扱う (castle の run-qa と同じ)
    let output = match last_verified_commit
        .and_then(|last| full_sha(last).ok())
        .filter(|last| resolve_commit(checkout, last).as_deref() == Some(last.as_str()))
    {
        Some(last) => git(
            checkout,
            &[DIFF_NAMES, &[last.as_str(), commit, "--", directory]].concat(),
        )?,
        None => git(
            checkout,
            &[
                "ls-tree",
                "-r",
                "-z",
                "--name-only",
                commit,
                "--",
                directory,
            ],
        )?,
    };
    Ok(files_without_qa_md(&output))
}
