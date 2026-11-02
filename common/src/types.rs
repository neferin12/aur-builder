use sea_orm::sqlx::types::chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageSearchResult {
    pub name: String,
    pub version: String,
    pub maintainer: String,
    pub last_modified: i64,
    pub source: Option<String>,
    pub subfolder: Option<String>,
    pub options: Option<String>,
    pub environment: Option<Environment>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BuildResultTransmissionFormat {
    pub task: BuildTaskTransmissionFormat,
    pub status_code: i64,
    pub log_lines: Vec<String>,
    pub success: bool,
    pub timestamps: Timestamps,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Timestamps {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BuildTaskTransmissionFormat {
    pub id: i32,
    #[serde(default)]
    pub candidate_id: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
    pub attempt: String,
    pub object_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload: Option<UploadGrant>,
    pub name: String,
    pub version: String,
    pub source: Option<String>,
    pub subfolder: Option<String>,
    pub options: Option<String>,
    pub env: Option<Environment>,
}

type Environment = Vec<EnvironmentVariable>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EnvironmentVariable {
    pub name: String,
    pub value: String,
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
#[derive(Serialize, Deserialize, Clone)]
pub struct UploadGrant {
    pub url: String,
    pub expires_at: i64,
}
impl std::fmt::Debug for UploadGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UploadGrant([redacted])")
    }
}

/// Review alerts share the notification queue with legacy build-result messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReviewNotification {
    ReviewRequired {
        candidate_id: String,
        package_name: String,
        version: String,
        revision: String,
        review_url: String,
        reason: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Notification {
    Review(ReviewNotification),
    Build(BuildResultTransmissionFormat),
}
