mod build;

use lapin::options::{BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicQosOptions};
use lapin::types::FieldTable;
use futures_util::stream::StreamExt;
use aur_builder_commons::{connect_to_rabbitmq};
use aur_builder_commons::environment::load_dotenv;
use crate::build::build_package;
use crate::build::docker::pull_docker_image;

#[macro_use] extern crate log;

#[tokio::main]
async fn main() {
    load_dotenv().unwrap();
    pretty_env_logger::init();

    info!("Pulling docker image...");
    match pull_docker_image().await {
        Ok(_) => info!("Image pulled successfully!"),
        Err(_) => warn!("Builder image could not be pulled")
    }


    let conn = connect_to_rabbitmq().await;

    let rx_channel = conn.create_channel().await.unwrap();
    rx_channel.basic_qos(1, BasicQosOptions::default()).await.unwrap();
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