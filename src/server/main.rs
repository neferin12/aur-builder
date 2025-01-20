use aur_builder_commons::database::Database;
use aur_builder_commons::environment::{get_environment_variable, load_dotenv, VERSION};
use aur_builder_commons::types::{AurRequestResult, BuildResultTransmissionFormat};
use aur_builder_commons::{connect_to_rabbitmq, CONNECTION_RETRY_NUMBER, RETRY_TIMEOUT};
use futures_util::{StreamExt, TryStreamExt};
use lapin::options::{BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions};
use lapin::types::FieldTable;
use lapin::BasicProperties;
use log::{debug, error, info};
use reqwest::Error;
use std::process::exit;
use std::time::Duration;
use tokio::time::sleep;

pub type AurResult<'a> = Result<AurRequestResult, Error>;

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
    let results: AurRequestResult = AurRequestResult {
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
    load_dotenv().unwrap();
    pretty_env_logger::init();

    info!("Starting Aur-Builder Server v{VERSION}");

    let db;
    let mut db_retries: u8 = 0;

    loop {
        if db_retries > CONNECTION_RETRY_NUMBER {
            error!("Could not connect to database");
            exit(4);
        };
        let database_url = get_environment_variable("DATABASE_URL");

        let db_result = Database::new(database_url).await;

        if db_result.is_ok() {
            db = db_result.unwrap();
            break;
        }
        error!(
            "Failed to connect to database: {} ==> Retrying in {RETRY_TIMEOUT}s...",
            db_result.err().unwrap().to_string()
        );
        db_retries += 1;
        sleep(Duration::from_secs(RETRY_TIMEOUT as u64)).await;
    }

    db.migrate().await;

    let packages_string = get_environment_variable("PACKAGES");

    let packages = packages_string.split(",").collect::<Vec<&str>>();

    let mut package_data = Vec::new();

    for pkg in packages {
        let data = get_aur_data(pkg).await.unwrap();
        package_data.push(data);
    }

    let conn = connect_to_rabbitmq().await;

    let tx_channel = conn.create_channel().await.unwrap();
    tx_channel
        .queue_declare(
            "pkg_build",
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    let results_channel = conn.create_channel().await.unwrap();
    results_channel
        .queue_declare(
            "build_results",
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    let mut results_consumer = results_channel
        .basic_consume(
            "build_results",
            "aur-builder-server",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    let locale_db = db.clone();
    tokio::spawn(async move{
        while let Some(delivery) = results_consumer.next().await {
            let delivery = delivery.expect("error in consumer");
            let data_str = match std::str::from_utf8(&*delivery.data) {
                Ok(v) => v.to_string(),
                Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
            };
            let data: BuildResultTransmissionFormat = serde_json::from_str(&*data_str).unwrap();
            locale_db.save_build_results(&data).await.unwrap();

            delivery.ack(BasicAckOptions::default()).await.unwrap();
        }
    });

    loop {
        info!("Checking for package updates...");
        for data in &package_data {
            let updated = db.update_metadata(&data).await;
            // let updated = true;
            if updated {
                info!("{} was updated!", data.name);
                tx_channel
                    .basic_publish(
                        "",
                        "pkg_build",
                        BasicPublishOptions::default(),
                        data.name.as_ref(),
                        BasicProperties::default(),
                    )
                    .await
                    .unwrap()
                    .await
                    .unwrap();
            }
        }
        sleep(Duration::from_secs(60 * 5)).await;
    }
}
