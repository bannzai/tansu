//! 利用者のデータベースとプロパティの定義 (メタスキーマの `database` / `property` テーブル) を読み書きする。
//! 利用者の操作で DDL は実行せず、定義はデータとして持つ (`.claude/rules/sqlite-database.md`)。

use libsql::Connection;

use super::{begin, finish, now, rows};
use crate::error::Error;
use crate::model::{Database, Id, Property, PropertyKind, SelectOption};

/// 名前が `name` のデータベースを作って返す。既にあれば作らずにそれを返す (同じ名前で 2 回実行しても 1 つ)。
pub async fn create_database(connection: &Connection, name: &str) -> Result<Database, Error> {
    if name.is_empty() {
        return Err(Error::InvalidInput("データベースの名前が空です".into()));
    }
    let transaction = begin(connection).await?;
    let result = create_database_in(&transaction, name).await;
    finish(transaction, result).await
}

/// [`create_database`] の本体。`connection` は書き込みロックを取ったトランザクション。
async fn create_database_in(connection: &Connection, name: &str) -> Result<Database, Error> {
    if let Some(database) = find_database(connection, name).await? {
        return Ok(database);
    }
    connection
        .execute(
            r#"INSERT INTO "database" (id, name, created_at) VALUES (?, ?, ?)"#,
            (Id::generate().0, name, now()),
        )
        .await?;
    get_database(connection, name).await
}

/// すべてのデータベースを名前順に返す。
pub async fn list_databases(connection: &Connection) -> Result<Vec<Database>, Error> {
    let mut names = Vec::new();
    let mut rows = connection
        .query(r#"SELECT name FROM "database" ORDER BY name"#, ())
        .await?;
    while let Some(row) = rows.next().await? {
        names.push(row.get::<String>(0)?);
    }
    drop(rows);
    let mut databases = Vec::with_capacity(names.len());
    for name in names {
        databases.push(get_database(connection, &name).await?);
    }
    Ok(databases)
}

/// 名前が `name` のデータベース。無ければ [`Error::DatabaseNotFound`]。
pub async fn get_database(connection: &Connection, name: &str) -> Result<Database, Error> {
    find_database(connection, name)
        .await?
        .ok_or_else(|| Error::DatabaseNotFound(name.into()))
}

/// 名前が `name` のデータベースを、プロパティの定義ごと読む。無ければ `None`。
async fn find_database(connection: &Connection, name: &str) -> Result<Option<Database>, Error> {
    let mut rows = connection
        .query(
            r#"SELECT id, created_at FROM "database" WHERE name = ?"#,
            [name],
        )
        .await?;
    let Some(row) = rows.next().await? else {
        return Ok(None);
    };
    let database_id = Id(row.get::<String>(0)?);
    let database_created_at = row.get::<String>(1)?;
    drop(rows);
    Ok(Some(Database {
        properties: properties_of(connection, &database_id).await?,
        id: database_id,
        name: name.into(),
        created_at: database_created_at,
    }))
}

/// データベース `database_id` のプロパティを `position` 順に返す。
async fn properties_of(connection: &Connection, database_id: &Id) -> Result<Vec<Property>, Error> {
    let mut rows = connection
        .query(
            "SELECT id, name, kind, unique_value FROM property WHERE database_id = ? ORDER BY position",
            [database_id.0.as_str()],
        )
        .await?;
    let mut properties = Vec::new();
    while let Some(row) = rows.next().await? {
        properties.push(Property {
            id: Id(row.get::<String>(0)?),
            name: row.get::<String>(1)?,
            kind: serde_json::from_str(&row.get::<String>(2)?)?,
            unique: row.get::<i64>(3)? != 0,
        });
    }
    Ok(properties)
}

/// データベース `database_name` の定義を変えて、変えた後の定義を返す。1 つのトランザクションで、
/// `add` → `unique` → `remove` の順に当てる。どれも同じ引数で 2 回実行しても結果が変わらない。
///
/// - `add`: `<プロパティ名>:<型>[:<選択肢,...>|:<相手の表>]`。型は [`PropertyKind::type_name`] の名前。
///   同じ名前・同じ型のプロパティが既にあれば、select / multi_select は無い選択肢だけを足し、ほかは何もしない。
///   同じ名前で型か relation の相手が違えば [`Error::InvalidInput`]
/// - `unique`: プロパティ名。既に同じ値の行が 2 つあれば [`Error::UniqueViolation`]
/// - `remove`: プロパティ名。無ければ何もしない。行の値からもそのプロパティを消す
pub async fn change_schema(
    connection: &Connection,
    database_name: &str,
    add: &[String],
    unique: &[String],
    remove: &[String],
) -> Result<Database, Error> {
    let transaction = begin(connection).await?;
    let result = change_schema_in(&transaction, database_name, add, unique, remove).await;
    finish(transaction, result).await
}

/// [`change_schema`] の本体。`connection` は書き込みロックを取ったトランザクション。
async fn change_schema_in(
    connection: &Connection,
    database_name: &str,
    add: &[String],
    unique: &[String],
    remove: &[String],
) -> Result<Database, Error> {
    for spec in add {
        add_property(
            connection,
            &get_database(connection, database_name).await?,
            spec,
        )
        .await?;
    }
    for property_name in unique {
        set_unique(
            connection,
            &get_database(connection, database_name).await?,
            property_name,
        )
        .await?;
    }
    for property_name in remove {
        remove_property(
            connection,
            &get_database(connection, database_name).await?,
            property_name,
        )
        .await?;
    }
    get_database(connection, database_name).await
}

/// `database` に `spec` (`<プロパティ名>:<型>[:...]`) のプロパティを足す。規則は [`change_schema`] の `add`。
async fn add_property(
    connection: &Connection,
    database: &Database,
    spec: &str,
) -> Result<(), Error> {
    let mut parts = spec.splitn(3, ':');
    let (Some(property_name), Some(type_name)) =
        (parts.next().filter(|name| !name.is_empty()), parts.next())
    else {
        return Err(Error::InvalidInput(format!(
            "`{spec}` は解釈できません (`<プロパティ名>:<型>[:<選択肢,...>|:<相手の表>]` の形で指定してください)"
        )));
    };
    let kind = match (type_name, parts.next()) {
        ("text", None) => PropertyKind::Text,
        ("number", None) => PropertyKind::Number,
        ("checkbox", None) => PropertyKind::Checkbox,
        ("url", None) => PropertyKind::Url,
        ("date", None) => PropertyKind::Date,
        ("select", options) => PropertyKind::Select {
            options: new_options(options.unwrap_or_default()),
        },
        ("multi_select", options) => PropertyKind::MultiSelect {
            options: new_options(options.unwrap_or_default()),
        },
        ("relation", Some(target_name)) => PropertyKind::Relation {
            target_database_id: get_database(connection, target_name).await?.id,
        },
        ("relation", None) => {
            return Err(Error::InvalidInput(format!(
                "relation のプロパティ `{property_name}` には相手の表を `{property_name}:relation:<相手の表>` で指定してください"
            )));
        }
        ("text" | "number" | "checkbox" | "url" | "date", Some(_)) => {
            return Err(Error::InvalidInput(format!(
                "型 `{type_name}` は `:` の後ろの指定を取りません (`{spec}`)"
            )));
        }
        _ => {
            return Err(Error::InvalidInput(format!(
                "型 `{type_name}` はありません (text / number / checkbox / url / date / select / multi_select / relation)"
            )));
        }
    };
    let Some(existing) = database.property(property_name) else {
        connection
            .execute(
                "INSERT INTO property (id, database_id, name, kind, unique_value, position) \
                 VALUES (?, ?, ?, ?, 0, (SELECT COALESCE(MAX(position), -1) + 1 FROM property WHERE database_id = ?))",
                (
                    Id::generate().0,
                    database.id.0.as_str(),
                    property_name,
                    serde_json::to_string(&kind)?,
                    database.id.0.as_str(),
                ),
            )
            .await?;
        return Ok(());
    };
    if let Some(merged) = merge_kind(existing, kind)? {
        connection
            .execute(
                "UPDATE property SET kind = ? WHERE id = ?",
                (serde_json::to_string(&merged)?, existing.id.0.as_str()),
            )
            .await?;
    }
    Ok(())
}

/// 既存のプロパティ `existing` に、同じ名前で追加を指定された `requested` を合わせた型。変える必要が無ければ `None`。
fn merge_kind(existing: &Property, requested: PropertyKind) -> Result<Option<PropertyKind>, Error> {
    let mismatch = || {
        Error::InvalidInput(format!(
            "プロパティ `{}` は既に型 `{}` で定義されています",
            existing.name,
            existing.kind.type_name()
        ))
    };
    match (&existing.kind, requested) {
        (PropertyKind::Select { options }, PropertyKind::Select { options: requested }) => {
            Ok(merge_options(options, requested).map(|options| PropertyKind::Select { options }))
        }
        (
            PropertyKind::MultiSelect { options },
            PropertyKind::MultiSelect { options: requested },
        ) => {
            Ok(merge_options(options, requested)
                .map(|options| PropertyKind::MultiSelect { options }))
        }
        (
            PropertyKind::Relation { target_database_id },
            PropertyKind::Relation {
                target_database_id: requested,
            },
        ) => {
            if *target_database_id == requested {
                Ok(None)
            } else {
                Err(Error::InvalidInput(format!(
                    "relation のプロパティ `{}` は既に別の表を相手にしています",
                    existing.name
                )))
            }
        }
        (kind, requested) if kind.type_name() == requested.type_name() => Ok(None),
        _ => Err(mismatch()),
    }
}

/// `options` に無い name の選択肢を `requested` から足した一覧。足すものが無ければ `None`。
fn merge_options(
    options: &[SelectOption],
    requested: Vec<SelectOption>,
) -> Option<Vec<SelectOption>> {
    let added: Vec<SelectOption> = requested
        .into_iter()
        .filter(|option| options.iter().all(|existing| existing.name != option.name))
        .collect();
    if added.is_empty() {
        return None;
    }
    Some(options.iter().cloned().chain(added).collect())
}

/// `,` 区切りの選択肢の name から、新しい ID の選択肢を作る。空の name と重複は除く。
fn new_options(names: &str) -> Vec<SelectOption> {
    let mut options: Vec<SelectOption> = Vec::new();
    for name in names
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        if options.iter().all(|option| option.name != name) {
            options.push(SelectOption {
                id: Id::generate(),
                name: name.into(),
                color: None,
            });
        }
    }
    options
}

/// `database` のプロパティ `property_name` を unique にする。規則は [`change_schema`] の `unique`。
async fn set_unique(
    connection: &Connection,
    database: &Database,
    property_name: &str,
) -> Result<(), Error> {
    let property = database
        .property(property_name)
        .ok_or_else(|| Error::PropertyNotFound {
            database: database.name.clone(),
            property: property_name.into(),
        })?;
    if property.unique {
        return Ok(());
    }
    if let Some(duplicate) = rows::duplicate_row(
        &rows::rows_of_database(connection, &database.id).await?,
        &property.id,
    ) {
        return Err(Error::UniqueViolation {
            property: property.name.clone(),
            row_id: duplicate.id.0.clone(),
        });
    }
    connection
        .execute(
            "UPDATE property SET unique_value = 1 WHERE id = ?",
            [property.id.0.as_str()],
        )
        .await?;
    Ok(())
}

/// `database` のプロパティ `property_name` を外す。規則は [`change_schema`] の `remove`。
async fn remove_property(
    connection: &Connection,
    database: &Database,
    property_name: &str,
) -> Result<(), Error> {
    let Some(property) = database.property(property_name) else {
        return Ok(());
    };
    connection
        .execute(
            "DELETE FROM property WHERE id = ?",
            [property.id.0.as_str()],
        )
        .await?;
    connection
        .execute(
            r#"UPDATE "row" SET "values" = json_remove("values", ?) WHERE database_id = ?"#,
            (format!(r#"$."{}""#, property.id.0), database.id.0.as_str()),
        )
        .await?;
    Ok(())
}
