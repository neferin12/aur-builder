use std::error::Error;
use std::fmt;
use std::fmt::Formatter;

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

impl fmt::Display for MissingFieldError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Field '{}' is missing", self.field_name)
    }
}