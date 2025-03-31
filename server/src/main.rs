mod setup_rabbit_mq;
mod package_checkers;

use std::process::exit;
use database::connect_to_db;
use common::environment::{get_environment_variable, load_dotenv, VERSION};
use common::types::BuildTaskTransmissionFormat;
use lapin::options::BasicPublishOptions;
use lapin::BasicProperties;
use log::{debug, error, info};
use std::time::Duration;
use tokio::time::sleep;
use common::config::ServerConfig;

use package_checkers::{*};

#[tokio::main]
async fn main() {
    load_dotenv().ok();
    simple_logger::init_with_env().unwrap();

    let config_path = get_environment_variable("AB_CONFIG_PATH");
    let config = match ServerConfig::new(Some(config_path)) {
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

    let channels = setup_rabbit_mq::setup_rabbitmq(&db).await;
    let build_tx = channels.build_tx;

    loop {
        info!("Checking for package updates...");
        let mut package_data = Vec::new();

        for pkg in &config.aur_packages {
            debug!("Getting data for aur package {:?}", pkg);
            let aur_result = aur::get_aur_data(pkg).await;
            match aur_result {
                Ok(aur) => {
                    package_data.push(aur);
                }
                Err(e) => {
                    error!("Failed to get data for aur package \"{}\": {}", pkg.name, e);
                }
            }

        }

        for pkg in &config.git_packages {
            debug!("Getting data for git package {:?}", pkg);
            let git_result = git::get_git_data(pkg).await;
            match git_result {
                Ok(git_data) => {
                    package_data.push(git_data);
                }
                Err(e) => {
                    error!("Failed to get data for git package \"{}\": {}", pkg.source, e);
                }
            }
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
                build_tx
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

