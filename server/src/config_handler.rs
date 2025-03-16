use config;
use config::ConfigError;
use common::types::{AurPackageSettings, GitPackageSettings};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    pub aur_packages: Vec<AurPackageSettings>,
    pub git_packages: Vec<GitPackageSettings>,
}

impl Config {
    pub fn new(file: Option<String>) -> Result<Config, ConfigError> {
        let mut settings_builder = config::Config::builder()
            .add_source(config::Environment::with_prefix("AB")
            );
        if let Some(filename) = file {
            settings_builder = settings_builder.add_source(config::File::with_name(&filename));
        }
        let settings = settings_builder.build()?;

        let settings_deserialized: Config = settings.try_deserialize()?;

        Ok(settings_deserialized)
    }

}