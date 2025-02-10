use aur_builder_commons::connect_to_rabbitmq;
use aur_builder_commons::database::connect_to_db;
use aur_builder_commons::environment::{get_environment_variable, load_dotenv, VERSION};
use aur_builder_commons::types::{
    AurRequestResult, BuildResultTransmissionFormat, BuildTaskTransmissionFormat,
};
use futures_util::StreamExt;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions,
};
use lapin::types::FieldTable;
use lapin::BasicProperties;
use log::info;
use reqwest::Error;
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
    let name = String::from(data["results"][0]["Name"].as_str().unwrap());
    let results: AurRequestResult = AurRequestResult {
        name: name.clone(),
        version: String::from(data["results"][0]["Version"].as_str().unwrap()),
        maintainer: String::from(data["results"][0]["Maintainer"].as_str().unwrap()),
        last_modified: data["results"][0]["LastModified"].as_i64().unwrap(),
    };
    Ok(results)
}

#[tokio::main]
async fn main() {
    load_dotenv().unwrap();
    pretty_env_logger::init();

    info!("Starting Aur-Builder Server v{VERSION}");

    let db = connect_to_db().await;
    db.migrate().await;

    let packages_string = get_environment_variable("PACKAGES");

    let packages = &packages_string.split(",").collect::<Vec<&str>>();

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
    tokio::spawn(async move {
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
        let mut package_data = Vec::new();

        for pkg in packages {
            let data = get_aur_data(pkg).await.unwrap();
            package_data.push(data);
        }
        for data in &package_data {
            let updated = db.update_metadata(&data).await;
            // let updated = true;
            if updated {
                info!("{} was updated!", data.name);
                let package = db.get_package_by_name(&data.name).await.unwrap().unwrap();
                let task = BuildTaskTransmissionFormat {
                    id: package.id.clone(),
                    name: package.name.clone(),
                    version: data.version.clone(),
                };
                tx_channel
                    .basic_publish(
                        "",
                        "pkg_build",
                        BasicPublishOptions::default(),
                        serde_json::to_string(&task).unwrap().as_ref(),
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
