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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<crate::artifact::Receipt>,
    pub task: BuildTaskTransmissionFormat,
    pub status_code: i64,
    pub log_lines: Vec<String>,
    pub success: bool,
    pub timestamps: Timestamps,
}

impl BuildResultTransmissionFormat {
    /// Notification consumers need the result summary, not build credentials or output.
    pub fn notification_summary(&self) -> Self {
        let mut summary = self.clone();
        summary.task.env = None;
        summary.task.source = None;
        summary.task.options = None;
        summary.task.subfolder = None;
        summary.task.upload = None;
        summary.artifact = None;
        summary.log_lines.clear();
        summary
    }
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

#[cfg(test)]
mod notification_tests {
    #[test]
    fn summary_does_not_disclose_build_secrets_or_receipts() {
        let result: super::BuildResultTransmissionFormat = serde_json::from_value(serde_json::json!({
            "task":{"id":1,"attempt":"a","object_key":"builds/a/bundle.tar","name":"fixture","version":"1","source":"https://user:password@host/source","subfolder":"private","options":"private-options","env":[{"name":"AB_PKG_TOKEN","value":"password"}]},
            "artifact":{"sha256":"digest","bytes":1,"mac":"worker-proof"},"status_code":0,"success":true,"log_lines":["password"],"timestamps":{"start":"2026-11-02T10:00:00","end":"2026-11-02T10:01:00"}
        })).unwrap();
        let summary = serde_json::to_string(&result.notification_summary()).unwrap();
        assert!(!summary.contains("password"));
        assert!(!summary.contains("worker-proof"));
        assert!(!summary.contains("private-options"));
        assert!(result.task.env.is_some());
        assert!(result.artifact.is_some());
    }
}
