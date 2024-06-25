use aur_builder_commons::environment::get_environment_variable;
use std::time::Duration;
use lapin::{Connection, ConnectionProperties};
use lapin::options::{BasicAckOptions, BasicConsumeOptions};
use lapin::types::FieldTable;
use aur_builder_commons::database::Database;
use futures_util::stream::StreamExt;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    let database_url = get_environment_variable("DATABASE_URL");
    let db = Database::new(database_url).await.unwrap();

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
        // dbg!(&delivery);
        let name = match std::str::from_utf8(&*delivery.data) {
            Ok(v) => v,
            Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
        };
        dbg!(name);
        sleep(Duration::from_secs(10)).await;
        delivery
            .ack(BasicAckOptions::default())
            .await
            .expect("ack");
    };
}