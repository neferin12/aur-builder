mod build;

use crate::build::build_package;
use crate::build::docker::pull_docker_image;
use common::environment::{VERSION, load_dotenv};
use common::errors::get_error_descriptions;
use common::types::BuildTaskTransmissionFormat;
use common::{connect_to_rabbitmq, get_rand_string};
use futures_util::stream::StreamExt;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicQosOptions, QueueDeclareOptions,
};
use lapin::types::FieldTable;

#[macro_use]
extern crate log;

#[tokio::main]
async fn main() {
    load_dotenv().ok();
    simple_logger::init_with_env().unwrap();

    info!("Starting Aur-Builder Worker v{VERSION}");

    match pull_docker_image().await {
        Err(_) => warn!("Builder image could not be pulled"),
        _ => {}
    }

    let db = database::connect_to_db().await;
    let conn = connect_to_rabbitmq().await;

    let rx_channel = conn.create_channel().await.unwrap();
    rx_channel
        .basic_qos(1, BasicQosOptions::default())
        .await
        .unwrap();
    rx_channel
        .queue_declare(
            "pkg_build",
            QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
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
    tx_results
        .queue_declare(
            "build_results",
            QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await
        .unwrap();

    while let Some(delivery) = consumer.next().await {
        let delivery = delivery.expect("error in consumer");
        let build_task: BuildTaskTransmissionFormat = match serde_json::from_slice(&delivery.data) {
            Ok(task) => task,
            Err(_) => {
                error!("Rejecting malformed build task");
                delivery
                    .reject(lapin::options::BasicRejectOptions { requeue: false })
                    .await
                    .unwrap();
                continue;
            }
        };

        match db.valid_attempt(&build_task).await {
            Ok(true) => {}
            Ok(false) => {
                error!("Rejecting unapproved or unpinned build task");
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
        if build_task
            .upload
            .as_ref()
            .map(|g| g.expires_at <= sea_orm::sqlx::types::chrono::Utc::now().timestamp())
            .unwrap_or(true)
        {
            tx_results
                .queue_declare(
                    "expired_tasks",
                    QueueDeclareOptions {
                        durable: true,
                        ..Default::default()
                    },
                    FieldTable::default(),
                )
                .await
                .unwrap();
            let mut expired = build_task.clone();
            expired.upload = None;
            common::messaging::publish(
                &tx_results,
                "expired_tasks",
                serde_json::to_vec(&expired).unwrap().as_slice(),
            )
            .await
            .unwrap();
            delivery.ack(BasicAckOptions::default()).await.unwrap();
            continue;
        }
        match build_package(&build_task).await {
            Ok(results) => {
                if !results.success {
                    warn!(
                        "Failed to build package '{}': {}",
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
                delivery.ack(BasicAckOptions::default()).await.expect("ack");
            }
            Err(error) => {
                error!(
                    "System error while building package '{}':\n{:?}",
                    build_task.name, error
                );
                delivery
                    .nack(BasicNackOptions::default())
                    .await
                    .expect("nack");
            }
        }
    }
}
