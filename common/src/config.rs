use crate::types::{AurPackageSettings, GitPackageSettings};
use config;
use config::{Config, ConfigError};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

fn get_unserialized_settings(file: Option<String>) -> Result<Config, ConfigError> {
    let mut settings_builder =
        config::Config::builder().add_source(config::Environment::with_prefix("AB").separator("_"));
    if let Some(filename) = file {
        settings_builder = settings_builder.add_source(config::File::with_name(&filename));
    }
    let settings = settings_builder.build()?;
    Ok(settings)
}

pub trait Configurable: DeserializeOwned {
    fn new(file: Option<String>) -> Result<Self, ConfigError> {
        // The default trait method
        let settings = get_unserialized_settings(file)?;
        let settings_deserialized: Self = settings.try_deserialize()?;
        Ok(settings_deserialized)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerConfig {
    pub aur_packages: Vec<AurPackageSettings>,
    pub git_packages: Vec<GitPackageSettings>,
    pub sleepduration: Option<u64>,
    #[serde(default = "default_keep_versions")]
    pub keep_versions: std::num::NonZeroUsize,
}

fn default_keep_versions() -> std::num::NonZeroUsize {
    std::num::NonZeroUsize::new(2).unwrap()
}

impl Configurable for ServerConfig {}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SmtpSettings {
    pub host: String,
    pub user: String,
    pub pass: String,
    pub from: String,
    pub to: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NotifierConfig {
    pub smtp: SmtpSettings,
    pub maillogo: String,
}

impl Configurable for NotifierConfig {}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WorkerConfig {
    pub builder: Option<String>,
    pub builder_tag: Option<String>,
}

impl Configurable for WorkerConfig {
    fn new(file: Option<String>) -> Result<Self, ConfigError> {
        // Worker field names contain underscores; only double underscores denote nesting.
        let mut builder = Config::builder().add_source(
            config::Environment::with_prefix("AB")
                .prefix_separator("_")
                .separator("__"),
        );
        if let Some(filename) = file {
            builder = builder.add_source(config::File::with_name(&filename));
        }
        builder.build()?.try_deserialize()
    }
}

#[cfg(test)]
mod worker_config_tests {
    use super::*;
    #[test]
    fn worker_image_environment_keeps_field_underscores() {
        let source = config::Environment::with_prefix("AB")
            .prefix_separator("_")
            .separator("__")
            .source(Some(
                [
                    ("AB_BUILDER".to_string(), "local-builder".to_string()),
                    ("AB_BUILDER_TAG".to_string(), "setup-local".to_string()),
                ]
                .into_iter()
                .collect(),
            ));
        let settings: WorkerConfig = Config::builder()
            .add_source(source)
            .build()
            .unwrap()
            .try_deserialize()
            .unwrap();
        assert_eq!(settings.builder.as_deref(), Some("local-builder"));
        assert_eq!(settings.builder_tag.as_deref(), Some("setup-local"));
    }
}

/// Repository identity safe to display to viewers; credentials and query values stay private.
pub fn public_source(source: &str) -> String {
    if let Ok(mut url) = url::Url::parse(source) {
        let hidden_query = url.query().is_some();
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_query(None);
        url.set_fragment(None);
        let mut display = url.to_string();
        if hidden_query {
            display.push_str("?[redacted]");
        }
        display
    } else if let Some((_, host_path)) = source.split_once('@') {
        format!("[redacted]@{host_path}")
    } else {
        "Configured repository".into()
    }
}
#[cfg(test)]
mod source_redaction_tests {
    use super::*;
    #[test]
    fn source_display_hides_url_authentication() {
        assert_eq!(
            public_source("https://user:token@example.com/pkg.git?access_token=secret#fragment"),
            "https://example.com/pkg.git?[redacted]"
        );
        assert_eq!(
            public_source("git@example.com:packages/pkg.git"),
            "[redacted]@example.com:packages/pkg.git"
        );
        assert_eq!(
            public_source("https://example.com/pkg.git"),
            "https://example.com/pkg.git"
        );
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    #[test]
    fn retention_configuration() {
        for (value, expected) in [
            ("", Some(2)),
            (",\"keep_versions\":1", Some(1)),
            (",\"keep_versions\":0", None),
        ] {
            let raw = format!("{{\"aur_packages\":[],\"git_packages\":[]{value}}}");
            let parsed = serde_json::from_str::<ServerConfig>(&raw);
            assert_eq!(parsed.ok().map(|c| c.keep_versions.get()), expected);
        }
    }
}
