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
    pub sleepduration: Option<u64>
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
    pub builder_tag: Option<String>,
    pub gitea: GiteaSettings,
}

impl Configurable for WorkerConfig {}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_config(content: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::Builder::new()
            .suffix(".yaml")
            .tempfile()
            .unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file
    }

    #[test]
    fn test_server_config_from_yaml() {
        let yaml = r#"
aur_packages:
  - name: firefox-bin
    options: null
  - name: chromium
git_packages:
  - source: https://github.com/example/pkg
    subfolder: null
sleepduration: 300
"#;
        let file = write_temp_config(yaml);
        let path = file.path().to_str().unwrap().to_string();
        let config = ServerConfig::new(Some(path)).unwrap();
        assert_eq!(config.aur_packages.len(), 2);
        assert_eq!(config.aur_packages[0].name, "firefox-bin");
        assert_eq!(config.aur_packages[1].name, "chromium");
        assert_eq!(config.git_packages.len(), 1);
        assert_eq!(
            config.git_packages[0].source,
            "https://github.com/example/pkg"
        );
        assert_eq!(config.sleepduration, Some(300));
    }

    #[test]
    fn test_server_config_default_sleepduration() {
        let yaml = r#"
aur_packages: []
git_packages: []
"#;
        let file = write_temp_config(yaml);
        let path = file.path().to_str().unwrap().to_string();
        let config = ServerConfig::new(Some(path)).unwrap();
        assert!(config.sleepduration.is_none());
    }

    #[test]
    fn test_server_config_aur_package_with_env() {
        let yaml = r#"
aur_packages:
  - name: mypackage
    env:
      - name: MY_VAR
        value: my_value
git_packages: []
"#;
        let file = write_temp_config(yaml);
        let path = file.path().to_str().unwrap().to_string();
        let config = ServerConfig::new(Some(path)).unwrap();
        let pkg = &config.aur_packages[0];
        assert_eq!(pkg.name, "mypackage");
        let env = pkg.env.as_ref().unwrap();
        assert_eq!(env[0].name, "MY_VAR");
        assert_eq!(env[0].value, "my_value");
    }

    #[test]
    fn test_server_config_git_package_with_subfolder() {
        let yaml = r#"
aur_packages: []
git_packages:
  - source: https://github.com/example/multi-pkg
    subfolder: my-pkg
"#;
        let file = write_temp_config(yaml);
        let path = file.path().to_str().unwrap().to_string();
        let config = ServerConfig::new(Some(path)).unwrap();
        let pkg = &config.git_packages[0];
        assert_eq!(pkg.source, "https://github.com/example/multi-pkg");
        assert_eq!(pkg.subfolder, Some("my-pkg".to_string()));
    }

    #[test]
    fn test_notifier_config_from_yaml() {
        let yaml = r#"
smtp:
  host: smtp.example.com
  user: user@example.com
  pass: secret123
  from: "AUR Builder <noreply@example.com>"
  to: "admin@example.com"
maillogo: https://example.com/logo.png
"#;
        let file = write_temp_config(yaml);
        let path = file.path().to_str().unwrap().to_string();
        let config = NotifierConfig::new(Some(path)).unwrap();
        assert_eq!(config.smtp.host, "smtp.example.com");
        assert_eq!(config.smtp.user, "user@example.com");
        assert_eq!(config.smtp.pass, "secret123");
        assert_eq!(config.maillogo, "https://example.com/logo.png");
    }

    #[test]
    fn test_worker_config_from_yaml() {
        let yaml = r#"
gitea:
  repo: my-repo
  user: builder-user
  token: abc123token
builder: ghcr.io/example/builder
builder_tag: v1.0.0
"#;
        let file = write_temp_config(yaml);
        let path = file.path().to_str().unwrap().to_string();
        let config = WorkerConfig::new(Some(path)).unwrap();
        assert_eq!(config.gitea.repo, "my-repo");
        assert_eq!(config.gitea.user, "builder-user");
        assert_eq!(config.gitea.token, "abc123token");
        assert_eq!(config.builder, Some("ghcr.io/example/builder".to_string()));
        assert_eq!(config.builder_tag, Some("v1.0.0".to_string()));
    }

    #[test]
    fn test_worker_config_optional_builder_fields() {
        let yaml = r#"
gitea:
  repo: some-repo
  user: some-user
  token: some-token
"#;
        let file = write_temp_config(yaml);
        let path = file.path().to_str().unwrap().to_string();
        let config = WorkerConfig::new(Some(path)).unwrap();
        assert!(config.builder.is_none());
        assert!(config.builder_tag.is_none());
    }

    #[test]
    fn test_server_config_invalid_file_returns_error() {
        let result = ServerConfig::new(Some("/nonexistent/path/config.yaml".to_string()));
        assert!(result.is_err());
    }
}
