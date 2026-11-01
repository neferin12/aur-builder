mod package_checkers;
mod setup_rabbit_mq;

use common::config::{Configurable, ServerConfig};
use common::environment::{VERSION, get_environment_variable, load_dotenv};
use database::connect_to_db;
use log::{debug, error, info};
use std::process::exit;
use std::time::Duration;
use tokio::time::sleep;

use package_checkers::*;

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

    let storage = common::storage::Storage::from_env().unwrap();
    let db = connect_to_db().await;
    db.migrate().await;

    debug!("aur packages: {:?}", &config.aur_packages);
    debug!("git packages: {:?}", &config.git_packages);

    let channels = setup_rabbit_mq::setup_rabbitmq(&db).await;
    let build_tx = channels.build_tx;

    loop {
        // Recover tasks recorded before a crash between persistence and broker confirmation.
        for mut pending in db.pending_attempts().await.unwrap() {
            if !db.latest_attempt(&pending).await.unwrap() {
                db.attempt_state(&pending.attempt, "superseded")
                    .await
                    .unwrap();
                continue;
            }
            storage.renew(&mut pending).unwrap();
            common::messaging::publish(
                &build_tx,
                "pkg_build",
                &serde_json::to_vec(&pending).unwrap(),
            )
            .await
            .unwrap();
            db.attempt_state(&pending.attempt, "dispatched")
                .await
                .unwrap();
        }
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
                    error!(
                        "Failed to get data for git package \"{}\": {}",
                        pkg.source, e
                    );
                }
            }
        }

        for data in &package_data {
            if let Some(task) = db
                .schedule_build(data, |task| storage.grant(task))
                .await
                .unwrap()
            {
                info!("{} was updated!", data.name);
                common::messaging::publish(
                    &build_tx,
                    "pkg_build",
                    serde_json::to_string(&task).unwrap().as_ref(),
                )
                .await
                .unwrap();
                db.attempt_state(&task.attempt, "dispatched").await.unwrap();
            }
        }
        sleep(Duration::from_secs(config.sleepduration.unwrap_or(60 * 5))).await;
    }
}
