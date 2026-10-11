//! QA 層の型 (`documents/data-model.md`「QA 層」)。`qa import` が最初に作る 4 つのデータベースの [`crate::model::Row`] を、
//! この形に読み替える。union のプロパティは汎用層に無いので、`automation` は select + text の 2 列に平たくしている。

use serde::{Deserialize, Serialize};

use crate::model::Id;

/// project データベースの行。リポジトリ。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QaProject {
    /// `owner/repo`。unique。
    pub repo: String,
    /// ローカルの checkout のパス。
    pub path: Option<String>,
}

/// feature データベースの行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QaFeature {
    pub project: Id,
    pub name: String,
    /// QA.md の `verification` (動作確認手段)。
    pub verification: String,
}

/// item データベースの行。QA の 1 項目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QaItem {
    pub feature: Id,
    pub name: String,
    pub expected: String,
    /// select: auto / manual / todo。
    pub automation: QaAutomation,
    /// auto の flow のパス、manual の理由。
    pub automation_detail: Option<String>,
    /// multi_select。S1, S2, ...
    pub spec_ids: Vec<String>,
    /// multi_select。
    pub labels: Vec<String>,
}

/// 項目の自動化ステータス。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QaAutomation {
    Auto,
    Manual,
    Todo,
}

/// run データベースの行。QA を実行した事実の記録。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QaRun {
    pub item: Id,
    /// select: ok / ng / later / skip。
    pub result: QaResult,
    pub note: Option<String>,
    /// 時差付き ISO 8601。import は `**確認日:**` の日付。
    pub recorded_at: String,
    /// select: record / import。
    pub source: QaSource,
    pub image_url: Option<String>,
    pub env: Option<String>,
    /// full SHA。
    pub commit: Option<String>,
    /// unique。呼び出し側が渡す冪等キー。
    pub run_id: Option<String>,
}

/// run の結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QaResult {
    Ok,
    Ng,
    Later,
    Skip,
}

/// run の記録の経路。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QaSource {
    Record,
    Import,
}
