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