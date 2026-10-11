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
    /// `libsql://<host>` を指定したのに `TANSU_DB_AUTH_TOKEN` が無い。
    #[error("libsql:// の DB を開くには TANSU_DB_AUTH_TOKEN が要ります")]
    MissingAuthToken,
    /// DB のスキーマの版が、このバイナリが知っている最新のマイグレーションより新しい (古いバイナリが新しい DB を開いた)。
    #[error(
        "DB のスキーマの版 ({db}) がこのバイナリの版 ({binary}) より新しいため開けません。tansu を更新してください"
    )]
    SchemaNewerThanBinary { db: i64, binary: i64 },
    /// 必ず 1 行返すはずの問い合わせが行を返さなかった。
    #[error("{0} が結果を返しませんでした")]
    EmptyResult(&'static str),
    /// DB ファイルを置くディレクトリを作れなかった、などのファイル操作の失敗。
    #[error("ファイル操作に失敗しました: {0}")]
    Io(#[from] std::io::Error),
    /// libSQL の操作の失敗。
    #[error("DB の操作に失敗しました: {0}")]
    Db(#[from] libsql::Error),
}
