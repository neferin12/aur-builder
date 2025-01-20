use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct AurRequestResult {
    pub id: i64,
    pub name: String,
    pub version: String,
    pub maintainer: String,
    pub last_modified: i64
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BuildResultTransmissionFormat {
    pub name: String,
    pub status_code: i64,
    pub log_lines: Vec<String>,
    pub success: bool
}