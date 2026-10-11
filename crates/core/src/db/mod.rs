//! DB を開き、マイグレーションを当てる。物理スキーマ (CREATE TABLE) はこのモジュールの `migrations/` にだけ書く。

use std::time::Duration;

use libsql::{Builder, Connection, TransactionBehavior};

use crate::config::{DbLocation, Settings};
use crate::error::Error;

/// 番号順のマイグレーション。N 番目 (1 始まり) を当て終えると `user_version` が N になる。
/// 当てたものは書き換えず、変更は新しい番号で足す (`.claude/rules/sqlite-database.md`)。
const MIGRATIONS: &[&str] = &[include_str!("migrations/0001_create_tables.sql")];

/// ローカルのファイルを開く接続が、別プロセス (CLI・`tansu mcp`・`tansu serve`) の書き込みを待つ時間。
/// SQLite の既定の busy handler は NULL で、ロックに当たると待たずに SQLITE_BUSY を返す
/// (https://www.sqlite.org/c3ref/busy_handler.html「The default busy callback is NULL.」)。
/// `qa import` の 1 回の取り込み (数百行の upsert を 1 トランザクション) がローカルで 1 秒未満と見込むので、
/// その数倍の 5 秒にした。この見込みは推定で、実測していない。
const LOCAL_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// 設定が指す DB を開き、未適用のマイグレーションを当てて接続を返す。
/// 何度呼んでも同じ状態になる (適用済みのマイグレーションは当て直さない)。
/// DB のスキーマの版がこのバイナリより新しい時は `Error::SchemaNewerThanBinary` を返す。
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
        DbLocation::Remote { url, auth_token } => {
            Builder::new_remote(url.clone(), auth_token.expose().to_owned())
                .build()
                .await?
                .connect()?
        }
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

/// 最新なら write ロックを取らずに戻る。足りない番号がある時だけ `BEGIN IMMEDIATE` を開き、
/// 別プロセスが先に当てた可能性があるため、その中で版を読み直してから当てる。
async fn migrate(connection: &Connection) -> Result<(), Error> {
    let version = schema_version(connection).await?;
    ensure_not_newer(version)?;
    if version == latest_version() {
        return Ok(());
    }

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await?;
    let version = schema_version(&transaction).await?;
    ensure_not_newer(version)?;
    for (index, sql) in MIGRATIONS.iter().enumerate() {
        let number = i64::try_from(index + 1).expect("マイグレーションの数は i64 に収まる");
        if number > version {
            transaction.execute_batch(sql).await?;
            transaction
                .execute(&format!("PRAGMA user_version = {number}"), ())
                .await?;
        }
    }
    transaction.commit().await?;
    Ok(())
}
