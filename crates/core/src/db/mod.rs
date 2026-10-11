//! DB を開き、マイグレーションを当てる。物理スキーマ (CREATE TABLE) はこのモジュールの `migrations/` にだけ書き、
//! SQL はこのモジュールの下 (`schema` / `rows`) にだけ書く (`.claude/rules/sqlite-database.md`)。

pub mod rows;
pub mod schema;

use std::time::Duration;

use libsql::{Builder, Connection, Transaction, TransactionBehavior};

use crate::config::{DbLocation, Settings};
use crate::error::Error;

/// 番号順のマイグレーション。N 番目 (1 始まり) を当て終えると `user_version` が N になる。
/// 当てたものは書き換えず、変更は新しい番号で足す (`.claude/rules/sqlite-database.md`)。
const MIGRATIONS: &[&str] = &[include_str!("migrations/0001_create_tables.sql")];

/// ローカルのファイルを開く接続が、別プロセス (CLI・`tansu mcp`・`tansu serve`) の書き込みを待つ時間。
/// SQLite は既定でロックを待たず即座に失敗するため、短い書き込みの競合を吸収できる秒数にしている。
const LOCAL_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// 設定が指す DB を開き、未適用のマイグレーションを当てて接続を返す。
/// 何度呼んでも同じ状態になる (適用済みのマイグレーションは当て直さない)。
/// `libsql://` (Turso Cloud) は Turso Cloud の接続の issue まで [`Error::RemoteNotSupported`] を返す。
pub async fn open(settings: &Settings) -> Result<Connection, Error> {
    let connection = match &settings.db {
        DbLocation::Local(path) => {
            if let Some(directory) = path.parent() {
                std::fs::create_dir_all(directory)?;
            }
            let connection = Builder::new_local(path).build().await?.connect()?;
            connection.busy_timeout(LOCAL_BUSY_TIMEOUT)?;
            connection
                .query("PRAGMA journal_mode=WAL", ())
                .await?
                .next()
                .await?;
            connection
        }
        DbLocation::Remote { .. } => return Err(Error::RemoteNotSupported),
    };
    migrate(&connection).await?;
    Ok(connection)
}

/// 適用済みの番号 (`PRAGMA user_version`)。
pub async fn schema_version(connection: &Connection) -> Result<i64, Error> {
    let mut rows = connection.query("PRAGMA user_version", ()).await?;
    let row = rows
        .next()
        .await?
        .expect("PRAGMA user_version は必ず 1 行返す");
    Ok(row.get::<i64>(0)?)
}

/// 別プロセスと同時に起動しても二重に当てないよう、番号ごとに書き込みロックを取ってから適用済みかを確かめる。
async fn migrate(connection: &Connection) -> Result<(), Error> {
    for (index, sql) in MIGRATIONS.iter().enumerate() {
        let version = i64::try_from(index + 1).expect("マイグレーションの数は i64 に収まる");
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await?;
        if schema_version(&transaction).await? < version {
            transaction.execute_batch(sql).await?;
            transaction
                .execute(&format!("PRAGMA user_version = {version}"), ())
                .await?;
        }
        transaction.commit().await?;
    }
    Ok(())
}

/// 書き込みを始める。読み取りの時点で書き込みロックを取り、読んでから書くまでの間に別プロセスが割り込まないようにする
/// (unique の検査を読み取りと同じトランザクションで行うため)。
async fn begin(connection: &Connection) -> Result<Transaction, Error> {
    Ok(connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await?)
}

/// `result` が成功なら `transaction` を確定し、失敗なら巻き戻して `result` のエラーを返す。
async fn finish<T>(transaction: Transaction, result: Result<T, Error>) -> Result<T, Error> {
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
fn now() -> String {
    chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

/// `row.values` の JSON の中で、プロパティ `property_id` の値 (`Value` の `value`) を指す JSON path。
/// ID は `[0-9A-Za-z_-]` だけなので二重引用符で囲めば path として安全に書ける。
fn value_path(property_id: &crate::model::Id) -> String {
    format!(r#"$."{}".value"#, property_id.0)
}
