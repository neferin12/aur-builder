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
