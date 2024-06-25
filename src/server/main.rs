use lapin::{BasicProperties, Connection, ConnectionProperties};
use lapin::options::{BasicPublishOptions, QueueDeclareOptions};
use lapin::types::FieldTable;
use reqwest::Error;
use aur_builder_commons::database::Database;
use aur_builder_commons::environment::get_environment_variable;
use aur_builder_commons::types::AurRequestResultStruct;


pub type AurResult<'a> = Result<AurRequestResultStruct, Error>;

/// This asynchronous function fetches data from the Arch User Repository (AUR) for a given package.
///
/// # Arguments
///
/// * `package` - A string slice that holds the name of the package.
///
/// # Returns
///
/// * `AurResult` - A Result type that returns `AurRequestResultStruct` on success, or `Error` on failure.
///
/// # Errors
///
/// This function will return an error if the HTTP request fails or if the JSON parsing fails.
///
/// # Example
///
/// ```
/// let data = get_aur_data("cvc5").await.unwrap();
/// ```
async fn get_aur_data(package: &str) -> AurResult {
    let url = format!("https://aur.archlinux.org/rpc/v5/info?arg[]={}", package);
    let resp = reqwest::get(url).await;
    if resp.is_err() {
        return Err(resp.err().unwrap());
    };
    let data: serde_json::Value = resp.unwrap().json().await.unwrap();
    let results: AurRequestResultStruct = AurRequestResultStruct {
        id: data["results"][0]["ID"].as_i64().unwrap(),
        name: String::from(data["results"][0]["Name"].as_str().unwrap()),
        version: String::from(data["results"][0]["Version"].as_str().unwrap()),
        maintainer: String::from(data["results"][0]["Maintainer"].as_str().unwrap()),
        last_modified: data["results"][0]["LastModified"].as_i64().unwrap(),
    };
    return Ok(results);
}

#[tokio::main]
async fn main() {
    let database_url = get_environment_variable("DATABASE_URL");

    let db = Database::new(database_url).await.unwrap();
    db.migrate().await;

    let packages_string = get_environment_variable("PACKAGES");

    let packages = packages_string.split(",").collect::<Vec<&str>>();

    let mut package_data = Vec::new();

    for pkg in packages {
        let data = get_aur_data(pkg).await.unwrap();
        package_data.push(data);
    }

    let q_addr = std::env::var("AMQP_ADDR").unwrap_or_else(|_| "amqp://127.0.0.1:5672/%2f".into());

    let conn = Connection::connect(
            &q_addr,
            ConnectionProperties::default(),
        )
        .await.unwrap();

    let tx_channel = conn.create_channel().await.unwrap();
    let queue = tx_channel.queue_declare(
        "pkg_build",
        QueueDeclareOptions::default(),
        FieldTable::default(),
    ).await.unwrap();

    for data in package_data {
        // let updated = db.update_metadata(&data).await;
        let updated = true;
        if updated {
            println!("{} was updated!", data.name);
            let d = tx_channel.basic_publish(
                "",
                "pkg_build",
                BasicPublishOptions::default(),
                data.name.as_ref(),
                BasicProperties::default(),
            ).await.unwrap().await.unwrap();
        }
    }




}
