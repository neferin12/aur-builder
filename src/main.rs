mod database;
mod migrator;

use std::collections::HashMap;
use std::env;
use reqwest::Error;
use crate::database::Database;

#[derive(Debug)]
pub struct AurResultStruct {
    id: u64,
    name: String,
    version: String,
    maintainer: String,
    last_modified: u64
}

pub type AurResult<'a> = Result<AurResultStruct, Error>;

async fn get_aur_data(package: &str) -> AurResult {
    let url = format!("https://aur.archlinux.org/rpc/v5/info?arg[]={}", package);
    let resp = reqwest::get(url).await;
    if resp.is_err() {
        return Err(resp.err().unwrap());
    };
    let data: serde_json::Value = resp.unwrap().json().await.unwrap();
    let results: AurResultStruct = AurResultStruct {
        id: data["results"][0]["ID"].as_u64().unwrap(),
        name: String::from(data["results"][0]["Name"].as_str().unwrap()),
        version: String::from(data["results"][0]["Version"].as_str().unwrap()),
        maintainer: String::from(data["results"][0]["Maintainer"].as_str().unwrap()),
        last_modified: data["results"][0]["LastModified"].as_u64().unwrap(),
    };
    return Ok(results);
}

#[tokio::main]
async fn main() {
    let database_url = match env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_e) => panic!("Failed to read environment variable 'DATABASE_URL'")
    };

    let db = Database::new(database_url).await.unwrap();
    db.migrate().await;

    let packages_string = match env::var("PACKAGES") {
        Ok(pkgs) => pkgs,
        Err(_e) => panic!("Failed to read environment variable 'PACKAGES'")
    };
    let packages = packages_string.split(",").collect::<Vec<&str>>();

    let mut package_data = Vec::new();

    for pkg in packages {
        let data = get_aur_data(pkg).await.unwrap();
        package_data.push(data);
    }

    dbg!(package_data);
}
