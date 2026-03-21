use std::env;
use std::io::ErrorKind;
pub fn get_environment_variable(name: &str) -> String {
    match env::var(name) {
        Ok(url) => url,
        Err(_e) => panic!("Failed to read environment variable '{}'", name),
    }
}

pub fn load_dotenv() -> dotenvy::Result<()> {
    match dotenvy::dotenv() {
        Ok(_) => Ok(()),
        Err(dotenvy::Error::Io(err)) if (err.kind() == ErrorKind::NotFound) => Ok(()),
        Err(e) => Err(e),
    }
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_get_environment_variable_success() {
        unsafe { env::set_var("TEST_COMMON_VAR_12345", "test_value") };
        let result = get_environment_variable("TEST_COMMON_VAR_12345");
        assert_eq!(result, "test_value");
        unsafe { env::remove_var("TEST_COMMON_VAR_12345") };
    }

    #[test]
    fn test_get_environment_variable_returns_correct_value() {
        unsafe { env::set_var("TEST_COMMON_VAR_XYZ", "hello_world") };
        let result = get_environment_variable("TEST_COMMON_VAR_XYZ");
        assert_eq!(result, "hello_world");
        unsafe { env::remove_var("TEST_COMMON_VAR_XYZ") };
    }

    #[test]
    #[should_panic(expected = "Failed to read environment variable 'NONEXISTENT_COMMON_VAR_99999'")]
    fn test_get_environment_variable_panics_when_missing() {
        unsafe { env::remove_var("NONEXISTENT_COMMON_VAR_99999") };
        get_environment_variable("NONEXISTENT_COMMON_VAR_99999");
    }

    #[test]
    fn test_version_is_not_empty() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_load_dotenv_succeeds_without_env_file() {
        let result = load_dotenv();
        assert!(result.is_ok());
    }
}
