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
    /// `libsql://<host>` (Turso Cloud) への接続。Turso Cloud の接続の issue で対応するまでは開かない。
    #[error(
        "libsql:// の DB への接続はまだ対応していません (TANSU_DB_URL を外すか file:<パス> を指定してください)"
    )]
    RemoteNotSupported,
    /// 指定した名前のデータベースが無い。
    #[error("データベース `{0}` はありません")]
    DatabaseNotFound(String),
    /// データベースに指定した名前のプロパティが無い。
    #[error("データベース `{database}` にプロパティ `{property}` はありません")]
    PropertyNotFound { database: String, property: String },
    /// 利用者の入力 (引数・行の JSON) が定義と合わない。メッセージに直し方を書く。
    #[error("{0}")]
    InvalidInput(String),
    /// unique のプロパティに、別の行と同じ値を書こうとした。
    #[error("プロパティ `{property}` は unique で、同じ値の行 `{row_id}` が既にあります")]
    UniqueViolation { property: String, row_id: String },
    /// DB ファイルを置くディレクトリを作れなかった、などのファイル操作の失敗。
    #[error("ファイル操作に失敗しました: {0}")]
    Io(#[from] std::io::Error),
    /// libSQL の操作の失敗。
    #[error("DB の操作に失敗しました: {0}")]
    Db(#[from] libsql::Error),
    /// JSON (入力の行・DB に保存した値) を解釈できなかった。
    #[error("JSON を解釈できません: {0}")]
    Json(#[from] serde_json::Error),
}
