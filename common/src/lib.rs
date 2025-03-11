pub mod types;

use lapin::{Connection, ConnectionProperties};
use log::{error, info};
use std::process::exit;
use std::time::Duration;
use rand::RngCore;
use tokio::time::sleep;

pub mod environment;

/// The maximum number of retry attempts for establishing a database/rabbitmq connection.
pub const CONNECTION_RETRY_NUMBER: u8 = 10;

/// The timeout duration (in seconds) between retry attempts for connections.
pub const RETRY_TIMEOUT: u8 = 10;

pub async fn connect_to_rabbitmq() -> Connection {
    info!("Connecting to rabbitmq...");
    let q_addr = std::env::var("AMQP_ADDR").unwrap_or_else(|_| "amqp://127.0.0.1:5672/%2f".into());
    let conn;
    let mut rabbit_retries: u8 = 0;
    loop {
        if rabbit_retries > CONNECTION_RETRY_NUMBER {
            error!("Could not connect to rabbitmq");
            exit(5);
        };
        let conn_result = Connection::connect(&q_addr, ConnectionProperties::default()).await;
        if conn_result.is_ok() {
            conn = conn_result.unwrap();
            break;
        }

        error!(
            "Failed to connect to AMQP server: {} ==> Retrying in {RETRY_TIMEOUT}s...",
            conn_result.err().unwrap().to_string()
        );
        rabbit_retries += 1;
        sleep(Duration::from_secs(RETRY_TIMEOUT as u64)).await;
    }

    info!("Successfully connected to rabbitmq");

    conn
}

pub fn get_rand_string() -> String {
    rand::rng().next_u32().to_string()
}
