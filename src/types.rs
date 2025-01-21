use sea_orm::sqlx::types::chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct AurRequestResult {
    pub id: i64,
    pub name: String,
    pub version: String,
    pub maintainer: String,
    pub last_modified: i64
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BuildResultTransmissionFormat {
    pub task: BuildTaskTransmissionFormat,
    pub status_code: i64,
    pub log_lines: Vec<String>,
    pub success: bool,
    pub timestamps: Timestamps
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Timestamps {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BuildTaskTransmissionFormat {
    pub id: i64,
    pub name: String,
    pub version: String
}