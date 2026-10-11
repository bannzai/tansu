use tansu_core::config::{AuthToken, DbLocation, EnvVars, Settings};
use tansu_core::{Error, db};

/// `TANSU_HOME` を一時ディレクトリにした設定。実際のホームディレクトリには書かない。
/// 環境変数そのものは書き換えず、解決済みの値を直接渡す (edition 2024 で `set_var` が unsafe で、並列テストと競合するため)。
fn settings_under(home: &tempfile::TempDir) -> Settings {
    settings_with_db_url(home, None)
}

fn settings_with_db_url(home: &tempfile::TempDir, db_url: Option<String>) -> Settings {
    Settings::resolve(EnvVars {
        tansu_home: Some(home.path().to_str().unwrap().into()),
        db_url,
        ..EnvVars::default()
    })
    .unwrap()
}

async fn table_names(connection: &libsql::Connection) -> Vec<String> {
    let mut rows = connection
        .query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('database', 'property', 'row') ORDER BY name",
            (),
        )
        .await
        .unwrap();
    let mut names = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        names.push(row.get::<String>(0).unwrap());
    }
    names
}

#[tokio::test]
async fn open_creates_the_three_tables_under_tansu_home() {
    let home = tempfile::tempdir().unwrap();
    let settings = settings_under(&home);
    assert_eq!(
        settings.db,
        DbLocation::Local(home.path().join("tansu.sqlite"))
    );

    let connection = db::open(&settings).await.unwrap();

    assert_eq!(
        table_names(&connection).await,
        ["database", "property", "row"]
    );
    assert_eq!(db::schema_version(&connection).await.unwrap(), 1);
    assert!(home.path().join("tansu.sqlite").exists());
}

#[tokio::test]
async fn open_twice_does_not_reapply_migrations() {
    let home = tempfile::tempdir().unwrap();
    let settings = settings_under(&home);

    db::open(&settings).await.unwrap();
    let connection = db::open(&settings).await.unwrap();

    assert_eq!(
        table_names(&connection).await,
        ["database", "property", "row"]
    );
    assert_eq!(db::schema_version(&connection).await.unwrap(), 1);
}

#[tokio::test]
async fn open_enables_wal_mode() {
    let home = tempfile::tempdir().unwrap();
    let connection = db::open(&settings_under(&home)).await.unwrap();

    let mut rows = connection.query("PRAGMA journal_mode", ()).await.unwrap();
    let row = rows.next().await.unwrap().unwrap();

    assert_eq!(row.get::<String>(0).unwrap(), "wal");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_opens_apply_the_migration_once() {
    let home = tempfile::tempdir().unwrap();
    let settings = settings_under(&home);

    let (first, second) = tokio::join!(
        tokio::spawn({
            let settings = settings.clone();
            async move { db::open(&settings).await }
        }),
        tokio::spawn({
            let settings = settings.clone();
            async move { db::open(&settings).await }
        }),
    );
    first.unwrap().unwrap();
    second.unwrap().unwrap();

    let connection = db::open(&settings).await.unwrap();
    assert_eq!(
        table_names(&connection).await,
        ["database", "property", "row"]
    );
    assert_eq!(db::schema_version(&connection).await.unwrap(), 1);
}

#[tokio::test]
async fn file_url_decides_where_the_database_is_created() {
    let home = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let path = elsewhere.path().join("nested").join("custom.sqlite");
    let settings = settings_with_db_url(&home, Some(format!("file:{}", path.display())));

    db::open(&settings).await.unwrap();

    assert!(path.exists());
    assert!(!home.path().join("tansu.sqlite").exists());
}

#[tokio::test]
async fn open_refuses_a_database_newer_than_the_binary() {
    let home = tempfile::tempdir().unwrap();
    let settings = settings_under(&home);
    let connection = db::open(&settings).await.unwrap();
    connection
        .execute("PRAGMA user_version = 99", ())
        .await
        .unwrap();

    let error = db::open(&settings).await.unwrap_err();

    assert!(matches!(
        error,
        Error::SchemaNewerThanBinary {
            db: 99,
            binary: 1
        }
    ));
}

#[test]
fn empty_file_url_is_an_error() {
    let home = tempfile::tempdir().unwrap();
    let result = Settings::resolve(EnvVars {
        tansu_home: Some(home.path().to_str().unwrap().into()),
        db_url: Some("file:".into()),
        ..EnvVars::default()
    });

    assert!(matches!(result, Err(Error::UnsupportedDbUrl(_))));
}

#[test]
fn libsql_url_without_a_token_is_an_error() {
    let home = tempfile::tempdir().unwrap();
    let without_token = Settings::resolve(EnvVars {
        tansu_home: Some(home.path().to_str().unwrap().into()),
        db_url: Some("libsql://example.turso.io".into()),
        ..EnvVars::default()
    });
    let with_empty_token = Settings::resolve(EnvVars {
        tansu_home: Some(home.path().to_str().unwrap().into()),
        db_url: Some("libsql://example.turso.io".into()),
        db_auth_token: Some(AuthToken::new("")),
        ..EnvVars::default()
    });

    assert!(matches!(without_token, Err(Error::MissingAuthToken)));
    assert!(matches!(with_empty_token, Err(Error::MissingAuthToken)));
}
