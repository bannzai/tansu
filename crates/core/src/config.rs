use std::path::PathBuf;

use crate::error::Error;

/// `TANSU_HOME` が未設定の時にユーザーのホームディレクトリの下に使うディレクトリ名。
/// `~/.claude` / `~/.codex` と同じく、ツール名のドット付きディレクトリにして、バックアップ・削除の対象を見つけやすくする
/// (`documents/PROJECT.md`「汎用のデータベース」)。
const DEFAULT_HOME_DIR_NAME: &str = ".tansu";

/// `TANSU_DB_URL` が未設定の時に `TANSU_HOME` の下へ置く DB ファイル名 (`documents/PROJECT.md`「DB の置き場所」)。
const DEFAULT_DB_FILE_NAME: &str = "tansu.sqlite";

/// 設定の元になる環境変数の値。プロセスの環境を読むのは [`EnvVars::from_process`] だけで、
/// [`Settings::resolve`] は値を受け取るだけにする (テストが環境変数を書き換えずに済むように)。
#[derive(Debug, Default)]
pub struct EnvVars {
    /// `TANSU_HOME`
    pub tansu_home: Option<String>,
    /// `HOME`。`TANSU_HOME` が未設定の時の既定の置き場所を決める。
    pub home: Option<String>,
    /// `TANSU_DB_URL`
    pub db_url: Option<String>,
    /// `TANSU_DB_AUTH_TOKEN`
    pub db_auth_token: Option<String>,
}

impl EnvVars {
    /// 現在のプロセスの環境変数から読む。
    pub fn from_process() -> Self {
        Self {
            tansu_home: std::env::var("TANSU_HOME").ok(),
            home: std::env::var("HOME").ok(),
            db_url: std::env::var("TANSU_DB_URL").ok(),
            db_auth_token: std::env::var("TANSU_DB_AUTH_TOKEN").ok(),
        }
    }
}

/// DB の置き場所 (`documents/PROJECT.md`「DB の置き場所」)。
#[derive(Debug, PartialEq, Eq)]
pub enum DbLocation {
    /// ローカルの SQLite ファイル。
    Local(PathBuf),
    /// Turso Cloud の DB への直接接続。
    Remote { url: String, auth_token: String },
}

/// 解決済みの設定。
#[derive(Debug, PartialEq, Eq)]
pub struct Settings {
    /// tansu がこのマシンに書くファイルを置くディレクトリ (`TANSU_HOME`)。
    pub home: PathBuf,
    /// 開く DB。
    pub db: DbLocation,
}

impl Settings {
    /// 現在のプロセスの環境変数から設定を解決する。
    pub fn from_env() -> Result<Self, Error> {
        Self::resolve(EnvVars::from_process())
    }

    /// 環境変数の値から設定を解決する。空文字は未設定として扱う。
    pub fn resolve(env: EnvVars) -> Result<Self, Error> {
        let home = match (non_empty(env.tansu_home), non_empty(env.home)) {
            (Some(tansu_home), _) => PathBuf::from(tansu_home),
            (None, Some(home)) => PathBuf::from(home).join(DEFAULT_HOME_DIR_NAME),
            (None, None) => return Err(Error::HomeUnresolved),
        };
        let db = match non_empty(env.db_url) {
            None => DbLocation::Local(home.join(DEFAULT_DB_FILE_NAME)),
            Some(url) => match url.strip_prefix("file:") {
                Some(path) => DbLocation::Local(PathBuf::from(path)),
                None if url.starts_with("libsql://") => DbLocation::Remote {
                    url,
                    auth_token: non_empty(env.db_auth_token).ok_or(Error::MissingAuthToken)?,
                },
                None => return Err(Error::UnsupportedDbUrl(url)),
            },
        };
        Ok(Self { home, db })
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with_db_url(db_url: Option<&str>, db_auth_token: Option<&str>) -> EnvVars {
        EnvVars {
            tansu_home: Some("/data/tansu".into()),
            db_url: db_url.map(Into::into),
            db_auth_token: db_auth_token.map(Into::into),
            ..EnvVars::default()
        }
    }

    #[test]
    fn home_defaults_to_dot_tansu_under_user_home() {
        let settings = Settings::resolve(EnvVars {
            home: Some("/Users/example".into()),
            ..EnvVars::default()
        })
        .unwrap();
        assert_eq!(settings.home, PathBuf::from("/Users/example/.tansu"));
        assert_eq!(
            settings.db,
            DbLocation::Local(PathBuf::from("/Users/example/.tansu/tansu.sqlite"))
        );
    }

    #[test]
    fn tansu_home_wins_over_user_home() {
        let settings = Settings::resolve(EnvVars {
            tansu_home: Some("/data/tansu".into()),
            home: Some("/Users/example".into()),
            ..EnvVars::default()
        })
        .unwrap();
        assert_eq!(settings.home, PathBuf::from("/data/tansu"));
    }

    #[test]
    fn missing_home_is_an_error() {
        assert!(matches!(
            Settings::resolve(EnvVars::default()),
            Err(Error::HomeUnresolved)
        ));
    }

    #[test]
    fn file_url_points_to_the_given_path() {
        let settings =
            Settings::resolve(env_with_db_url(Some("file:/var/db/a.sqlite"), None)).unwrap();
        assert_eq!(
            settings.db,
            DbLocation::Local(PathBuf::from("/var/db/a.sqlite"))
        );
    }

    #[test]
    fn libsql_url_needs_an_auth_token() {
        assert!(matches!(
            Settings::resolve(env_with_db_url(Some("libsql://example.turso.io"), None)),
            Err(Error::MissingAuthToken)
        ));
        let settings = Settings::resolve(env_with_db_url(
            Some("libsql://example.turso.io"),
            Some("token"),
        ))
        .unwrap();
        assert_eq!(
            settings.db,
            DbLocation::Remote {
                url: "libsql://example.turso.io".into(),
                auth_token: "token".into(),
            }
        );
    }

    #[test]
    fn other_schemes_are_rejected() {
        assert!(matches!(
            Settings::resolve(env_with_db_url(Some("https://example.com"), None)),
            Err(Error::UnsupportedDbUrl(_))
        ));
    }
}
