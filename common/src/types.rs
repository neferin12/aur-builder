use sea_orm::sqlx::types::chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct PackageSearchResult {
    pub name: String,
    pub version: String,
    pub maintainer: String,
    pub last_modified: i64,
    pub source: Option<String>,
    pub subfolder: Option<String>,
    pub options: Option<String>,
    pub environment: Option<Environment>
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
    pub id: i32,
    pub name: String,
    pub version: String,
    pub source: Option<String>,
    pub subfolder: Option<String>,
    pub options: Option<String>,
    pub env: Option<Environment>
}

type Environment = Vec<EnvironmentVariable>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EnvironmentVariable {
    pub name: String,
    pub value: String
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AurPackageSettings {
    pub name: String,
    pub env: Option<Environment>,
    pub options: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GitPackageSettings {
    pub source: String,
    pub subfolder: Option<String>,
    pub env: Option<Environment>,
    pub options: Option<String>,
}