//! tansu のコア。データモデル・設定の解決・DB の読み書き・CLI と MCP の入出力の形を持つ。
//! CLI・MCP・HTTP API の入口はここの関数を呼ぶだけにし、SQL はこの crate の `db` モジュールにだけ書く
//! (`.claude/rules/sqlite-database.md`)。

pub mod config;
pub mod db;
pub mod error;
pub mod export;
pub mod model;
pub mod named_json;
pub mod qa;
pub mod usage;

pub use error::Error;
