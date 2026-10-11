//! `tansu qa import`: QA.md 群を QA 層の 4 つのデータベースに取り込む。
//! 同じファイルを 2 回取り込んでも行が増えない (project は `repo`、feature は project + 名前、item は feature + 名前、
//! run は `run_id` を自然キーにして upsert する。`documents/PROJECT.md`「制約」の冪等)。

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use libsql::Connection;
use serde::Serialize;
use serde_json::json;

use super::qa_md::{self, QaMd, QaMdItem, QaMdRecordKind, SkippedLine};
use super::{FEATURE, ITEM, PROJECT, RUN, ensure_schema};
use crate::db::{begin, finish, rows, schema};
use crate::error::Error;
use crate::model::Id;

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
    /// 取り込んだ記録 (チェック済み・⏭️・❌) の数。
    pub run: usize,
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
    /// 解釈できずに読み飛ばした行。
    pub skipped: Vec<SkippedLine>,
}

/// 読んだ QA.md。取り込みの単位 (ルート, project の `repo`, ファイル, feature 名, パース結果)。
type ParsedFile = (PathBuf, String, PathBuf, String, QaMd);

/// `paths` の QA.md を読むだけで、DB を開かず何も書かない (`--dry-run`)。結果は [`import`] と同じ件数と読み飛ばした行。
pub fn dry_run(paths: &[PathBuf]) -> Result<ImportReport, Error> {
    Ok(report(&parse_all(paths)?, true))
}

/// `paths` の QA.md を取り込む。パスはリポジトリのルート (配下の `QA.md` をすべて探す) か QA.md のファイル。
///
/// - project: ルートの `git remote` の `owner/repo` (取れなければディレクトリ名。[`repo_name`])。`path` はルートの絶対パス
/// - feature: frontmatter の `feature` (無ければ、ルート直下の QA.md は `_root`、それ以外は親ディレクトリの名前)
/// - item: 項目名・期待動作・自動化・仕様 ID。`labels` は QA.md に無いため触らない
/// - run: チェック済み (`ok`)・⏭️ (`skip`)・❌ (`ng`) の項目に 1 行。`source` は `import`、`run_id` は `import:<item の行 ID>`、
///   `recorded_at` はエビデンスの確認日 (無ければ frontmatter の `last_verified_at`、それも無ければ値なし)、
///   `commit` は frontmatter の `last_verified_commit`
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
            let mut files = Vec::new();
            find_qa_md(&root, &mut files)?;
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
    // 2 つ目以降をファイルごと読み飛ばして報告する (feature は project + 名前で行を決めるため)
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
                    qa_md = QaMd {
                        skipped: vec![SkippedLine {
                            file: file_name,
                            line: 1,
                            content: format!("feature: {feature}"),
                            reason: format!(
                                "同じ project の feature `{feature}` が既に {} にある (このファイルは取り込まない)",
                                entry.get().display()
                            ),
                        }],
                        ..QaMd::default()
                    };
                }
            }
            parsed.push((root.clone(), repo.clone(), file.clone(), feature, qa_md));
        }
    }
    Ok(parsed)
}

/// 読んだ QA.md から、取り込みの件数 (入力に含まれる project / feature / item / run の数) と読み飛ばした行をまとめる。
fn report(parsed: &[ParsedFile], dry_run: bool) -> ImportReport {
    let mut projects = BTreeSet::new();
    let mut features = BTreeSet::new();
    let mut items = BTreeSet::new();
    let mut runs = BTreeSet::new();
    // 書き込みと同じ自然キー (project は repo、feature は repo + 名前、item は feature + 名前) で数える。
    // 同じ origin の worktree を 2 つ渡しても DB には 1 組しか入らないため、root のパスでは数えない
    for (_, repo, _, feature, qa_md) in parsed {
        projects.insert(repo);
        features.insert((repo, feature));
        for item in &qa_md.items {
            items.insert((repo, feature, &item.name));
            if run_result(item).is_some() {
                runs.insert((repo, feature, &item.name));
            }
        }
    }
    ImportReport {
        dry_run,
        files: parsed
            .iter()
            .map(|(_, _, file, _, _)| file.display().to_string())
            .collect(),
        counts: ImportCounts {
            project: projects.len(),
            feature: features.len(),
            item: items.len(),
            run: runs.len(),
        },
        skipped: parsed
            .iter()
            .flat_map(|(_, _, _, _, qa_md)| qa_md.skipped.iter().cloned())
            .collect(),
    }
}

/// [`import`] の書き込み。`connection` は書き込みロックを取ったトランザクション。
async fn write_all(connection: &Connection, parsed: &[ParsedFile]) -> Result<(), Error> {
    ensure_schema(connection).await?;
    // multi_select の値は定義にある選択肢しか書けないため、仕様 ID を選択肢として先に足す (無いものだけが足される)
    let spec_ids: BTreeSet<&str> = parsed
        .iter()
        .flat_map(|(_, _, _, _, qa_md)| {
            qa_md.spec_rows.iter().map(|row| row.id.as_str())
        })
        .collect();
    if !spec_ids.is_empty() {
        let spec = format!(
            "spec_ids:multi_select:{}",
            spec_ids.into_iter().collect::<Vec<_>>().join(",")
        );
        schema::change_schema_in(connection, ITEM, &[spec], &[], &[]).await?;
    }

    let mut project_ids: BTreeMap<&str, Id> = BTreeMap::new();
    for (root, repo, _, feature, qa_md) in parsed {
        let project_id = match project_ids.get(repo.as_str()) {
            Some(project_id) => project_id.clone(),
            None => {
                let (_, row) = rows::upsert_in(
                    connection,
                    PROJECT,
                    &json!({"values": {"repo": repo, "path": root.display().to_string()}}),
                )
                .await?;
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
                "values": {"project": [project_id], "name": feature, "verification": qa_md.verification},
            }),
        )
        .await?;
        for item in &qa_md.items {
            let item_id = find_row_id(
                connection,
                ITEM,
                &[
                    format!("feature={}", feature_id.0),
                    format!("name={}", item.name),
                ],
            )
            .await?
            .unwrap_or_else(Id::generate);
            rows::upsert_in(
                connection,
                ITEM,
                &json!({
                    "id": item_id,
                    "values": {
                        "feature": [feature_id],
                        "name": item.name,
                        "expected": item.expected,
                        "automation": item.automation.map(|automation| automation.name()),
                        "automation_detail": item.automation_detail,
                        "spec_ids": item.spec_ids,
                    },
                }),
            )
            .await?;
            let Some(result) = run_result(item) else {
                continue;
            };
            rows::upsert_in(
                connection,
                RUN,
                &json!({
                    "values": {
                        "run_id": format!("import:{}", item_id.0),
                        "item": [item_id],
                        "result": result,
                        "note": item.record.as_ref().map(|record| record.note.as_str()).filter(|note| !note.is_empty()),
                        "recorded_at": item.verified_at.as_deref().or(qa_md.last_verified_at.as_deref()),
                        "source": "import",
                        "image_url": item.image_url,
                        "commit": qa_md.last_verified_commit,
                    },
                }),
            )
            .await?;
        }
    }
    Ok(())
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

/// データベース `database_name` で `filters` に一致する最初の行の ID。無ければ `None`。
async fn find_row_id(
    connection: &Connection,
    database_name: &str,
    filters: &[String],
) -> Result<Option<Id>, Error> {
    let (_, rows) = rows::query_in(connection, database_name, filters, None, Some(1)).await?;
    Ok(rows.into_iter().next().map(|row| row.id))
}

/// `directory` の下の `QA.md` を再帰的に探してパス順で `files` に足す。`.git` / `node_modules` / `target` の下は見ず、
/// symlink は辿らない (リポジトリの外へ出ないように)。
fn find_qa_md(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), Error> {
    let mut entries: Vec<_> = std::fs::read_dir(directory)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        if file_type.is_dir() {
            if !matches!(name.to_str(), Some(".git" | "node_modules" | "target")) {
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
    fn feature_name_falls_back_to_the_directory() {
        let root = Path::new("/r");
        assert_eq!(default_feature_name(root, Path::new("/r/QA.md")), "_root");
        assert_eq!(
            default_feature_name(root, Path::new("/r/features/notes/QA.md")),
            "notes"
        );
    }
}
