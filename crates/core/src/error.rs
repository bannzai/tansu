/// コアの処理が返すエラー。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `TANSU_HOME` も `HOME` も無く、データの置き場所を決められない。
    #[error("TANSU_HOME も HOME も設定されていないため、データの置き場所を決められません")]
    HomeUnresolved,
    /// 環境変数の値が UTF-8 でない。
    #[error("環境変数 {0} の値が UTF-8 ではありません")]
    EnvNotUnicode(&'static str),
    /// `TANSU_DB_URL` が `file:<パス>` でも `libsql://<host>` でもない。
    /// 認証情報が続き得るため、URL 全体ではなくスキーム (`://` が無ければ先頭の数文字) だけを持つ。
    #[error(
        "TANSU_DB_URL ({0}...) を解釈できません (空でないパスの file:<パス> か libsql://<host> を指定してください)"
    )]
    UnsupportedDbUrl(String),
    /// `TANSU_DB_URL` が `file::memory:` (インメモリの DB)。ファイルのパスではなく、WAL にも切り替えられないため開かない。
    #[error(
        "file::memory: (インメモリの DB) には対応していません (file:<ファイルのパス> を指定してください)"
    )]
    InMemoryDbNotSupported,
    /// `libsql://<host>` を指定したのに `TANSU_DB_AUTH_TOKEN` が無い。
    #[error("libsql:// の DB を開くには TANSU_DB_AUTH_TOKEN が要ります")]
    MissingAuthToken,
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
    /// DB のスキーマの版が、このバイナリが知っている最新のマイグレーションより新しい (古いバイナリが新しい DB を開いた)。
    #[error(
        "DB のスキーマの版 ({db}) がこのバイナリの版 ({binary}) より新しいため開けません。tansu を更新してください"
    )]
    SchemaNewerThanBinary { db: i64, binary: i64 },
    /// ローカルの DB を WAL モードに切り替えられなかった (再試行の期限まで別のプロセスと競合し続けた)。
    #[error("DB を WAL モードに切り替えられませんでした (journal_mode が {0} のまま)")]
    WalNotEnabled(String),
    /// 必ず 1 行返すはずの問い合わせが行を返さなかった。
    #[error("{0} が結果を返しませんでした")]
    EmptyResult(&'static str),
    /// git (`qa verified` / `qa export` が project の checkout で実行する) を起動できなかった、または失敗した。
    #[error("git の実行に失敗しました: {0}")]
    Git(String),
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
