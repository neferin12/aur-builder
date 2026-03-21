use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fmt::{Display, Formatter};
use lazy_static::lazy_static;
use serde_json::Value;

const BUILD_ERROR_CODES_JSON : &str = include_str!("./error_codes.json");

fn create_error_map() -> HashMap<i64, String> {
    let mut error_map = HashMap::new();
    let pjson: Value = serde_json::from_str(&BUILD_ERROR_CODES_JSON).unwrap();
    if let Value::Object(map) = pjson {
        for (key, value) in &map {
            let code = key.parse::<i64>().unwrap_or(-1);
            let description = value.as_str().unwrap_or("").to_string();
            error_map.insert(code, description);
        }
    }
    error_map
}

lazy_static! {
    pub static ref ERROR_CODES: HashMap<i64, String> = create_error_map();
}

pub fn get_error_descriptions(error: i64) -> String {
    ERROR_CODES.get(&error).unwrap_or(&"Unknown error".to_string()).to_owned()
}

#[derive(Debug)]
pub struct MissingFieldError {
    field_name: String,
}

impl MissingFieldError {
    pub fn new(field_name: String) -> MissingFieldError {
        MissingFieldError {
            field_name
        }
    }
}

impl Error for MissingFieldError {}

impl Display for MissingFieldError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Field '{}' is missing", self.field_name)
    }
}

#[derive(Debug)]
pub struct AurRequestError {
    package: String,
    status_code: u16
}

impl AurRequestError {
    pub fn new(package: String, status_code: u16) -> AurRequestError {
        AurRequestError { package, status_code }
    }
}

impl Display for AurRequestError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Aur request for '{}' failed with code {}", self.package, self.status_code )
    }
}

impl Error for AurRequestError {}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_field_error_display() {
        let error = MissingFieldError::new("results".to_string());
        assert_eq!(error.to_string(), "Field 'results' is missing");
    }

    #[test]
    fn test_missing_field_error_display_custom_field() {
        let error = MissingFieldError::new("my_custom_field".to_string());
        assert_eq!(error.to_string(), "Field 'my_custom_field' is missing");
    }

    #[test]
    fn test_aur_request_error_display() {
        let error = AurRequestError::new("some-package".to_string(), 404);
        assert_eq!(
            error.to_string(),
            "Aur request for 'some-package' failed with code 404"
        );
    }

    #[test]
    fn test_aur_request_error_display_server_error() {
        let error = AurRequestError::new("another-pkg".to_string(), 500);
        assert_eq!(
            error.to_string(),
            "Aur request for 'another-pkg' failed with code 500"
        );
    }

    #[test]
    fn test_get_error_descriptions_success() {
        assert_eq!(get_error_descriptions(0), "Success");
    }

    #[test]
    fn test_get_error_descriptions_unable_to_change_dir() {
        assert_eq!(get_error_descriptions(100), "Unable to change dir");
    }

    #[test]
    fn test_get_error_descriptions_env_missing() {
        assert_eq!(get_error_descriptions(101), "Environment Variable missing");
    }

    #[test]
    fn test_get_error_descriptions_git_clone_failed() {
        assert_eq!(get_error_descriptions(102), "Git clone failed");
    }

    #[test]
    fn test_get_error_descriptions_yay_failed() {
        assert_eq!(get_error_descriptions(103), "Failed to run `yay -Syu`");
    }

    #[test]
    fn test_get_error_descriptions_dependency_install_failed() {
        assert_eq!(get_error_descriptions(104), "Failed to install dependency");
    }

    #[test]
    fn test_get_error_descriptions_build_failed() {
        assert_eq!(get_error_descriptions(105), "Failed to build package");
    }

    #[test]
    fn test_get_error_descriptions_copy_failed() {
        assert_eq!(get_error_descriptions(106), "Failed to copy result files");
    }

    #[test]
    fn test_get_error_descriptions_upload_failed() {
        assert_eq!(get_error_descriptions(107), "Failed to upload pkg file");
    }

    #[test]
    fn test_get_error_descriptions_unknown_code() {
        assert_eq!(get_error_descriptions(999), "Unknown error");
    }

    #[test]
    fn test_get_error_descriptions_negative_code() {
        assert_eq!(get_error_descriptions(-1), "Unknown error");
    }

    #[test]
    fn test_error_codes_map_contains_all_defined_codes() {
        assert!(ERROR_CODES.contains_key(&0));
        assert!(ERROR_CODES.contains_key(&100));
        assert!(ERROR_CODES.contains_key(&101));
        assert!(ERROR_CODES.contains_key(&102));
        assert!(ERROR_CODES.contains_key(&103));
        assert!(ERROR_CODES.contains_key(&104));
        assert!(ERROR_CODES.contains_key(&105));
        assert!(ERROR_CODES.contains_key(&106));
        assert!(ERROR_CODES.contains_key(&107));
    }

    #[test]
    fn test_error_codes_map_does_not_contain_undefined_codes() {
        assert!(!ERROR_CODES.contains_key(&999));
        assert!(!ERROR_CODES.contains_key(&-5));
    }

    #[test]
    fn test_missing_field_error_is_error() {
        let error = MissingFieldError::new("field".to_string());
        let _: &dyn Error = &error;
    }

    #[test]
    fn test_aur_request_error_is_error() {
        let error = AurRequestError::new("pkg".to_string(), 400);
        let _: &dyn Error = &error;
    }
}
