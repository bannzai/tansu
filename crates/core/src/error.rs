/// コアの処理が返すエラー。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `TANSU_HOME` も `HOME` も無く、データの置き場所を決められない。
    #[error("TANSU_HOME も HOME も設定されていないため、データの置き場所を決められません")]
    HomeUnresolved,
    /// `TANSU_DB_URL` が `file:<パス>` でも `libsql://<host>` でもない。
    #[error(
        "TANSU_DB_URL の `{0}` は解釈できません (file:<パス> か libsql://<host> を指定してください)"
    )]
    UnsupportedDbUrl(String),
    /// `libsql://<host>` を指定したのに `TANSU_DB_AUTH_TOKEN` が無い。
    #[error("libsql:// の DB を開くには TANSU_DB_AUTH_TOKEN が要ります")]
    MissingAuthToken,
    /// DB ファイルを置くディレクトリを作れなかった、などのファイル操作の失敗。
    #[error("ファイル操作に失敗しました: {0}")]
    Io(#[from] std::io::Error),
    /// libSQL の操作の失敗。
    #[error("DB の操作に失敗しました: {0}")]
    Db(#[from] libsql::Error),
}
