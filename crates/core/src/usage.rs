//! 利用記録 `$TANSU_HOME/usage.jsonl`。CLI の実行の種類と日時だけを 1 行 1 件の JSON で足す (データの内容は書かない)。
//! `documents/DIRECTION.md`「判定基準」の計測元で、外部へは送らない (`documents/PROJECT.md`「制約」)。

use std::io::Write;
use std::path::Path;

use serde_json::json;

use crate::db::{create_private_dir_all, now};
use crate::error::Error;

/// `TANSU_HOME` の下の利用記録のファイル名 (`documents/PROJECT.md`「制約」)。
const USAGE_FILE_NAME: &str = "usage.jsonl";

/// `home` (`TANSU_HOME`) の利用記録に `{"event": <event>, "at": <時差付き ISO 8601>}` を 1 行足す。
/// ファイルとディレクトリが無ければ、DB と同じく所有者だけが読み書きできる権限で作る。
/// 1 行を 1 回の追記で書くので、別プロセスが同時に足しても行が混ざらない。
pub fn record(home: &Path, event: &str) -> Result<(), Error> {
    create_private_dir_all(home)?;
    let mut options = std::fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
        .open(home.join(USAGE_FILE_NAME))?
        .write_all(format!("{}\n", json!({"event": event, "at": now()})).as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_record_appends_one_line() {
        let home = tempfile::tempdir().unwrap();
        record(home.path(), "qa_record").unwrap();
        record(home.path(), "query").unwrap();
        let lines: Vec<serde_json::Value> =
            std::fs::read_to_string(home.path().join(USAGE_FILE_NAME))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["event"], "qa_record");
        assert_eq!(lines[1]["event"], "query");
        assert!(
            chrono::DateTime::parse_from_rfc3339(lines[0]["at"].as_str().unwrap()).is_ok(),
            "{}",
            lines[0]
        );
        assert_eq!(lines[0].as_object().unwrap().len(), 2);
    }
}
