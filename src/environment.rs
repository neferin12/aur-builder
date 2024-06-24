use std::env;

pub fn get_environment_variable(name: &str) -> String {
    match env::var(name) {
        Ok(url) => url,
        Err(_e) => panic!("Failed to read environment variable '{}'", name),
    }
}