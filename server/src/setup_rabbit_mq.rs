use common::connect_to_rabbitmq;
use common::types::BuildResultTransmissionFormat;
use database::Database;
use futures_util::StreamExt;
use lapin::{BasicProperties, Channel};
use lapin::options::{BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions};
use lapin::types::FieldTable;

pub struct RabbitChannels {
    pub build_tx: Channel
}

pub async fn setup_rabbitmq(db: &Database) -> RabbitChannels {
    let conn = connect_to_rabbitmq().await;

    let build_tx = conn.create_channel().await.unwrap();
    build_tx
        .queue_declare(
            "pkg_build",
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    let notify_tx = conn.create_channel().await.unwrap();
    notify_tx
        .queue_declare(
            "notifications",
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
            notify_tx.basic_publish(
                "",
                "notifications",
                BasicPublishOptions::default(),
                serde_json::to_string(&data).unwrap().as_ref(),
                BasicProperties::default(),
            ).await.unwrap();

            delivery.ack(BasicAckOptions::default()).await.unwrap();
        }
    });

    RabbitChannels {
        build_tx
    }
}
