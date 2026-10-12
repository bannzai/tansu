use std::env::VarError;
use std::fmt;
use std::path::PathBuf;

use crate::error::Error;

/// `TANSU_HOME` が未設定の時にユーザーのホームディレクトリの下に使うディレクトリ名。
/// `~/.claude` / `~/.codex` と同じく、ツール名のドット付きディレクトリにして、バックアップ・削除の対象を見つけやすくする
/// (`documents/PROJECT.md`「汎用のデータベース」)。
const DEFAULT_HOME_DIR_NAME: &str = ".tansu";

/// `TANSU_DB_URL` が未設定の時に `TANSU_HOME` の下へ置く DB ファイル名 (`documents/PROJECT.md`「DB の置き場所」)。
const DEFAULT_DB_FILE_NAME: &str = "tansu.sqlite";

/// `://` を含まない `TANSU_DB_URL` を `Error::UnsupportedDbUrl` に入れる時に残す先頭の文字数。
/// URL の後ろには認証情報が続き得るため全体は入れない。16 文字なら `libsql://` (9 文字) と `file:` (5 文字) の
/// どちらのスキームも収まり、`://` の後の認証情報 (ホスト名より後) はほぼ含まない。
const URL_PREFIX_LEN_WITHOUT_SCHEME_SEPARATOR: usize = 16;

/// Turso Cloud の認証トークン。`Debug` の出力 (ログ・エラー・テストの失敗表示) に値を出さない。
#[derive(Clone, PartialEq, Eq)]
pub struct AuthToken(String);

impl AuthToken {
    /// 環境変数などから読んだトークンの値を包む。値の検証はしない (空文字は `Settings::resolve` が未設定として扱う)。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// トークンの値。DB への接続にだけ使う。
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AuthToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuthToken(\"***\")")
    }
}

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
    pub db_auth_token: Option<AuthToken>,
}

impl EnvVars {
    /// 現在のプロセスの環境変数から読む。値が UTF-8 でない変数は未設定扱いにせず、エラーにする。
    pub fn from_process() -> Result<Self, Error> {
        Ok(Self {
            tansu_home: read_env("TANSU_HOME")?,
            home: read_env("HOME")?,
            db_url: read_env("TANSU_DB_URL")?,
            db_auth_token: read_env("TANSU_DB_AUTH_TOKEN")?.map(AuthToken::new),
        })
    }
}

/// DB の置き場所 (`documents/PROJECT.md`「DB の置き場所」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbLocation {
    /// ローカルの SQLite ファイル。
    Local(PathBuf),
    /// Turso Cloud の DB への直接接続。
    Remote { url: String, auth_token: AuthToken },
}

/// 解決済みの設定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// tansu がこのマシンに書くファイルを置くディレクトリ (`TANSU_HOME`)。
    pub home: PathBuf,
    /// 開く DB。
    pub db: DbLocation,
}

impl Settings {
    /// 現在のプロセスの環境変数から設定を解決する。
    pub fn from_env() -> Result<Self, Error> {
        Self::resolve(EnvVars::from_process()?)
    }

    /// 環境変数の値から設定を解決する。空文字は未設定として扱う。
    /// `TANSU_HOME` と `file:<パス>` のパスが相対パスの時は、プロセスのカレントディレクトリ基準になる (絶対パスは必須にしない)。
    pub fn resolve(env: EnvVars) -> Result<Self, Error> {
        let home = match (non_empty(env.tansu_home), non_empty(env.home)) {
            (Some(tansu_home), _) => PathBuf::from(tansu_home),
            (None, Some(home)) => PathBuf::from(home).join(DEFAULT_HOME_DIR_NAME),
            (None, None) => return Err(Error::HomeUnresolved),
        };
        let db = match non_empty(env.db_url) {
            None => DbLocation::Local(home.join(DEFAULT_DB_FILE_NAME)),
            Some(url) => match url.strip_prefix("file:") {
                Some("") => return Err(Error::UnsupportedDbUrl(url_scheme(&url))),
                // `:memory:` はファイルのパスとして扱えず、WAL にも切り替えられない (インメモリは非対応。`documents/PROJECT.md`「DB の置き場所」)
                Some(":memory:") => return Err(Error::InMemoryDbNotSupported),
                Some(path) => DbLocation::Local(PathBuf::from(path)),
                None if url
                    .strip_prefix("libsql://")
                    .is_some_and(|host| !host.is_empty()) =>
                {
                    DbLocation::Remote {
                        url,
                        auth_token: env
                            .db_auth_token
                            .filter(|token| !token.expose().is_empty())
                            .ok_or(Error::MissingAuthToken)?,
                    }
                }
                None => return Err(Error::UnsupportedDbUrl(url_scheme(&url))),
            },
        };
        Ok(Self { home, db })
    }
}

/// 環境変数の値を読む。未設定は `None`、UTF-8 でない値は `Error::EnvNotUnicode`。
fn read_env(name: &'static str) -> Result<Option<String>, Error> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(VarError::NotPresent) => Ok(None),
        Err(VarError::NotUnicode(_)) => Err(Error::EnvNotUnicode(name)),
    }
}

/// エラーに入れる URL の表示用の部分。認証情報が続き得る URL 全体は入れない。
/// `://` の前までをスキームとして返し、`://` が無ければ先頭の数文字だけを返す。
fn url_scheme(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, _)) => scheme.to_owned(),
        None => url
            .chars()
            .take(URL_PREFIX_LEN_WITHOUT_SCHEME_SEPARATOR)
            .collect(),
    }
}

/// 空文字の環境変数を未設定 (`None`) として扱うための絞り込み。
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
            db_auth_token: db_auth_token.map(AuthToken::new),
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
    fn file_url_without_a_path_is_rejected() {
        assert!(matches!(
            Settings::resolve(env_with_db_url(Some("file:"), None)),
            Err(Error::UnsupportedDbUrl(_))
        ));
    }

    #[test]
    fn in_memory_file_url_is_rejected() {
        assert!(matches!(
            Settings::resolve(env_with_db_url(Some("file::memory:"), None)),
            Err(Error::InMemoryDbNotSupported)
        ));
    }

    #[test]
    fn libsql_url_without_a_host_is_rejected() {
        assert!(matches!(
            Settings::resolve(env_with_db_url(Some("libsql://"), Some("token"))),
            Err(Error::UnsupportedDbUrl(_))
        ));
    }

    #[test]
    fn libsql_url_needs_an_auth_token() {
        assert!(matches!(
            Settings::resolve(env_with_db_url(Some("libsql://example.turso.io"), None)),
            Err(Error::MissingAuthToken)
        ));
        assert!(matches!(
            Settings::resolve(env_with_db_url(Some("libsql://example.turso.io"), Some(""))),
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
                auth_token: AuthToken::new("token"),
            }
        );
    }

    #[test]
    fn other_schemes_are_rejected_with_the_scheme_only() {
        let error = Settings::resolve(env_with_db_url(
            Some("https://user:secret@example.com/db"),
            None,
        ))
        .unwrap_err();
        assert!(matches!(&error, Error::UnsupportedDbUrl(scheme) if scheme == "https"));
        assert!(!error.to_string().contains("secret"));
    }

    #[test]
    fn url_without_scheme_separator_keeps_only_a_short_prefix() {
        let error = Settings::resolve(env_with_db_url(
            Some("not-a-url-with-secret-material"),
            None,
        ))
        .unwrap_err();
        assert!(matches!(&error, Error::UnsupportedDbUrl(prefix) if prefix == "not-a-url-with-s"));
    }

    #[test]
    fn auth_token_is_masked_in_debug_output() {
        let env = env_with_db_url(Some("libsql://example.turso.io"), Some("super-secret"));
        assert!(!format!("{env:?}").contains("super-secret"));
        let settings = Settings::resolve(env).unwrap();
        assert!(!format!("{settings:?}").contains("super-secret"));
    }
}
