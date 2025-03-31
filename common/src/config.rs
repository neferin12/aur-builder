use crate::types::{AurPackageSettings, GitPackageSettings};
use config;
use config::{Config, ConfigError};
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerConfig {
    pub aur_packages: Vec<AurPackageSettings>,
    pub git_packages: Vec<GitPackageSettings>,
}

impl ServerConfig {
    pub fn new(file: Option<String>) -> Result<ServerConfig, ConfigError> {
        let settings_deserialized: ServerConfig =
            get_unserialized_settings(file)?.try_deserialize()?;

        Ok(settings_deserialized)
    }
}

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
}

impl NotifierConfig {
    pub fn new(file: Option<String>) -> Result<NotifierConfig, ConfigError> {
        let settings_deserialized: NotifierConfig =
            get_unserialized_settings(file)?.try_deserialize()?;

        Ok(settings_deserialized)
    }
}
