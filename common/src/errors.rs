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