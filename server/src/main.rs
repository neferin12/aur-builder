mod config_handler;
mod setup_rabbit_mq;
mod package_checkers;

use std::process::exit;
use common::connect_to_rabbitmq;
use database::{connect_to_db, Database};
use common::environment::{get_environment_variable, load_dotenv, VERSION};
use common::types::{BuildResultTransmissionFormat, BuildTaskTransmissionFormat, PackageSearchResult};
use futures_util::StreamExt;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions,
};
use lapin::types::FieldTable;
use lapin::{BasicProperties, Channel};
use log::{debug, error, info};
use reqwest::Error;
use std::time::Duration;
use tokio::time::sleep;
use crate::config_handler::Config;

use package_checkers::{*};

#[tokio::main]
async fn main() {
    load_dotenv().unwrap();
    pretty_env_logger::init();

    let config_path = get_environment_variable("AB_CONFIG_PATH");
    let config = match Config::new(Some(config_path)) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load config: {}", e);
            exit(4);
        }
    };
    info!("Starting Aur-Builder Server v{VERSION}");

    let db = connect_to_db().await;
    db.migrate().await;

    debug!("aur packages: {:?}", &config.aur_packages);
    debug!("git packages: {:?}", &config.git_packages);

    let tx_channel = setup_rabbit_mq::setup_rabbitmq(&db).await;

    loop {
        info!("Checking for package updates...");
        let mut package_data = Vec::new();

        for pkg in &config.aur_packages {
            debug!("Getting data for aur package {:?}", pkg);
            let data = aur::get_aur_data(pkg).await.unwrap();
            package_data.push(data);
        }

        for pkg in &config.git_packages {
            debug!("Getting data for git package {:?}", pkg);
            let data = git::get_git_data(pkg).await.unwrap();
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
                    source: package.source.clone(),
                    subfolder: package.subfolder.clone(),
                    options: data.options.clone(),
                    env: data.environment.clone()
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

