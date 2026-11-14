mod build;

use crate::build::build_package;
use crate::build::docker::pull_docker_image;
use common::environment::{VERSION, load_dotenv};
use common::errors::get_error_descriptions;
use common::types::BuildTaskTransmissionFormat;
use common::{connect_to_rabbitmq, get_rand_string};
use futures_util::stream::StreamExt;
use lapin::options::{BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicQosOptions};
use lapin::types::FieldTable;

#[macro_use]
extern crate log;

#[tokio::main]
async fn main() {
    load_dotenv().ok();
    simple_logger::init_with_env().unwrap();

    info!("Starting Aur-Builder Worker v{VERSION}");
    assert!(
        std::env::var("AB_ARTIFACT_KEY")
            .expect("AB_ARTIFACT_KEY is required")
            .len()
            >= 32,
        "AB_ARTIFACT_KEY must contain at least 32 bytes"
    );
    common::storage::Storage::from_env().expect("worker storage configuration is required");

    match pull_docker_image().await {
        Err(e) => warn!("Builder image could not be pulled: {e:?}"),
        _ => {}
    }

    let db = database::connect_to_db().await;
    let conn = connect_to_rabbitmq().await;

    let rx_channel = conn.create_channel().await.unwrap();
    rx_channel
        .basic_qos(1, BasicQosOptions::default())
        .await
        .unwrap();
    let mut consumer = rx_channel
        .basic_consume(
            "pkg_build",
            format!("aur-builder-worker-{}", get_rand_string()).as_str(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    let tx_results = conn.create_channel().await.unwrap();
    tx_results
        .confirm_select(lapin::options::ConfirmSelectOptions::default())
        .await
        .unwrap();

    info!("Worker ready; waiting for build tasks");
    while let Some(delivery) = consumer.next().await {
        let delivery = delivery.expect("error in consumer");
        let build_task: BuildTaskTransmissionFormat = match serde_json::from_slice(&delivery.data) {
            Ok(task) => task,
            Err(e) => {
                error!("Rejecting malformed build task: {e:?}");
                delivery
                    .reject(lapin::options::BasicRejectOptions { requeue: false })
                    .await
                    .unwrap();
                continue;
            }
        };

        let authorized: Result<bool, sea_orm::DbErr> = async {
            Ok(db.valid_attempt(&build_task).await?
                && db.latest_attempt(&build_task).await?
                && db.attempt_needs_build(&build_task).await?)
        }
        .await;
        match authorized {
            Ok(true) => {}
            Ok(false) => {
                error!(
                    "Rejecting unapproved or unpinned build task: package={:?}, attempt={:?}",
                    build_task.name, build_task.attempt
                );
                delivery
                    .reject(lapin::options::BasicRejectOptions { requeue: false })
                    .await
                    .unwrap();
                continue;
            }
            Err(e) => {
                error!("Task validation temporarily unavailable: {e}");
                delivery
                    .nack(BasicNackOptions {
                        requeue: true,
                        ..Default::default()
                    })
                    .await
                    .unwrap();
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }
        }
        db.attempt_state(&build_task.attempt, "building")
            .await
            .expect("record build start");
        let build_started = std::time::Instant::now();
        info!(
            "Starting build: package={:?}, version={:?}, attempt={:?}",
            build_task.name, build_task.version, build_task.attempt
        );
        match build_package(&build_task).await {
            Ok(results) => {
                info!(
                    "Build completed: package={:?}, attempt={:?}, success={}, status_code={}, elapsed_ms={}",
                    build_task.name,
                    build_task.attempt,
                    results.success,
                    results.status_code,
                    build_started.elapsed().as_millis()
                );
                if !results.success {
                    warn!(
                        "Failed to build package {:?}: {}",
                        results.task.name,
                        get_error_descriptions(results.status_code)
                    );
                }
                common::messaging::publish(
                    &tx_results,
                    "build_results",
                    serde_json::to_string(&results).unwrap().as_ref(),
                )
                .await
                .unwrap();
                debug!(
                    "Build result published: package={:?}, attempt={:?}",
                    build_task.name, build_task.attempt
                );
                delivery.ack(BasicAckOptions::default()).await.expect("ack");
            }
            Err(error) => {
                db.attempt_state(&build_task.attempt, "dispatched")
                    .await
                    .expect("reset build state");
                error!(
                    "System error while building package {:?}, attempt={:?}: {:?}",
                    build_task.name, build_task.attempt, error
                );
                delivery
                    .nack(BasicNackOptions {
                        requeue: true,
                        ..Default::default()
                    })
                    .await
                    .expect("nack");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }
}
