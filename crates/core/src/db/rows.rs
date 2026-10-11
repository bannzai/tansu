//! 利用者の表の行 (`row` テーブル) を読み書きする。値は `row.values` の JSON 1 列に持ち、絞り込みは `json_extract` で見る
//! (`documents/data-model.md`「SQLite / libSQL への持ち方」)。

use std::collections::{BTreeMap, BTreeSet};

use libsql::Connection;

use super::{begin, finish, now, schema, value_path};
use crate::error::Error;
use crate::model::{Database, Id, Property, PropertyKind, Row, Value};
use crate::named_json::{self, RowInput};

/// `SELECT` で [`row_from_sql`] が読む列の並び。
const ROW_COLUMNS: &str = r#"id, database_id, body, "values", created_at, updated_at"#;

/// データベース `database_name` に行を書き、書いた後の行を返す。`input` は [`named_json::parse_row_input`] の形。
///
/// 書く行は `id` があればその行 (無ければその `id` で足す)、`id` が無ければ unique のプロパティの値が一致する行
/// (無ければ新しい ID で足す) に決める。`id` も unique のプロパティの値も無ければ [`Error::InvalidInput`] にする
/// (同じ入力を 2 回書いても行が増えないように、行を決めるキーを必ず求める)。
/// 既存の行には、`values` に書いたプロパティだけを上書きし (null は値を消す)、`body` は書いた時だけ置き換える。
/// 結果が既存の行と同じなら何も書かない (`updated_at` も変えない)。
pub async fn upsert(
    connection: &Connection,
    database_name: &str,
    input: &serde_json::Value,
) -> Result<(Database, Row), Error> {
    let transaction = begin(connection).await?;
    let result = upsert_in(&transaction, database_name, input).await;
    finish(transaction, result).await
}

/// [`upsert`] の本体。`connection` は書き込みロックを取ったトランザクション。
async fn upsert_in(
    connection: &Connection,
    database_name: &str,
    input: &serde_json::Value,
) -> Result<(Database, Row), Error> {
    let database = schema::get_database(connection, database_name).await?;
    let request = named_json::parse_row_input(&database, input)?;
    check_relation_targets(connection, &database, &request).await?;
    let rows = rows_of_database(connection, &database.id).await?;
    let existing = match &request.id {
        Some(row_id) => {
            if let Some(other_database_id) = database_of_row(connection, row_id).await?
                && other_database_id != database.id
            {
                return Err(Error::InvalidInput(format!(
                    "行 `{}` は別のデータベースの行です",
                    row_id.0
                )));
            }
            rows.iter().find(|row| row.id == *row_id)
        }
        None => {
            let unique_values = unique_values(&database, &request.values);
            if unique_values.is_empty() {
                return Err(Error::InvalidInput(
                    "書く行を決められません。`id` を渡すか、unique のプロパティの値を渡してください"
                        .into(),
                ));
            }
            rows.iter().find(|row| {
                unique_values
                    .iter()
                    .any(|(property_id, value)| {
                        row.values
                            .get(*property_id)
                            .is_some_and(|stored| same_unique_value(stored, value))
                    })
            })
        }
    };
    let mut values = existing.map(|row| row.values.clone()).unwrap_or_default();
    for (property_id, value) in request.values {
        match value {
            Some(value) => values.insert(property_id, value),
            None => values.remove(&property_id),
        };
    }
    let row_id = existing
        .map(|row| row.id.clone())
        .or(request.id)
        .unwrap_or_else(Id::generate);
    let body = request
        .body
        .or_else(|| existing.map(|row| row.body.clone()))
        .unwrap_or_default();
    check_unique(&database, &rows, &row_id, &values)?;
    let row = match existing {
        Some(existing) if existing.values == values && existing.body == body => existing.clone(),
        Some(existing) => {
            let updated_at = now();
            connection
                .execute(
                    r#"UPDATE "row" SET body = ?, "values" = ?, updated_at = ? WHERE id = ?"#,
                    (
                        body.as_str(),
                        serde_json::to_string(&values)?,
                        updated_at.as_str(),
                        row_id.0.as_str(),
                    ),
                )
                .await?;
            Row {
                body,
                values,
                updated_at,
                ..existing.clone()
            }
        }
        None => {
            let created_at = now();
            connection
                .execute(
                    &format!(r#"INSERT INTO "row" ({ROW_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?)"#),
                    (
                        row_id.0.as_str(),
                        database.id.0.as_str(),
                        body.as_str(),
                        serde_json::to_string(&values)?,
                        created_at.as_str(),
                        created_at.as_str(),
                    ),
                )
                .await?;
            Row {
                id: row_id,
                database_id: database.id.clone(),
                body,
                values,
                updated_at: created_at.clone(),
                created_at,
            }
        }
    };
    Ok((database, row))
}

/// relation の値が指す行が、相手の表に実在するかを確かめる。
async fn check_relation_targets(
    connection: &Connection,
    database: &Database,
    request: &RowInput,
) -> Result<(), Error> {
    for property in &database.properties {
        let (PropertyKind::Relation { target_database_id }, Some(Some(Value::Relation(row_ids)))) =
            (&property.kind, request.values.get(&property.id))
        else {
            continue;
        };
        for row_id in row_ids {
            if database_of_row(connection, row_id).await?.as_ref() != Some(target_database_id) {
                return Err(Error::InvalidInput(format!(
                    "relation のプロパティ `{}` の相手の表に行 `{}` はありません",
                    property.name, row_id.0
                )));
            }
        }
    }
    Ok(())
}

/// 書く値 `values` のうち、unique のプロパティの値 (行を決めるキーになる値) を (プロパティの ID, 値) で返す。
fn unique_values<'a>(
    database: &'a Database,
    values: &'a BTreeMap<Id, Option<Value>>,
) -> Vec<(&'a Id, &'a Value)> {
    database
        .properties
        .iter()
        .filter(|property| property.unique)
        .filter_map(|property| match values.get(&property.id) {
            Some(Some(value)) => Some((&property.id, value)),
            _ => None,
        })
        .collect()
}

/// 行 `row_id` に `values` を書いた時、unique のプロパティの値がほかの行と重ならないかを確かめる。
fn check_unique(
    database: &Database,
    rows: &[Row],
    row_id: &Id,
    values: &BTreeMap<Id, Value>,
) -> Result<(), Error> {
    for property in database
        .properties
        .iter()
        .filter(|property| property.unique)
    {
        let Some(value) = values.get(&property.id) else {
            continue;
        };
        if let Some(duplicate) = rows
            .iter()
            .find(|row| {
                row.id != *row_id
                    && row
                        .values
                        .get(&property.id)
                        .is_some_and(|stored| same_unique_value(stored, value))
            })
        {
            return Err(Error::UniqueViolation {
                property: property.name.clone(),
                row_id: duplicate.id.0.clone(),
            });
        }
    }
    Ok(())
}

/// データベース `database_name` の行 `row_id` を消す。行が無ければ何もしない (2 回実行しても同じ結果)。
/// relation でこの行を参照している値からも、この行の ID を外す (外して空になった値はキーごと消す)。
pub async fn delete(
    connection: &Connection,
    database_name: &str,
    row_id: &str,
) -> Result<(), Error> {
    let transaction = begin(connection).await?;
    let result = delete_in(&transaction, database_name, row_id).await;
    finish(transaction, result).await
}

/// [`delete`] の本体。`connection` は書き込みロックを取ったトランザクション。
async fn delete_in(
    connection: &Connection,
    database_name: &str,
    row_id: &str,
) -> Result<(), Error> {
    let database = schema::get_database(connection, database_name).await?;
    let deleted = connection
        .execute(
            r#"DELETE FROM "row" WHERE id = ? AND database_id = ?"#,
            (row_id, database.id.0.as_str()),
        )
        .await?;
    if deleted == 0 {
        // 行が無いのは 2 回目の delete と同じ状態なので成功にする。別のデータベースに実在する ID は指定の誤りなのでエラーにする
        if database_of_row(connection, &Id(row_id.into())).await?.is_some() {
            return Err(Error::InvalidInput(format!(
                "行 `{row_id}` は別のデータベースの行です"
            )));
        }
        return Ok(());
    }
    for (referring_database_id, property) in
        relation_properties_targeting(connection, &database.id).await?
    {
        let property_id = &property.id;
        let mut rows = connection
            .query(
                &format!(
                    r#"SELECT {ROW_COLUMNS} FROM "row" WHERE database_id = ? AND EXISTS (SELECT 1 FROM json_each("row"."values", ?) WHERE json_each.value = ?)"#
                ),
                (
                    referring_database_id.0.as_str(),
                    value_path(property_id),
                    row_id,
                ),
            )
            .await?;
        let mut referring_rows = Vec::new();
        while let Some(row) = rows.next().await? {
            referring_rows.push(row_from_sql(&row)?);
        }
        drop(rows);
        for mut referring_row in referring_rows {
            if let Some(Value::Relation(row_ids)) = referring_row.values.get_mut(property_id) {
                row_ids.retain(|id| id.0 != row_id);
                if row_ids.is_empty() {
                    referring_row.values.remove(property_id);
                }
            }
            connection
                .execute(
                    r#"UPDATE "row" SET "values" = ?, updated_at = ? WHERE id = ?"#,
                    (
                        serde_json::to_string(&referring_row.values)?,
                        now(),
                        referring_row.id.0.as_str(),
                    ),
                )
                .await?;
        }
        // 参照を外すと、unique の relation で別々の行の値が同じになり得る (["p1", "p2"] と ["p2"] から p1 を外す等)。
        // その時は削除ごと巻き戻す
        if property.unique
            && let Some(duplicate) = duplicate_row(
                &rows_of_database(connection, &referring_database_id).await?,
                property_id,
            )
        {
            return Err(Error::UniqueViolation {
                property: property.name.clone(),
                row_id: duplicate.id.0.clone(),
            });
        }
    }
    Ok(())
}

/// 相手の表が `target_database_id` の relation のプロパティを、(そのプロパティを持つデータベースの ID, プロパティ) で返す。
async fn relation_properties_targeting(
    connection: &Connection,
    target_database_id: &Id,
) -> Result<Vec<(Id, Property)>, Error> {
    let mut rows = connection
        .query(
            "SELECT database_id, id, name, kind, unique_value FROM property",
            (),
        )
        .await?;
    let mut properties = Vec::new();
    while let Some(row) = rows.next().await? {
        let kind = serde_json::from_str::<PropertyKind>(&row.get::<String>(3)?)?;
        if matches!(&kind, PropertyKind::Relation { target_database_id: target } if target == target_database_id)
        {
            properties.push((
                Id(row.get::<String>(0)?),
                Property {
                    id: Id(row.get::<String>(1)?),
                    name: row.get::<String>(2)?,
                    kind,
                    unique: row.get::<i64>(4)? != 0,
                },
            ));
        }
    }
    Ok(properties)
}

/// `rows` の中で、プロパティ `property_id` の値が先に出た行と同じ行 (unique の重複)。無ければ `None`。
pub(super) fn duplicate_row<'a>(rows: &'a [Row], property_id: &Id) -> Option<&'a Row> {
    rows.iter().enumerate().find_map(|(index, row)| {
        let value = row.values.get(property_id)?;
        rows[..index]
            .iter()
            .any(|earlier| {
                earlier
                    .values
                    .get(property_id)
                    .is_some_and(|earlier_value| same_unique_value(earlier_value, value))
            })
            .then_some(row)
    })
}

/// unique の判定で 2 つの値を同じとみなすか。multi_select / relation は要素の順序を問わない
/// (`["p1", "p2"]` と `["p2", "p1"]` は同じ組を指すため)。
fn same_unique_value(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::MultiSelect(left_ids), Value::MultiSelect(right_ids))
        | (Value::Relation(left_ids), Value::Relation(right_ids)) => {
            left_ids.iter().collect::<BTreeSet<_>>() == right_ids.iter().collect::<BTreeSet<_>>()
        }
        _ => left == right,
    }
}

/// データベース `database_name` の行を絞り込んで返す。行の解釈に使った定義も返す。
///
/// - `filters`: `<プロパティ名>=<値>` (等価) か `<プロパティ名>^=<値>` (前方一致。text / url / date だけ)。複数は AND。
///   select は選択肢の name、checkbox は `true` / `false` で書く。multi_select と relation は、その選択肢の name・行 ID を含む行に一致する
/// - `sort`: `<プロパティ名>[:asc|:desc]`。text / number / checkbox / url / date だけ。無ければ追加した順
/// - `limit`: 返す行の上限。無ければすべて
pub async fn query(
    connection: &Connection,
    database_name: &str,
    filters: &[String],
    sort: Option<&str>,
    limit: Option<u64>,
) -> Result<(Database, Vec<Row>), Error> {
    // 定義と行を同じスナップショットで読む (間に別プロセスがプロパティを足すと、新しい値を古い定義で解釈して落とすため)。
    // 読むだけなので書き込みロックは取らない
    let transaction = connection.transaction().await?;
    let result = query_in(&transaction, database_name, filters, sort, limit).await;
    finish(transaction, result).await
}

/// [`query`] の本体。`connection` は読み取りのトランザクション。
async fn query_in(
    connection: &Connection,
    database_name: &str,
    filters: &[String],
    sort: Option<&str>,
    limit: Option<u64>,
) -> Result<(Database, Vec<Row>), Error> {
    let database = schema::get_database(connection, database_name).await?;
    let mut sql = format!(r#"SELECT {ROW_COLUMNS} FROM "row" WHERE database_id = ?"#);
    let mut params = vec![libsql::Value::from(database.id.0.clone())];
    for filter in filters {
        let (left, raw_value) = filter.split_once('=').ok_or_else(|| {
            Error::InvalidInput(format!(
                "`{filter}` は解釈できません (`<プロパティ名>=<値>` か `<プロパティ名>^=<値>` の形で指定してください)"
            ))
        })?;
        let (property_name, prefix) = match left.strip_suffix('^') {
            Some(property_name) => (property_name, true),
            None => (left, false),
        };
        let property = property_of(&database, property_name)?;
        let path = value_path(&property.id);
        let invalid_value = |expected: &str| {
            Error::InvalidInput(format!(
                "プロパティ `{property_name}` ({}) の絞り込みの値 `{raw_value}` は{expected}ではありません",
                property.kind.type_name()
            ))
        };
        let (condition, value) = match (&property.kind, prefix) {
            (PropertyKind::Text | PropertyKind::Url, false) => (
                r#"json_extract("values", ?) = ?"#,
                libsql::Value::from(raw_value),
            ),
            (PropertyKind::Text | PropertyKind::Url, true) => (
                r#"instr(json_extract("values", ?), ?) = 1"#,
                libsql::Value::from(raw_value),
            ),
            // date は保存時に書き方を揃えている (named_json::normalize_date) ため、検索の値も同じく揃えて比べる。
            // 前方一致の値 (`2026-10` 等) は日付として読めないので、区切りの大文字化だけを揃える
            (PropertyKind::Date, false) => (
                r#"json_extract("values", ?) = ?"#,
                libsql::Value::from(
                    named_json::normalize_date(raw_value)
                        .unwrap_or_else(|| raw_value.to_ascii_uppercase()),
                ),
            ),
            (PropertyKind::Date, true) => (
                r#"instr(json_extract("values", ?), ?) = 1"#,
                libsql::Value::from(raw_value.to_ascii_uppercase()),
            ),
            (PropertyKind::Number, false) => (
                r#"json_extract("values", ?) = ?"#,
                libsql::Value::from(
                    raw_value
                        .parse::<f64>()
                        .map_err(|_| invalid_value("数値"))?,
                ),
            ),
            (PropertyKind::Checkbox, false) => (
                r#"json_extract("values", ?) = ?"#,
                libsql::Value::from(
                    raw_value
                        .parse::<bool>()
                        .map_err(|_| invalid_value("true / false"))?,
                ),
            ),
            (PropertyKind::Select { options }, false) => (
                r#"json_extract("values", ?) = ?"#,
                libsql::Value::from(named_json::option_id(property, options, raw_value)?.0),
            ),
            (PropertyKind::MultiSelect { options }, false) => (
                r#"EXISTS (SELECT 1 FROM json_each("row"."values", ?) WHERE json_each.value = ?)"#,
                libsql::Value::from(named_json::option_id(property, options, raw_value)?.0),
            ),
            (PropertyKind::Relation { .. }, false) => (
                r#"EXISTS (SELECT 1 FROM json_each("row"."values", ?) WHERE json_each.value = ?)"#,
                libsql::Value::from(raw_value),
            ),
            (_, true) => {
                return Err(Error::InvalidInput(format!(
                    "前方一致 (^=) は text / url / date のプロパティだけに使えます (`{property_name}` は {})",
                    property.kind.type_name()
                )));
            }
        };
        sql.push_str(" AND ");
        sql.push_str(condition);
        params.push(libsql::Value::from(path));
        params.push(value);
    }
    match sort {
        Some(sort) => {
            let (property_name, direction) = match sort.rsplit_once(':') {
                Some((property_name, "desc")) => (property_name, "DESC"),
                Some((property_name, "asc")) => (property_name, "ASC"),
                _ => (sort, "ASC"),
            };
            let property = property_of(&database, property_name)?;
            if !matches!(
                property.kind,
                PropertyKind::Text
                    | PropertyKind::Number
                    | PropertyKind::Checkbox
                    | PropertyKind::Url
                    | PropertyKind::Date
            ) {
                return Err(Error::InvalidInput(format!(
                    "並べ替えは text / number / checkbox / url / date のプロパティだけに使えます (`{property_name}` は {})",
                    property.kind.type_name()
                )));
            }
            // date は時差付きの日時を含むため、文字列ではなく時刻 (julianday は時差を UTC に直す) で比べる
            let sort_key = match property.kind {
                PropertyKind::Date => r#"julianday(json_extract("values", ?))"#,
                _ => r#"json_extract("values", ?)"#,
            };
            // 値の無い行は昇順でも降順でも最後に置く (SQLite は昇順で NULL を先頭にするため、`--limit` で値のある行を取りこぼす)
            sql.push_str(&format!(
                " ORDER BY {sort_key} IS NULL, {sort_key} {direction}, rowid"
            ));
            params.push(libsql::Value::from(value_path(&property.id)));
            params.push(libsql::Value::from(value_path(&property.id)));
        }
        None => sql.push_str(" ORDER BY rowid"),
    }
    // SQLite の LIMIT は負の値で上限なしになる
    sql.push_str(" LIMIT ?");
    params.push(libsql::Value::from(match limit {
        Some(limit) => i64::try_from(limit)
            .map_err(|_| Error::InvalidInput(format!("--limit の {limit} は大きすぎます")))?,
        None => -1,
    }));
    let mut rows = connection.query(&sql, params).await?;
    let mut found = Vec::new();
    while let Some(row) = rows.next().await? {
        found.push(row_from_sql(&row)?);
    }
    Ok((database, found))
}

/// `database` の名前が `property_name` のプロパティ。無ければ [`Error::PropertyNotFound`]。
fn property_of<'a>(database: &'a Database, property_name: &str) -> Result<&'a Property, Error> {
    database
        .property(property_name)
        .ok_or_else(|| Error::PropertyNotFound {
            database: database.name.clone(),
            property: property_name.into(),
        })
}

/// データベース `database_id` のすべての行を追加した順に返す。
pub(super) async fn rows_of_database(
    connection: &Connection,
    database_id: &Id,
) -> Result<Vec<Row>, Error> {
    let mut rows = connection
        .query(
            &format!(r#"SELECT {ROW_COLUMNS} FROM "row" WHERE database_id = ? ORDER BY rowid"#),
            [database_id.0.as_str()],
        )
        .await?;
    let mut found = Vec::new();
    while let Some(row) = rows.next().await? {
        found.push(row_from_sql(&row)?);
    }
    Ok(found)
}

/// 行 `row_id` が属するデータベースの ID。行が無ければ `None`。
async fn database_of_row(connection: &Connection, row_id: &Id) -> Result<Option<Id>, Error> {
    let mut rows = connection
        .query(
            r#"SELECT database_id FROM "row" WHERE id = ?"#,
            [row_id.0.as_str()],
        )
        .await?;
    match rows.next().await? {
        Some(row) => Ok(Some(Id(row.get::<String>(0)?))),
        None => Ok(None),
    }
}

/// [`ROW_COLUMNS`] の並びで読んだ 1 行を [`Row`] にする。
fn row_from_sql(row: &libsql::Row) -> Result<Row, Error> {
    Ok(Row {
        id: Id(row.get::<String>(0)?),
        database_id: Id(row.get::<String>(1)?),
        body: row.get::<String>(2)?,
        values: serde_json::from_str(&row.get::<String>(3)?)?,
        created_at: row.get::<String>(4)?,
        updated_at: row.get::<String>(5)?,
    })
}
