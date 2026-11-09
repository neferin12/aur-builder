mod ai;
mod package_checkers;
mod review_notifications;
mod reviews;
mod setup_rabbit_mq;

use common::config::{Configurable, ServerConfig};
use common::environment::{VERSION, get_environment_variable, load_dotenv};
use database::connect_to_db;
use log::{debug, error, info};
use std::process::exit;
use std::time::{Duration, Instant};
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

    debug!("Configured {} AUR packages", config.aur_packages.len());
    debug!("Configured {} Git packages", config.git_packages.len());

    let channels = setup_rabbit_mq::setup_rabbitmq(&db).await;
    let build_tx = channels.build_tx;

    review_notifications::start(db.clone(), channels.notify_tx);
    ai::start(db.clone(), storage.clone());
    // Dispatch approvals independently of the package metadata polling interval.
    let dispatch_db = db.clone();
    let dispatch_storage = storage.clone();
    let dispatch_tx = build_tx.clone();
    common::messaging::supervise(async move {
        let db = dispatch_db;
        let storage = dispatch_storage;
        let build_tx = dispatch_tx;
        loop {
        // Recover tasks recorded before a crash between persistence and broker confirmation.
        for mut pending in db.pending_attempts().await.unwrap() {
            if !db.approved_task(&pending).await.unwrap()
                || !db.latest_attempt(&pending).await.unwrap()
            {
                db.attempt_state(&pending.attempt, "superseded")
                    .await
                    .unwrap();
                continue;
            }
            debug!("Dispatching pending build: package={}, attempt={}", pending.name, pending.attempt);
            storage.renew(&mut pending).unwrap();
            common::messaging::publish(
                &build_tx,
                "pkg_build",
                &serde_json::to_vec(&pending).unwrap(),
            )
            .await
            .unwrap();
            info!("Build dispatched: package={}, attempt={}", pending.name, pending.attempt);
            db.attempt_state(&pending.attempt, "dispatched")
                .await
                .unwrap();
        }
            sleep(Duration::from_secs(5)).await;
        }
    });
    loop {
        if let Err(e) = common::messaging::publish(
            &build_tx,
            "repo_ingest",
            &serde_json::to_vec(&common::messaging::RetentionRequest {
                keep_versions: config.keep_versions,
            })
            .unwrap(),
        )
        .await
        {
            error!("Could not request repository cleanup: {e}");
        }
        let check_started = Instant::now();
        info!("Checking for package updates: aur={}, git={}", config.aur_packages.len(), config.git_packages.len());
        let mut package_data = Vec::new();

        for pkg in &config.aur_packages {
            debug!("Getting data for AUR package {}", pkg.name);
            let aur_result = aur::get_aur_data(pkg).await;
            match aur_result {
                Ok(aur) => {
                    package_data.push(aur);
                }
                Err(e) => {
                    error!("Failed to get data for aur package \"{}\": {:?}", pkg.name, e);
                }
            }
        }

        for pkg in &config.git_packages {
            debug!("Getting data for configured Git package");
            let git_result = git::get_git_data(pkg).await;
            match git_result {
                Ok(git_data) => {
                    package_data.push(git_data);
                }
                Err(_) => {
                    error!(
                        "Failed to get package metadata from {}",
                        common::config::public_source(&pkg.source)
                    );
                }
            }
        }

        let fetched = package_data.len();
        let mut observed = 0usize;
        for mut data in package_data {
            match reviews::capture(&mut data) {
                Ok((revision, fingerprint, snapshot)) => {
                    if let Err(e) = db
                        .observe_release(&data, &revision, &fingerprint, &snapshot)
                        .await
                    {
                        error!("Could not persist release candidate for {}: {e:?}", data.name);
                    } else {
                        observed += 1;
                    }
                }
                Err(e) => error!("Could not capture packaging source for {}: {e}", data.name),
            }
        }
        let sleep_seconds = config.sleepduration.unwrap_or(60 * 5);
        info!("Package update check completed: fetched={fetched}, fetch_failed={}, observed={observed}, observation_failed={}, elapsed_ms={}, next_check_seconds={sleep_seconds}", config.aur_packages.len() + config.git_packages.len() - fetched, fetched - observed, check_started.elapsed().as_millis());
        sleep(Duration::from_secs(sleep_seconds)).await;
    }
}
