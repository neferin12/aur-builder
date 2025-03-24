mod build;

use crate::build::build_package;
use crate::build::docker::pull_docker_image;
use common::environment::{load_dotenv, VERSION};
use common::{connect_to_rabbitmq, get_rand_string};
use futures_util::stream::StreamExt;
use lapin::BasicProperties;
use lapin::options::{BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicPublishOptions, BasicQosOptions, QueueDeclareOptions};
use lapin::types::FieldTable;
use common::types::BuildTaskTransmissionFormat;

#[macro_use]
extern crate log;

#[tokio::main]
async fn main() {
    load_dotenv().unwrap();
    simple_logger::init_with_env().unwrap();

    info!("Starting Aur-Builder Worker v{VERSION}");

    info!("Pulling docker image...");
    match pull_docker_image().await {
        Ok(_) => info!("Image pulled successfully!"),
        Err(_) => warn!("Builder image could not be pulled"),
    }

    let conn = connect_to_rabbitmq().await;

    let rx_channel = conn.create_channel().await.unwrap();
    rx_channel
        .basic_qos(1, BasicQosOptions::default())
        .await
        .unwrap();
    rx_channel
        .queue_declare(
            "pkg_build",
            QueueDeclareOptions::default(),
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
        .queue_declare(
            "build_results",
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    while let Some(delivery) = consumer.next().await {
        let delivery = delivery.expect("error in consumer");
        let raw_data = match std::str::from_utf8(&*delivery.data) {
            Ok(v) => v.to_string(),
            Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
        };
        let build_task: BuildTaskTransmissionFormat = serde_json::from_str(raw_data.as_str()).unwrap();

        match build_package(&build_task).await {
            Ok(results) => {
                delivery.ack(BasicAckOptions::default()).await.expect("ack");
                tx_results.basic_publish(
                    "",
                    "build_results",
                    BasicPublishOptions::default(),
                    serde_json::to_string(&results).unwrap().as_ref(),
                    BasicProperties::default(),
                ).await.unwrap();
            }
            Err(error) => {
                error!("Error building package '{}':\n{:?}", build_task.name, error);
                delivery
                    .nack(BasicNackOptions::default())
                    .await
                    .expect("nack");
            }
        }
    }
}
