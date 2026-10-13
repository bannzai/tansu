//! DB を開き、マイグレーションを当てる。物理スキーマ (CREATE TABLE) はこのモジュールの `migrations/` にだけ書き、
//! SQL はこのモジュールの下 (`schema` / `rows`) にだけ書く (`.claude/rules/sqlite-database.md`)。

pub mod rows;
pub mod schema;

use std::time::Duration;

use libsql::{Builder, Connection, Transaction, TransactionBehavior};

use crate::config::{DbLocation, Settings};
use crate::error::Error;

/// 番号順のマイグレーション。N 番目 (1 始まり) を当て終えると `schema_migration` に N の行が入る。
/// 当てたものは書き換えず、変更は新しい番号で足す (`.claude/rules/sqlite-database.md`)。
const MIGRATIONS: &[&str] = &[include_str!("migrations/0001_create_tables.sql")];

/// 適用済みのマイグレーションの番号を記録する `schema_migration` テーブルを、無ければ作る SQL。
const CREATE_SCHEMA_MIGRATION_TABLE: &str = include_str!("migrations/schema_migration.sql");

/// ローカルのファイルを開く接続が、別プロセス (CLI・`tansu mcp`・`tansu serve`) の書き込みを待つ時間。
/// SQLite の既定の busy handler は NULL で、ロックに当たると待たずに SQLITE_BUSY を返す
/// (https://www.sqlite.org/c3ref/busy_handler.html「The default busy callback is NULL.」)。
/// `qa import` の 1 回の取り込み (数百行の upsert を 1 トランザクション) がローカルで 1 秒未満と見込むので、
/// その数倍の 5 秒にした。この見込みは推定で、実測していない。
const LOCAL_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// 設定が指す DB を開き、未適用のマイグレーションを当てて接続を返す。
/// 何度呼んでも同じ状態になる (適用済みのマイグレーションは当て直さない)。
/// DB のスキーマの版がこのバイナリより新しい時は `Error::SchemaNewerThanBinary` を返す。
/// `libsql://` (Turso Cloud) は接続を最初の問い合わせ (マイグレーションの版の読み取り) まで張らないため
/// (libsql 0.9.30 `Database::connect` のコメント「it will not call any C ffi and will lazily create a HTTP connection」)、
/// トークンの誤り・接続できない先はここで `Error::Db` になる。
pub async fn open(settings: &Settings) -> Result<Connection, Error> {
    let connection = match &settings.db {
        DbLocation::Local(path) => {
            if let Some(directory) = path.parent() {
                create_private_dir_all(directory)?;
            }
            create_private_file_if_missing(path)?;
            let connection = Builder::new_local(path).build().await?.connect()?;
            connection.busy_timeout(LOCAL_BUSY_TIMEOUT)?;
            enable_wal(&connection).await?;
            connection
        }
        // WAL・busy_timeout は送らない。Turso Cloud は journal mode を自分で決めて変えさせず
        // (https://docs.turso.tech/cloud/limitations「Turso Cloud manages journaling internally and does not allow changing
        // the journal mode.」)、busy_timeout はリモートの接続では何もしない (libsql 0.9.30 `src/hrana/hyper.rs` の
        // `busy_timeout`「Busy timeout is a no-op for remote connections.」)
        DbLocation::Remote { url, auth_token } => {
            Builder::new_remote(url.clone(), auth_token.expose().to_owned())
                .build()
                .await?
                .connect()?
        }
    };
    migrate(&connection, matches!(settings.db, DbLocation::Local(_))).await?;
    Ok(connection)
}

/// DB を置くディレクトリを作る。DB に private リポジトリの QA の項目が入るため、unix では新しく作るディレクトリを
/// 所有者だけが読み書きできる権限 (0o700) にして、同じ Mac の他のユーザーに読ませない。
/// 既にあるディレクトリの権限は変えない。何度呼んでも同じ状態になる。
pub(crate) fn create_private_dir_all(directory: &std::path::Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(directory)
}

/// DB ファイルが無ければ、所有者だけが読み書きできる権限 (unix では 0o600) の空ファイルとして作る (空ファイルは SQLite が
/// 空の DB として開く)。既存のディレクトリが他のユーザーから読める権限でも、DB の中身を読ませないため。
/// WAL・journal・shm の付随ファイルは SQLite の unix VFS が DB ファイルと同じ権限で作るので、ここで作らなくてよい
/// (SQLite の `src/os_unix.c`: `findCreateFileMode` のコメント「WAL and journal files are created using the same
/// permissions as the associated database file」と、shm を開く `robust_open(zShm, ..., (sStat.st_mode&0777))`)。
/// 既にあるファイルの権限は変えない。何度呼んでも同じ状態になる。
fn create_private_file_if_missing(path: &std::path::Path) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    match options.open(path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

/// WAL への切り替えを `LOCAL_BUSY_TIMEOUT` まで再試行する間隔。WAL への切り替えは数 ms で終わる見込み (推定で、実測していない)
/// なので、その 10 倍ほど空ければ長く待ち過ぎず、CPU を使い続ける busy loop にもならない。
const WAL_RETRY_INTERVAL: Duration = Duration::from_millis(50);

/// 拡張結果コードから主結果コードを取り出すマスク (下位 8 ビット。https://www.sqlite.org/rescode.html)。
const PRIMARY_RESULT_CODE_MASK: std::ffi::c_int = 0xff;

/// SQLite の結果コード SQLITE_BUSY (https://www.sqlite.org/rescode.html#busy)。
const SQLITE_BUSY: std::ffi::c_int = 5;

/// SQLite の結果コード SQLITE_LOCKED (https://www.sqlite.org/rescode.html#locked)。
const SQLITE_LOCKED: std::ffi::c_int = 6;

/// ローカルのファイルを WAL モードにする。新しい DB を複数のプロセスが同時に開くと切り替えが競合し、
/// `busy_timeout` があっても SQLITE_BUSY で失敗するか、切り替わらずに元のモードが返ることがある
/// (https://www.sqlite.org/pragma.html#pragma_journal_mode「If the journal mode could not be changed,
/// the original journal mode is returned.」)。どちらも `LOCAL_BUSY_TIMEOUT` まで待って再試行する。
///
/// `libsql::Error::SqliteFailure` の第 1 要素は `sqlite3_extended_errcode` の値なので、主結果コードで比べるために下位 8 ビットを取る
/// (https://www.sqlite.org/rescode.html「primary result code」は拡張結果コードの下位 8 ビット)。
async fn enable_wal(connection: &Connection) -> Result<(), Error> {
    let deadline = std::time::Instant::now() + LOCAL_BUSY_TIMEOUT;
    loop {
        let attempt = async {
            let mut rows = connection.query("PRAGMA journal_mode=WAL", ()).await?;
            let row = rows
                .next()
                .await?
                .ok_or(Error::EmptyResult("PRAGMA journal_mode=WAL"))?;
            Ok::<String, Error>(row.get::<String>(0)?)
        }
        .await;
        let retry = match &attempt {
            Ok(mode) => !mode.eq_ignore_ascii_case("wal"),
            Err(Error::Db(libsql::Error::SqliteFailure(code, _))) => {
                let primary_code = *code & PRIMARY_RESULT_CODE_MASK;
                primary_code == SQLITE_BUSY || primary_code == SQLITE_LOCKED
            }
            Err(_) => false,
        };
        if !retry {
            return attempt.map(|_| ());
        }
        if std::time::Instant::now() >= deadline {
            return attempt.and_then(|mode| Err(Error::WalNotEnabled(mode)));
        }
        tokio::time::sleep(WAL_RETRY_INTERVAL).await;
    }
}

/// 適用済みの最新の番号 (`schema_migration` の最大の `version`)。`schema_migration` が無い DB
/// (まだ何も当てていない、または `PRAGMA user_version` で記録していた版のバイナリが作った DB) は 0。
pub async fn schema_version(connection: &Connection) -> Result<i64, Error> {
    if !has_schema_migration_table(connection).await? {
        return Ok(0);
    }
    let mut rows = connection
        .query("SELECT COALESCE(MAX(version), 0) FROM schema_migration", ())
        .await?;
    let row = rows.next().await?.ok_or(Error::EmptyResult(
        "SELECT MAX(version) FROM schema_migration",
    ))?;
    Ok(row.get::<i64>(0)?)
}

/// `schema_migration` テーブルがあるか。テーブルの有無を問い合わせの失敗で見分けると、ローカルとリモートで
/// エラーの型が違う (`SqliteFailure` と `Hrana`) ため、`sqlite_master` を読む。
async fn has_schema_migration_table(connection: &Connection) -> Result<bool, Error> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migration'",
            (),
        )
        .await?;
    Ok(rows.next().await?.is_some())
}

/// `PRAGMA user_version` の値。適用済みの番号を `schema_migration` で記録する前の版のバイナリは、ここに番号を書いていた。
async fn user_version(connection: &Connection) -> Result<i64, Error> {
    let mut rows = connection.query("PRAGMA user_version", ()).await?;
    let row = rows
        .next()
        .await?
        .ok_or(Error::EmptyResult("PRAGMA user_version"))?;
    Ok(row.get::<i64>(0)?)
}

/// このバイナリが知っている最新のスキーマの版 (最後のマイグレーションの番号)。
fn latest_version() -> i64 {
    i64::try_from(MIGRATIONS.len()).expect("マイグレーションの数は i64 に収まる")
}

/// DB の版がこのバイナリより新しければエラーにする。
fn ensure_not_newer(db: i64) -> Result<(), Error> {
    let binary = latest_version();
    if db > binary {
        return Err(Error::SchemaNewerThanBinary { db, binary });
    }
    Ok(())
}

/// 書き込みを始める。読み取りの時点で書き込みロックを取り、読んでから書くまでの間に別プロセスが割り込まないようにする
/// (unique の検査を読み取りと同じトランザクションで行うため)。
pub(crate) async fn begin(connection: &Connection) -> Result<Transaction, Error> {
    Ok(connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await?)
}

/// `result` が成功なら `transaction` を確定し、失敗なら巻き戻して `result` のエラーを返す。
pub(crate) async fn finish<T>(
    transaction: Transaction,
    result: Result<T, Error>,
) -> Result<T, Error> {
    match result {
        Ok(value) => {
            transaction.commit().await?;
            Ok(value)
        }
        Err(error) => {
            // 巻き戻しの失敗より、巻き戻す原因になった `error` の方が利用者に必要なため、巻き戻しのエラーは捨てる
            let _ = transaction.rollback().await;
            Err(error)
        }
    }
}

/// 行・データベースに書く時刻。時差付き ISO 8601 (`documents/data-model.md`)。
/// 秒より細かい精度は持たない (行の既定の並びは追加順の rowid で決め、時刻では並べないため)。
pub(crate) fn now() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

/// `row.values` の JSON の中で、プロパティ `property_id` の値 (`Value` の `value`) を指す JSON path。
/// ID は `[0-9A-Za-z_-]` だけなので二重引用符で囲めば path として安全に書ける。
fn value_path(property_id: &crate::model::Id) -> String {
    format!(r#"$."{}".value"#, property_id.0)
}

/// 最新なら write ロックを取らずに戻る。足りない番号がある時だけ `BEGIN IMMEDIATE` を開き、
/// 別プロセス (Turso Cloud では別の Mac) が先に当てた可能性があるため、その中で版を読み直してから当てる。
/// `read_user_version` はローカルのファイルの時だけ true にする (`PRAGMA user_version` で記録していた版のバイナリが
/// 作った DB はローカルにしか無く、Turso Cloud の `user_version` の読み取りは未検証のため送らない)。
async fn migrate(connection: &Connection, read_user_version: bool) -> Result<(), Error> {
    let version = schema_version(connection).await?;
    ensure_not_newer(version)?;
    if version == latest_version() {
        return Ok(());
    }

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await?;
    let result = migrate_in(&transaction, read_user_version).await;
    finish(transaction, result).await
}

/// [`migrate`] の本体。`connection` は書き込みロックを取ったトランザクション。
/// `schema_migration` が無ければ作り、`read_user_version` の時は `PRAGMA user_version` の番号までを適用済みとして記録してから、
/// 未適用の番号を当てる。
async fn migrate_in(connection: &Connection, read_user_version: bool) -> Result<(), Error> {
    if !has_schema_migration_table(connection).await? {
        connection
            .execute_batch(CREATE_SCHEMA_MIGRATION_TABLE)
            .await?;
        if read_user_version {
            let applied = user_version(connection).await?;
            ensure_not_newer(applied)?;
            for number in 1..=applied {
                record_migration(connection, number).await?;
            }
        }
    }
    let version = schema_version(connection).await?;
    ensure_not_newer(version)?;
    for (index, sql) in MIGRATIONS.iter().enumerate() {
        let number = i64::try_from(index + 1).expect("マイグレーションの数は i64 に収まる");
        if number > version {
            connection.execute_batch(sql).await?;
            record_migration(connection, number).await?;
        }
    }
    Ok(())
}

/// 番号 `number` のマイグレーションを当て終えたことを `schema_migration` に記録する。
/// 同じ番号が既にあれば PRIMARY KEY の衝突で失敗し、呼び出し元のトランザクションごと巻き戻る (二重に当てた変更を残さない)。
async fn record_migration(connection: &Connection, number: i64) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO schema_migration (version) VALUES (?)",
            [number],
        )
        .await?;
    Ok(())
}
