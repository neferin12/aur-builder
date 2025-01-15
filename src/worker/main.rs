mod build;

use std::error::Error;
use aur_builder_commons::environment::get_environment_variable;
use std::time::Duration;
use lapin::{Connection, ConnectionProperties};
use lapin::options::{BasicAckOptions, BasicConsumeOptions, BasicNackOptions};
use lapin::types::FieldTable;
use futures_util::stream::StreamExt;
use crate::build::build_package;

#[macro_use] extern crate log;

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    // info!("Pulling docker image...");
    // pull_docker_image().await.unwrap();
    info!("Image pulled successfully!");

    info!("Connecting to rabbitmq...");
    let q_addr = std::env::var("AMQP_ADDR").unwrap_or_else(|_| "amqp://127.0.0.1:5672/%2f".into());
    let conn = Connection::connect(
        &q_addr,
        ConnectionProperties::default(),
    )
        .await.unwrap();

    let rx_channel = conn.create_channel().await.unwrap();
    let mut consumer = rx_channel.basic_consume(
        "pkg_build",
        "aur-builder-worker",
        BasicConsumeOptions::default(),
        FieldTable::default(),
    ).await.unwrap();
    while let Some(delivery) = consumer.next().await {
        let delivery = delivery.expect("error in consumer");
        let name = match std::str::from_utf8(&*delivery.data) {
            Ok(v) => v.to_string(),
            Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
        };

        match build_package(&name).await {
            Ok(_) => {
                delivery
                    .ack(BasicAckOptions::default())
                    .await
                    .expect("ack");
            }
            Err(error) => {
                error!("Error building package '{}':\n{:?}",name, error);
                delivery.nack(BasicNackOptions::default()).await.expect("nack");
            }
        }
    };
}