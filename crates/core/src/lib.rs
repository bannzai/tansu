//! tansu のコア。データモデル・設定の解決・DB を開く処理を持つ。
//! CLI・MCP・HTTP API の入口はここの関数を呼ぶだけにし、SQL はこの crate の `db` モジュールにだけ書く
//! (`.claude/rules/sqlite-database.md`)。

pub mod config;
pub mod db;
pub mod error;
pub mod model;
pub mod qa;

pub use error::Error;
