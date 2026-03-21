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
#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::sqlx::types::chrono::NaiveDateTime;

    fn make_naive_datetime(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn sample_task() -> BuildTaskTransmissionFormat {
        BuildTaskTransmissionFormat {
            id: 42,
            name: "test-package".to_string(),
            version: "2.3.1".to_string(),
            source: None,
            subfolder: None,
            options: None,
            env: None,
        }
    }

    #[test]
    fn test_build_task_serialization_roundtrip() {
        let task = sample_task();
        let json = serde_json::to_string(&task).unwrap();
        let deserialized: BuildTaskTransmissionFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, 42);
        assert_eq!(deserialized.name, "test-package");
        assert_eq!(deserialized.version, "2.3.1");
        assert!(deserialized.source.is_none());
        assert!(deserialized.env.is_none());
    }

    #[test]
    fn test_build_task_with_source_serialization_roundtrip() {
        let task = BuildTaskTransmissionFormat {
            id: 1,
            name: "git-package".to_string(),
            version: "1.0.0".to_string(),
            source: Some("https://github.com/example/repo".to_string()),
            subfolder: Some("pkg".to_string()),
            options: Some("--flag".to_string()),
            env: None,
        };
        let json = serde_json::to_string(&task).unwrap();
        let deserialized: BuildTaskTransmissionFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.source, Some("https://github.com/example/repo".to_string()));
        assert_eq!(deserialized.subfolder, Some("pkg".to_string()));
        assert_eq!(deserialized.options, Some("--flag".to_string()));
    }

    #[test]
    fn test_build_task_with_env_serialization_roundtrip() {
        let env = vec![
            EnvironmentVariable { name: "KEY1".to_string(), value: "val1".to_string() },
            EnvironmentVariable { name: "KEY2".to_string(), value: "val2".to_string() },
        ];
        let task = BuildTaskTransmissionFormat {
            id: 5,
            name: "pkg".to_string(),
            version: "0.1".to_string(),
            source: None,
            subfolder: None,
            options: None,
            env: Some(env),
        };
        let json = serde_json::to_string(&task).unwrap();
        let deserialized: BuildTaskTransmissionFormat = serde_json::from_str(&json).unwrap();
        let env_vars = deserialized.env.unwrap();
        assert_eq!(env_vars.len(), 2);
        assert_eq!(env_vars[0].name, "KEY1");
        assert_eq!(env_vars[0].value, "val1");
        assert_eq!(env_vars[1].name, "KEY2");
        assert_eq!(env_vars[1].value, "val2");
    }

    #[test]
    fn test_environment_variable_serialization_roundtrip() {
        let env_var = EnvironmentVariable {
            name: "MY_VAR".to_string(),
            value: "my_value".to_string(),
        };
        let json = serde_json::to_string(&env_var).unwrap();
        let deserialized: EnvironmentVariable = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "MY_VAR");
        assert_eq!(deserialized.value, "my_value");
    }

    #[test]
    fn test_build_result_transmission_format_success_roundtrip() {
        let start = make_naive_datetime("2024-06-01 10:00:00");
        let end = make_naive_datetime("2024-06-01 10:05:00");
        let result = BuildResultTransmissionFormat {
            task: sample_task(),
            status_code: 0,
            log_lines: vec!["stdout: Build started".to_string(), "stdout: Build done".to_string()],
            success: true,
            timestamps: Timestamps { start, end },
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: BuildResultTransmissionFormat = serde_json::from_str(&json).unwrap();
        assert!(deserialized.success);
        assert_eq!(deserialized.status_code, 0);
        assert_eq!(deserialized.log_lines.len(), 2);
        assert_eq!(deserialized.log_lines[0], "stdout: Build started");
        assert_eq!(deserialized.task.name, "test-package");
    }

    #[test]
    fn test_build_result_transmission_format_failure_roundtrip() {
        let start = make_naive_datetime("2024-06-01 10:00:00");
        let end = make_naive_datetime("2024-06-01 10:01:00");
        let result = BuildResultTransmissionFormat {
            task: sample_task(),
            status_code: 105,
            log_lines: vec!["stderr: error: failed to build".to_string()],
            success: false,
            timestamps: Timestamps { start, end },
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: BuildResultTransmissionFormat = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.success);
        assert_eq!(deserialized.status_code, 105);
        assert_eq!(deserialized.log_lines.len(), 1);
    }

    #[test]
    fn test_timestamps_serialization_roundtrip() {
        let start = make_naive_datetime("2024-01-15 08:30:00");
        let end = make_naive_datetime("2024-01-15 08:45:00");
        let ts = Timestamps { start, end };
        let json = serde_json::to_string(&ts).unwrap();
        let deserialized: Timestamps = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.start, start);
        assert_eq!(deserialized.end, end);
    }

    #[test]
    fn test_aur_package_settings_serialization_roundtrip() {
        let settings = AurPackageSettings {
            name: "firefox".to_string(),
            env: None,
            options: None,
        };
        let json = serde_json::to_string(&settings).unwrap();
        let deserialized: AurPackageSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "firefox");
        assert!(deserialized.env.is_none());
    }

    #[test]
    fn test_git_package_settings_serialization_roundtrip() {
        let settings = GitPackageSettings {
            source: "https://github.com/example/package".to_string(),
            subfolder: Some("pkg-dir".to_string()),
            env: None,
            options: Some("--sign".to_string()),
        };
        let json = serde_json::to_string(&settings).unwrap();
        let deserialized: GitPackageSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.source, "https://github.com/example/package");
        assert_eq!(deserialized.subfolder, Some("pkg-dir".to_string()));
        assert_eq!(deserialized.options, Some("--sign".to_string()));
    }

    #[test]
    fn test_build_result_empty_log_lines() {
        let start = make_naive_datetime("2024-06-01 10:00:00");
        let end = make_naive_datetime("2024-06-01 10:05:00");
        let result = BuildResultTransmissionFormat {
            task: sample_task(),
            status_code: 0,
            log_lines: vec![],
            success: true,
            timestamps: Timestamps { start, end },
        };
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: BuildResultTransmissionFormat = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.log_lines.len(), 0);
    }
}
