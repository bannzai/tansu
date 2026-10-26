//! `tansu qa import`: QA.md 群を QA 層の 4 つのデータベースに取り込む。
//! 同じファイルを 2 回取り込んでも行が増えない (project は `repo`、feature は project + 名前、item は feature + 名前 +
//! 同じ名前の中での順番、run は `run_id` を自然キーにして upsert する。`documents/PROJECT.md`「制約」の冪等)。

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use libsql::Connection;
use serde::Serialize;
use serde_json::json;

use super::qa_md::{self, QaMd, QaMdItem, QaMdRecordKind, SkippedLine};
use super::{
    FEATURE, IMPORT_RUN_ID_PREFIX, ITEM, PROJECT, RUN, ensure_schema, find_row_id,
    recorded_instant, text_of,
};
use crate::db::{begin, finish, rows, schema};
use crate::error::Error;
use crate::model::{Id, Row};

/// 取り込んだ行の数 (データベースごと)。同じ QA.md の 2 回目でも、`--dry-run` でも同じ数になる
/// (入力に含まれる project / feature / item / run の数で、新しく足した行の数ではない)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportCounts {
    /// 取り込んだリポジトリの数。
    pub project: usize,
    /// 取り込んだ QA.md (feature) の数。
    pub feature: usize,
    /// 取り込んだチェック項目の数。
    pub item: usize,
    /// 取り込んだ記録 (チェック済み・⏭️・❌ の項目の、エビデンスの確認ごと) の数。
    pub run: usize,
}

/// 読んだ QA.md の行の扱いの内訳 (全ファイルの合計)。どちらにも入らなかった行は [`ImportReport::unaccounted`]。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportLines {
    /// 構造として読んだ行 (値に入れた行と、空行・`<details>`・`---` のような書式だけの行) の数。
    pub structured: usize,
    /// 構造として読めず、feature・item・run の本文に残した行の数 ([`ImportReport::skipped`] の数)。
    pub body: usize,
}

/// `qa import` の結果 (`--json` の出力)。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportReport {
    /// `--dry-run` で、DB に書いていないなら true。
    pub dry_run: bool,
    /// 読んだ QA.md の絶対パス。
    pub files: Vec<String>,
    /// 取り込んだ行の数。
    pub counts: ImportCounts,
    /// 読んだ QA.md の行の扱いの内訳。
    pub lines: ImportLines,
    /// 構造として読めず、本文に残した行。
    pub skipped: Vec<SkippedLine>,
    /// 構造としても本文としても取り込まなかった行。空でなければ、QA.md を消すと失われる情報がある。
    pub unaccounted: Vec<SkippedLine>,
}

/// 読んだ QA.md。取り込みの単位 (ルート, project の `repo`, ファイル, feature 名, ファイルの内容, パース結果)。
type ParsedFile = (PathBuf, String, PathBuf, String, String, QaMd);

/// `paths` の QA.md を読むだけで、DB を開かず何も書かない (`--dry-run`)。結果は [`import`] と同じ。
pub fn dry_run(paths: &[PathBuf]) -> Result<ImportReport, Error> {
    Ok(report(&parse_all(paths)?, true))
}

/// `paths` の QA.md を取り込む。パスはリポジトリのルート (配下の `QA.md` を探す。探す範囲は `documents/cli-contract.md` の
/// `qa import`) か QA.md のファイル。
///
/// - project: ルートの `git remote` の `owner/repo` (取れなければディレクトリ名。[`repo_name`])。`path` はルートの絶対パス。
///   本文はルート直下の QA.md の案内 ([`qa_md::root_guide`])。ルート直下の QA.md を取り込まない時は本文を変えない
/// - feature: frontmatter の `feature` (無ければ、ルート直下の QA.md は `_root`、それ以外は親ディレクトリの名前)。
///   `path` は QA.md のディレクトリ ([`feature_directory`])、本文は [`feature_body`]
/// - item: 項目名・セクション・期待動作・自動化・仕様 ID・本文。`labels` は QA.md に無いため触らない
/// - run: チェック済み (`ok`)・⏭️ (`skip`)・❌ (`ng`) の項目に、エビデンスの確認ごとに 1 行 (確認が無ければ 1 行)。
///   規則は [`write_runs`]
///
/// 4 つのデータベースが無ければ作る。全部を 1 つのトランザクションで書き、途中で失敗したら何も書かない。
pub async fn import(connection: &Connection, paths: &[PathBuf]) -> Result<ImportReport, Error> {
    let parsed = parse_all(paths)?;
    let transaction = begin(connection).await?;
    let result = write_all(&transaction, &parsed).await;
    finish(transaction, result).await?;
    Ok(report(&parsed, false))
}

/// `paths` のすべての QA.md を見つけて読む。パスが読めなければエラー。
fn parse_all(paths: &[PathBuf]) -> Result<Vec<ParsedFile>, Error> {
    let mut files_by_root: BTreeMap<PathBuf, BTreeSet<PathBuf>> = BTreeMap::new();
    for path in paths {
        let metadata = std::fs::metadata(path).map_err(|error| {
            Error::InvalidInput(format!("{} を読めません: {error}", path.display()))
        })?;
        if metadata.is_dir() {
            let root = path.canonicalize()?;
            let files = match find_qa_md_in_git(&root) {
                Some(files) => files,
                None => {
                    let mut files = Vec::new();
                    find_qa_md(&root, &mut files)?;
                    files
                }
            };
            files_by_root.entry(root).or_default().extend(files);
        } else {
            let file = path.canonicalize()?;
            let parent = file.parent().unwrap_or(&file).to_path_buf();
            let root = git_toplevel(&parent).unwrap_or(parent);
            files_by_root.entry(root).or_default().insert(file);
        }
    }

    let mut parsed: Vec<ParsedFile> = Vec::new();
    // 同じ project の同じ feature 名を持つファイルは、後のファイルの項目が先のファイルの項目を黙って上書きしないよう、
    // 2 つ目以降をファイルごと取り込まず、全行をどこにも入らなかった行として報告する (feature は project + 名前で行を決めるため)
    let mut first_file_by_feature: BTreeMap<(String, String), PathBuf> = BTreeMap::new();
    for (root, files) in &files_by_root {
        let repo = repo_name(root);
        for file in files {
            let text = std::fs::read_to_string(file).map_err(|error| {
                Error::InvalidInput(format!("{} を読めません: {error}", file.display()))
            })?;
            let file_name = file.display().to_string();
            let mut qa_md = qa_md::parse(&file_name, &text);
            let feature = qa_md
                .feature
                .clone()
                .unwrap_or_else(|| default_feature_name(root, file));
            match first_file_by_feature.entry((repo.clone(), feature.clone())) {
                Entry::Vacant(entry) => {
                    entry.insert(file.clone());
                }
                Entry::Occupied(entry) => {
                    let reason = format!(
                        "同じ project の feature `{feature}` が既に {} にある (このファイルは取り込まない)",
                        entry.get().display()
                    );
                    qa_md = QaMd {
                        unaccounted: text
                            .lines()
                            .enumerate()
                            .map(|(index, line)| SkippedLine {
                                file: file_name.clone(),
                                line: index + 1,
                                content: line.trim_end_matches('\r').to_owned(),
                                reason: reason.clone(),
                            })
                            .collect(),
                        ..QaMd::default()
                    };
                }
            }
            parsed.push((
                root.clone(),
                repo.clone(),
                file.clone(),
                feature,
                text,
                qa_md,
            ));
        }
    }
    Ok(parsed)
}

/// 読んだ QA.md から、取り込みの件数 (入力に含まれる project / feature / item / run の数)・行の扱いの内訳・
/// 本文に残した行・どこにも入らなかった行をまとめる。
fn report(parsed: &[ParsedFile], dry_run: bool) -> ImportReport {
    let mut projects = BTreeSet::new();
    let mut features = BTreeSet::new();
    let mut items = BTreeSet::new();
    let mut runs = BTreeSet::new();
    // 書き込みと同じ自然キー (project は repo、feature は repo + 名前、item は feature + 名前 + 同じ名前の中での順番、
    // run は item + 確認の順番) で数える。同じ origin の worktree を 2 つ渡しても DB には 1 組しか入らないため、
    // root のパスでは数えない
    for (_, repo, _, feature, _, qa_md) in parsed {
        projects.insert(repo);
        features.insert((repo, feature));
        for (item, occurrence) in qa_md.items.iter().zip(occurrences(&qa_md.items)) {
            items.insert((repo, feature, &item.name, occurrence));
            for run_number in 1..=run_count(item) {
                runs.insert((repo, feature, &item.name, occurrence, run_number));
            }
        }
    }
    let skipped: Vec<SkippedLine> = parsed
        .iter()
        .flat_map(|(_, _, _, _, _, qa_md)| qa_md.skipped.iter().cloned())
        .collect();
    ImportReport {
        dry_run,
        files: parsed
            .iter()
            .map(|(_, _, file, _, _, _)| file.display().to_string())
            .collect(),
        counts: ImportCounts {
            project: projects.len(),
            feature: features.len(),
            item: items.len(),
            run: runs.len(),
        },
        lines: ImportLines {
            structured: parsed
                .iter()
                .map(|(_, _, _, _, _, qa_md)| qa_md.structured_lines)
                .sum(),
            body: skipped.len(),
        },
        skipped,
        unaccounted: parsed
            .iter()
            .flat_map(|(_, _, _, _, _, qa_md)| qa_md.unaccounted.iter().cloned())
            .collect(),
    }
}

/// 各項目が、同じ名前の項目の中で何番目か (0 始まり)。`items` と同じ順。
fn occurrences(items: &[QaMdItem]) -> Vec<usize> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    items
        .iter()
        .map(|item| {
            let count = counts.entry(item.name.as_str()).or_default();
            *count += 1;
            *count - 1
        })
        .collect()
}

/// 項目から作る run の数。記録 (チェック済み・⏭️・❌) が無ければ 0、あればエビデンスの確認の数 (確認が無ければ 1)。
fn run_count(item: &QaMdItem) -> usize {
    match run_result(item) {
        Some(_) => item.confirmations.len().max(1),
        None => 0,
    }
}

/// [`import`] の書き込み。`connection` は書き込みロックを取ったトランザクション。
async fn write_all(connection: &Connection, parsed: &[ParsedFile]) -> Result<(), Error> {
    ensure_schema(connection).await?;
    // multi_select の値は定義にある選択肢しか書けないため、仕様 ID を選択肢として先に足す (無いものだけが足される)
    let spec_ids: BTreeSet<&str> = parsed
        .iter()
        .flat_map(|(_, _, _, _, _, qa_md)| qa_md.spec_rows.iter().map(|row| row.id.as_str()))
        .collect();
    if !spec_ids.is_empty() {
        let spec = format!(
            "spec_ids:multi_select:{}",
            spec_ids.into_iter().collect::<Vec<_>>().join(",")
        );
        schema::change_schema_in(connection, ITEM, &[spec], &[], &[]).await?;
    }

    // project の本文は、その repo のルート直下の QA.md の案内。project を書くのは repo の最初のファイルの時で、
    // ルート直下の QA.md が最初のファイルとは限らないため先に決める。取り込まない 2 つ目以降のファイルからは作らない
    let mut guides: BTreeMap<&str, String> = BTreeMap::new();
    let mut first_features: BTreeSet<(&str, &str)> = BTreeSet::new();
    for (root, repo, file, feature, text, _) in parsed {
        if first_features.insert((repo.as_str(), feature.as_str()))
            && file.parent() == Some(root.as_path())
        {
            guides
                .entry(repo.as_str())
                .or_insert_with(|| qa_md::root_guide(text));
        }
    }

    let mut project_ids: BTreeMap<&str, Id> = BTreeMap::new();
    // 同じ project の同じ feature 名の 2 つ目以降のファイルは `parse_all` がどこにも入らなかった行として報告済み。
    // 書くと空の値で 1 つ目のファイルの feature (verification) を消すため、書かない
    let mut written_features: BTreeSet<(&str, &str)> = BTreeSet::new();
    for (root, repo, file, feature, text, qa_md) in parsed {
        if !written_features.insert((repo.as_str(), feature.as_str())) {
            continue;
        }
        let project_id = match project_ids.get(repo.as_str()) {
            Some(project_id) => project_id.clone(),
            None => {
                let mut project =
                    json!({"values": {"repo": repo, "path": root.display().to_string()}});
                // ルート直下の QA.md を取り込まない時は、案内の元が無いので本文を書かない (既にある本文を保つ)
                if let Some(guide) = guides.get(repo.as_str()) {
                    project["body"] = json!(guide);
                }
                let (_, row) = rows::upsert_in(connection, PROJECT, &project).await?;
                project_ids.insert(repo, row.id.clone());
                row.id
            }
        };
        let feature_id = find_row_id(
            connection,
            FEATURE,
            &[
                format!("project={}", project_id.0),
                format!("name={feature}"),
            ],
        )
        .await?
        .unwrap_or_else(Id::generate);
        rows::upsert_in(
            connection,
            FEATURE,
            &json!({
                "id": feature_id,
                "body": feature_body(&qa_md.body, text),
                "values": {
                    "project": [project_id],
                    "name": feature,
                    "verification": qa_md.verification,
                    "path": feature_directory(root, file),
                    "last_verified_commit": qa_md.last_verified_commit,
                    "last_verified_at": qa_md.last_verified_at,
                },
            }),
        )
        .await?;
        // 同じ名前の項目は名前で 1 行に決まらないため、名前と期待動作が一致する行を先に当て、無ければ名前が一致する行
        // (追加した順) のうちまだ当てていないものに当てる。順番だけで当てると、前の項目を QA.md から消した時に
        // その行 (と `qa record` の run) が後ろの項目に移るため
        let (item_database, existing_items) = rows::query_in(
            connection,
            ITEM,
            &[format!("feature={}", feature_id.0)],
            None,
            None,
        )
        .await?;
        // 1 周目で全項目の名前と期待動作の一致を当ててから、2 周目で残りを名前だけで当てる (先に名前だけで当てると、
        // 後ろの項目の一致する行を前の項目が取ってしまうため)
        let mut claimed_item_ids: BTreeSet<&Id> = BTreeSet::new();
        let mut item_ids: Vec<Option<Id>> = vec![None; qa_md.items.len()];
        for match_expected in [true, false] {
            for (item, item_id) in qa_md.items.iter().zip(item_ids.iter_mut()) {
                if item_id.is_some() {
                    continue;
                }
                let matched = existing_items.iter().find(|row| {
                    let same_expected =
                        text_of(&item_database, row, "expected") == Some(item.expected.as_str());
                    !claimed_item_ids.contains(&row.id)
                        && text_of(&item_database, row, "name") == Some(item.name.as_str())
                        && (same_expected || !match_expected)
                });
                if let Some(row) = matched {
                    claimed_item_ids.insert(&row.id);
                    *item_id = Some(row.id.clone());
                }
            }
        }
        for (item, item_id) in qa_md.items.iter().zip(item_ids) {
            let item_id = item_id.unwrap_or_else(Id::generate);
            let has_runs = run_count(item) > 0;
            rows::upsert_in(
                connection,
                ITEM,
                &json!({
                    "id": item_id,
                    "body": item_body(item, !has_runs),
                    "values": {
                        "feature": [feature_id],
                        "name": item.name,
                        "section": item.section,
                        "expected": item.expected,
                        "automation": item.automation.map(|automation| automation.name()),
                        "automation_detail": item.automation_detail,
                        "spec_ids": item.spec_ids,
                    },
                }),
            )
            .await?;
            write_runs(connection, qa_md, item, &item_id).await?;
        }
    }
    Ok(())
}

/// 項目 `item` (行 ID `item_id`) の run を書く。`qa_md` は項目があるファイルの読んだ結果 (frontmatter の値を使う)。
///
/// - 記録 (チェック済み・⏭️・❌) のある項目に、エビデンスの確認ごとに 1 行 (確認が無ければ 1 行)。`run_id` は
///   `import:<item の行 ID>:<確認の順番 (1 始まり)>`、`source` は `import`、本文・`image_url` は確認の本文と最初の画像
/// - `recorded_at` は確認日 (無ければ frontmatter の `last_verified_at`、それも無ければ値なし)
/// - 項目の結果 (`ok` / `skip` / `ng`)・⏭️ / ❌ の本文 (`note`)・frontmatter の `last_verified_commit` (`commit`) は、
///   [`super::RepoTables::latest`] と同じ順 (`recorded_at`、同じなら行を追加した順) で最後になる確認の run にだけ書く。
///   QA.md は項目の今の結果と最後の確認の commit しか持たず、それより前の確認の結果と commit は分からないため
/// - 既にある取り込みの run の行は、確認の内容 (確認日と本文) で確認に当て、行 ID と取り込みが書かない値 (`env` 等) を保つ。
///   前の版の `import:<item の行 ID>` の run は、日付が最後の確認の run に移す
/// - その項目の取り込みの run (`run_id` が `import:` で始まる) のうち、どの確認にも当たらなかったもの (QA.md で記録・確認が
///   消えたもの) は消す。`qa record` と Web 画面の run は消さない
async fn write_runs(
    connection: &Connection,
    qa_md: &QaMd,
    item: &QaMdItem,
    item_id: &Id,
) -> Result<(), Error> {
    let recorded_at: Vec<Option<&str>> = if item.confirmations.is_empty() {
        vec![qa_md.last_verified_at.as_deref()]
    } else {
        item.confirmations
            .iter()
            .map(|confirmation| {
                confirmation
                    .date
                    .as_deref()
                    .or(qa_md.last_verified_at.as_deref())
            })
            .collect()
    };
    let result = run_result(item);
    let run_ids: Vec<String> = (1..=run_count(item))
        .map(|run_number| format!("{IMPORT_RUN_ID_PREFIX}{}:{run_number}", item_id.0))
        .collect();

    let (run_database, existing_runs) = rows::query_in(
        connection,
        RUN,
        &[format!("item={}", item_id.0)],
        None,
        None,
    )
    .await?;
    let bodies: Vec<&str> = (0..run_ids.len())
        .map(|index| {
            item.confirmations
                .get(index)
                .map_or("", |confirmation| confirmation.body.as_str())
        })
        .collect();
    let item_run_prefix = format!("{IMPORT_RUN_ID_PREFIX}{}:", item_id.0);
    // 確認ごとに、既にある取り込みの run の行を当てる。行 ID と、取り込みが書かない値 (`env` 等) と、ほかの行からの
    // relation を保つため、確認の順番ではなく内容で当てる (前の確認を QA.md から消すと順番がずれるため):
    // 1. 確認日・本文・`run_id` が同じ行 2. 確認日と本文が同じ行 3. 同じ `run_id` の行 (確認の本文を書き換えた時)
    // 4. 前の版の `import:<item の行 ID>` (1 項目 1 run) の行を、日付が最後の確認に
    let mut row_for_index: Vec<Option<&Row>> = vec![None; run_ids.len()];
    let mut claimed: BTreeSet<&Id> = BTreeSet::new();
    for (match_content, match_run_id) in [(true, true), (true, false), (false, true)] {
        for index in 0..run_ids.len() {
            if row_for_index[index].is_some() {
                continue;
            }
            let matched = existing_runs.iter().find(|run| {
                let run_id = text_of(&run_database, run, "run_id").unwrap_or_default();
                let same_content = text_of(&run_database, run, "recorded_at") == recorded_at[index]
                    && run.body == bodies[index];
                run_id.starts_with(&item_run_prefix)
                    && (same_content || !match_content)
                    && (run_id == run_ids[index] || !match_run_id)
                    && !claimed.contains(&run.id)
            });
            if let Some(run) = matched {
                claimed.insert(&run.id);
                row_for_index[index] = Some(run);
            }
        }
    }
    let legacy_run_id = format!("{IMPORT_RUN_ID_PREFIX}{}", item_id.0);
    let legacy_index = recorded_at
        .iter()
        .enumerate()
        .max_by_key(|(index, recorded_at)| (recorded_instant(**recorded_at), *index))
        .map(|(index, _)| index)
        .filter(|index| *index < run_ids.len() && row_for_index[*index].is_none());
    let legacy_run = existing_runs
        .iter()
        .find(|run| text_of(&run_database, run, "run_id") == Some(legacy_run_id.as_str()));
    if let (Some(index), Some(run)) = (legacy_index, legacy_run) {
        claimed.insert(&run.id);
        row_for_index[index] = Some(run);
    }
    // 当てなかった取り込みの run (QA.md で記録・確認が消えたもの) は消す。`qa record` と Web 画面の run は消さない
    for run in &existing_runs {
        if !claimed.contains(&run.id)
            && text_of(&run_database, run, "run_id")
                .is_some_and(|run_id| run_id.starts_with(IMPORT_RUN_ID_PREFIX))
        {
            rows::delete_in(connection, RUN, &run.id.0).await?;
        }
    }
    // 当てた行の `run_id` を付け替える前に、付け替える行を一時的な `run_id` に退避する (確認の順番が入れ替わった時に、
    // 付け替えの途中で `run_id` の unique が重ならないように)。付け替えの無い行は書かない (2 回目の取り込みで行を変えないため)
    for (index, run) in row_for_index.iter().enumerate() {
        if let Some(run) = run
            && text_of(&run_database, run, "run_id") != Some(run_ids[index].as_str())
        {
            rows::upsert_in(
                connection,
                RUN,
                &json!({"id": run.id, "values": {"run_id": format!("{item_run_prefix}moving:{}", run.id.0)}}),
            )
            .await?;
        }
    }

    // 結果を書く run は、`RepoTables::latest` が選ぶものと同じにする: `recorded_at` が最後で、同じなら行の追加順
    // (rowid) が後のもの。既にある行はその位置、これから足す行は既存の行の後ろに確認の順で並ぶ
    let row_position = |index: usize| {
        row_for_index[index]
            .and_then(|row| existing_runs.iter().position(|run| run.id == row.id))
            .unwrap_or(existing_runs.len() + index)
    };
    let latest = (0..run_ids.len())
        .max_by_key(|index| (recorded_instant(recorded_at[*index]), row_position(*index)));
    for (index, run_id) in run_ids.iter().enumerate() {
        let confirmation = item.confirmations.get(index);
        let is_latest = Some(index) == latest;
        rows::upsert_in(
            connection,
            RUN,
            &json!({
                "id": row_for_index[index].map(|run| &run.id),
                "body": bodies[index],
                "values": {
                    "run_id": run_id,
                    "item": [item_id],
                    "result": result.filter(|_| is_latest),
                    "note": item.record.as_ref().map(|record| record.note.as_str()).filter(|note| is_latest && !note.is_empty()),
                    "recorded_at": recorded_at[index],
                    "source": "import",
                    "image_url": confirmation.and_then(|confirmation| confirmation.image_url.as_deref()),
                    "commit": qa_md.last_verified_commit.as_deref().filter(|_| is_latest),
                },
            }),
        )
        .await?;
    }
    Ok(())
}

/// feature の行の本文: QA.md の項目・エビデンスに当たらない本文 (`prose`。[`QaMd::body`]) の後ろに、`## QA.md の原文` の
/// 見出しと、ファイルの内容 `qa_md_text` をそのまま入れた fenced code block を置いたもの。原文はほかの規則で拾えない
/// 行があっても QA.md を復元できるようにする最後の受け皿で、`qa_md_text` は本文の部分文字列としてそのまま入る
/// (末尾に改行が無ければ、閉じのフェンスの前に改行を足す)。フェンスは原文のどのバッククォートの並びより長くする
/// (原文の中のフェンスで閉じないように)。
pub fn feature_body(prose: &str, qa_md_text: &str) -> String {
    let longest_backticks = qa_md_text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest_backticks.max(3) + 1);
    let mut body = String::new();
    if !prose.is_empty() {
        body.push_str(prose);
        body.push_str("\n\n");
    }
    body.push_str("## QA.md の原文\n\n");
    body.push_str(&fence);
    body.push_str("markdown\n");
    body.push_str(qa_md_text);
    if !qa_md_text.is_empty() && !qa_md_text.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(&fence);
    body
}

/// item の行の本文: 項目の本文 ([`QaMdItem::body`])。`with_confirmations` (記録が無く run を作らない項目) なら、
/// エビデンスの確認 (`**確認日: YYYY-MM-DD**` と確認の本文) を後ろに足す (run にしない確認も落とさないため)。
fn item_body(item: &QaMdItem, with_confirmations: bool) -> String {
    let mut sections: Vec<String> = Vec::new();
    if !item.body.is_empty() {
        sections.push(item.body.clone());
    }
    if with_confirmations {
        for confirmation in &item.confirmations {
            let parts: Vec<String> = [
                confirmation
                    .date
                    .as_deref()
                    .map(|date| format!("**確認日: {date}**"))
                    .unwrap_or_default(),
                confirmation.body.clone(),
            ]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect();
            if !parts.is_empty() {
                sections.push(parts.join("\n"));
            }
        }
    }
    sections.join("\n\n")
}

/// 項目の記録から run の `result` を決める。❌ は `ng`、⏭️ は `skip`、チェック済みは `ok`。記録が無ければ `None` (run を作らない)。
fn run_result(item: &QaMdItem) -> Option<&'static str> {
    match (&item.record, item.checked) {
        (Some(record), _) => Some(match record.kind {
            QaMdRecordKind::Fail => "ng",
            QaMdRecordKind::Skip => "skip",
        }),
        (None, true) => Some("ok"),
        (None, false) => None,
    }
}

/// `directory` が QA.md を探さない名前のディレクトリ (`.git` / `node_modules` / `target`) なら true。
/// `.git` は git の管理領域、`node_modules` / `target` は依存とビルドの生成物で、利用者が QA.md を書く場所ではないため。
fn is_excluded_directory(directory: &Path) -> bool {
    matches!(
        directory.file_name().and_then(|name| name.to_str()),
        Some(".git" | "node_modules" | "target")
    )
}

/// `root` を起点にリポジトリを探す `git -C <root>` のコマンド。呼び出し元から受け継いだ、リポジトリ・index の場所を
/// 決める環境変数 (git の hook の中で tansu を動かすと設定されている) を外し、`root` を含むリポジトリを使わせる。
fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(root);
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
    ] {
        command.env_remove(variable);
    }
    command
}

/// git の作業ツリーの中の `root` (絶対パス) の下の `QA.md`。git が追跡しているものと、未追跡で `.gitignore`
/// (`.git/info/exclude`・global の excludes を含む) で除外されていないものを返す。`root` より下で `.git`
/// (ファイルでもディレクトリでも) を持つディレクトリ (入れ子の worktree・サブモジュール・別のリポジトリ) の下、
/// [`is_excluded_directory`] の下、symlink とその下、作業ツリーに無いファイル (削除してまだ commit していない等) は含めない。
/// `root` が git の作業ツリーの外か、`root` 自体が `.gitignore` の対象 (`ls-files` が何も返さない) か、
/// git を実行できなければ `None`。
fn find_qa_md_in_git(root: &Path) -> Option<Vec<PathBuf>> {
    if git_command(root)
        .args(["check-ignore", "-q", "."])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        return None;
    }
    let output = git_command(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    Some(
        output
            .stdout
            .split(|byte| *byte == 0)
            .map(|relative| root.join(path_from_git_output(relative)))
            .filter(|file| file.file_name().is_some_and(|name| name == "QA.md"))
            .filter(|file| file.is_file() && !file.is_symlink())
            .filter(|file| {
                // git は `.git` が有効なリポジトリを指す時だけ、そのディレクトリを入れ子のリポジトリとして下を見ない。
                // `.git` の中身を問わず外すため、`.git` の有無を自分でも見る
                !file
                    .ancestors()
                    .skip(1)
                    .take_while(|directory| *directory != root)
                    .any(|directory| {
                        is_excluded_directory(directory)
                            || directory.is_symlink()
                            || std::fs::symlink_metadata(directory.join(".git")).is_ok()
                    })
            })
            .collect(),
    )
}

/// `git ls-files -z` が出す 1 件分のパスのバイト列。unix ではバイト列のまま使い、UTF-8 でない名前のパスも落とさない
/// (`read_dir` で探す時と同じファイルを読めるように)。
#[cfg(unix)]
fn path_from_git_output(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

/// `git ls-files -z` が出す 1 件分のパスのバイト列。unix 以外の git はパスを UTF-8 で出す。
#[cfg(not(unix))]
fn path_from_git_output(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// `directory` の下の `QA.md` を再帰的に探してパス順で `files` に足す。git の作業ツリーの外 (git を実行できない時を含む) で使う。
/// [`is_excluded_directory`] の下は見ず、symlink は辿らない (リポジトリの外へ出ないように)。
fn find_qa_md(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), Error> {
    let mut entries: Vec<_> = std::fs::read_dir(directory)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        if file_type.is_dir() {
            if !is_excluded_directory(&entry.path()) {
                find_qa_md(&entry.path(), files)?;
            }
        } else if file_type.is_file() && name == "QA.md" {
            files.push(entry.path());
        }
    }
    Ok(())
}

/// frontmatter に `feature` が無い時の feature 名。ルート直下の QA.md は root QA.md の `_root`、それ以外は
/// `<features-dir>/<feature>/QA.md` の配置 (castle の `qa-md-format.md`「配置」) に合わせて親ディレクトリの名前。
fn default_feature_name(root: &Path, file: &Path) -> String {
    let parent = file.parent().unwrap_or(root);
    if parent == root {
        return "_root".to_owned();
    }
    parent.file_name().map_or_else(
        || parent.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// feature の `path`: QA.md があるディレクトリの、ルートからの相対パス (区切りは `/`)。ルート直下の QA.md は空文字
/// (`qa verified` はルート直下の feature のディレクトリを diff しない)。
fn feature_directory(root: &Path, file: &Path) -> String {
    file.parent()
        .and_then(|parent| parent.strip_prefix(root).ok())
        .map(|relative| {
            relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        // `parse_all` はルートの下のファイルと、親ディレクトリか git のルートをルートにしたファイルだけを集めるため、
        // ルートの外のファイルは来ない
        .unwrap_or_default()
}

/// `directory` を含む git リポジトリのルート (`git rev-parse --show-toplevel`)。git の外なら `None`。
fn git_toplevel(directory: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(directory)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    PathBuf::from(String::from_utf8(output.stdout).ok()?.trim())
        .canonicalize()
        .ok()
}

/// project の `repo`。`root` が git リポジトリのルートそのもので origin の URL から `owner/repo` を読めればそれ、
/// そうでなければ `root` のディレクトリ名。ルートの下のディレクトリ (このリポジトリの `fixtures/qa/<プロジェクト>` 等) を
/// 囲むリポジトリの名前にしないため、ルートそのものの時だけ remote を見る。
pub fn repo_name(root: &Path) -> String {
    let from_remote = git_toplevel(root)
        .filter(|toplevel| root.canonicalize().is_ok_and(|root| root == *toplevel))
        .and_then(|_| {
            Command::new("git")
                .args(["-C"])
                .arg(root)
                .args(["remote", "get-url", "origin"])
                .output()
                .ok()
                .filter(|output| output.status.success())
        })
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|url| repo_from_remote_url(url.trim()));
    from_remote.unwrap_or_else(|| {
        root.file_name().map_or_else(
            || root.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    })
}

/// remote の URL (`https://github.com/owner/repo.git`、`git@github.com:owner/repo.git`、`ssh://git@github.com/owner/repo`) の
/// 末尾 2 つのパスの要素 `owner/repo`。読めなければ `None`。
fn repo_from_remote_url(url: &str) -> Option<String> {
    let path = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map(|(_, path)| path)?,
        None => url.split_once(':').map(|(_, path)| path)?,
    };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut segments = path.rsplit('/').filter(|segment| !segment.is_empty());
    let repo = segments.next()?;
    let owner = segments.next()?;
    Some(format!("{owner}/{repo}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_is_read_from_https_scp_and_ssh_urls() {
        for url in [
            "https://github.com/owner/repo",
            "https://github.com/owner/repo.git",
            "https://github.com/owner/repo/",
            "git@github.com:owner/repo.git",
            "ssh://git@github.com/owner/repo.git",
        ] {
            assert_eq!(
                repo_from_remote_url(url).as_deref(),
                Some("owner/repo"),
                "{url}"
            );
        }
        assert_eq!(repo_from_remote_url("https://github.com/repo"), None);
        assert_eq!(repo_from_remote_url("repo"), None);
    }

    #[test]
    fn directory_name_is_used_outside_a_git_root() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("my-app");
        std::fs::create_dir(&root).unwrap();
        assert_eq!(repo_name(&root), "my-app");
    }

    #[test]
    fn feature_directory_is_relative_to_the_root() {
        let root = Path::new("/r");
        assert_eq!(feature_directory(root, Path::new("/r/QA.md")), "");
        assert_eq!(
            feature_directory(root, Path::new("/r/features/notes/QA.md")),
            "features/notes"
        );
    }

    #[test]
    fn feature_body_fences_the_original_longer_than_any_backticks_in_it() {
        let original = "# QA\n\n````md\n```sh\nls\n```\n````\n";
        assert_eq!(
            feature_body("前提: 2 台使う", original),
            format!("前提: 2 台使う\n\n## QA.md の原文\n\n`````markdown\n{original}`````")
        );
        assert_eq!(
            feature_body("", "# QA"),
            "## QA.md の原文\n\n````markdown\n# QA\n````"
        );
    }

    #[test]
    fn feature_name_falls_back_to_the_directory() {
        let root = Path::new("/r");
        assert_eq!(default_feature_name(root, Path::new("/r/QA.md")), "_root");
        assert_eq!(
            default_feature_name(root, Path::new("/r/features/notes/QA.md")),
            "notes"
        );
    }
}
