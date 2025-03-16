use database::Database;
use lapin::Channel;
use common::connect_to_rabbitmq;
use lapin::options::{BasicAckOptions, BasicConsumeOptions, QueueDeclareOptions};
use lapin::types::FieldTable;
use common::types::BuildResultTransmissionFormat;
use futures_util::StreamExt;

pub async fn setup_rabbitmq(db: &Database) -> Channel {
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
    tx_channel
}