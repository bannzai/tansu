//! `tansu serve` の HTTP API のトークン `$TANSU_HOME/token`。`tansu serve` が起動のたびに作り直し、HTTP API は
//! `Authorization: Bearer <トークン>` の付いた要求だけを受け付ける (`documents/PROJECT.md`「制約」)。ファイルは DB と同じく
//! 所有者だけが読めるので、API に届くのはこのファイルを読める同じユーザーだけになる。

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::db::create_private_dir_all;
use crate::error::Error;

/// `TANSU_HOME` の下のトークンのファイル名 (`documents/PROJECT.md`「制約」)。
const TOKEN_FILE_NAME: &str = "token";

/// トークンの文字数。nanoid の既定の alphabet (64 文字。URL の `#` の後にそのまま書ける) の 1 文字は 6 ビットなので、
/// 32 文字で 192 ビットになり、総当たりで当てられない (行 ID の 21 文字 = 126 ビットより長くした)。
const TOKEN_LENGTH: usize = 32;

/// `home` (`TANSU_HOME`) の下のトークンのファイルのパス。
pub fn path(home: &Path) -> PathBuf {
    home.join(TOKEN_FILE_NAME)
}

/// 新しいトークンを作って [`path`] に書き、そのトークンを返す。ファイルとディレクトリが無ければ、DB と同じく所有者だけが
/// 読み書きできる権限で作り、既にあるファイルも中身を書く前に所有者だけが読み書きできる権限に直す。
/// 呼ぶたびに別のトークンになり、前のトークンは使えなくなる (冪等にしないのは、起動のたびに作り直すため)。
pub fn issue(home: &Path) -> Result<String, Error> {
    create_private_dir_all(home)?;
    let token = nanoid::nanoid!(TOKEN_LENGTH);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path(home))?;
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    file.write_all(token.as_bytes())?;
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_writes_a_new_token_each_time() {
        let home = tempfile::tempdir().unwrap();
        let first = issue(home.path()).unwrap();
        assert_eq!(first.len(), TOKEN_LENGTH);
        assert_eq!(std::fs::read_to_string(path(home.path())).unwrap(), first);
        let second = issue(home.path()).unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read_to_string(path(home.path())).unwrap(), second);
    }

    #[cfg(unix)]
    #[test]
    fn token_file_is_readable_only_by_the_owner() {
        use std::os::unix::fs::PermissionsExt;

        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let home = tempfile::tempdir().unwrap();
        let new_home = home.path().join("new");
        issue(&new_home).unwrap();
        assert_eq!(mode(&path(&new_home)), 0o600);
        assert_eq!(mode(&new_home), 0o700);

        // 他のユーザーが読める権限で残っていたファイルも、書き直す時に所有者だけに直す
        std::fs::set_permissions(path(&new_home), std::fs::Permissions::from_mode(0o644)).unwrap();
        issue(&new_home).unwrap();
        assert_eq!(mode(&path(&new_home)), 0o600);
    }
}
