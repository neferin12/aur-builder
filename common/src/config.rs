use crate::types::{AurPackageSettings, GitPackageSettings};
use config;
use config::{Config, ConfigError};
use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;

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
pub struct GiteaSettings {
    pub repo: String,
    pub user: String,
    pub token: String,
}


#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WorkerConfig {
    pub builder:  Option<String>,
    pub gitea: GiteaSettings,
}

impl Configurable for WorkerConfig {}