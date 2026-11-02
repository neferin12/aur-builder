//! Confirm routing and persistence before acknowledging upstream deliveries.
use crate::storage::Error;
use lapin::{
    BasicProperties, Channel, options::BasicPublishOptions, publisher_confirm::Confirmation,
};
pub fn check_confirmation(confirmation: Confirmation) -> Result<(), Error> {
    match confirmation {
        Confirmation::Ack(None) => Ok(()),
        _ => Err("broker did not confirm a routed message".into()),
    }
}
pub async fn publish(channel: &Channel, queue: &str, payload: &[u8]) -> Result<(), Error> {
    let confirmation = channel
        .basic_publish(
            "",
            queue,
            BasicPublishOptions {
                mandatory: true,
                ..Default::default()
            },
            payload,
            BasicProperties::default().with_delivery_mode(2),
        )
        .await?
        .await?;
    check_confirmation(confirmation)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requires_acknowledgement() {
        assert!(check_confirmation(Confirmation::Ack(None)).is_ok());
        assert!(check_confirmation(Confirmation::Nack(None)).is_err());
        assert!(check_confirmation(Confirmation::NotRequested).is_err());
    }
}

/// A stopped background consumer must restart the service, rather than leave it silently idle.
pub fn supervise(future: impl std::future::Future<Output = ()> + Send + 'static) {
    let consumer = tokio::spawn(future);
    tokio::spawn(async move {
        let _ = consumer.await;
        log::error!("Background message consumer stopped; restarting service");
        std::process::exit(1);
    });
}

pub fn repo_queue_arguments() -> lapin::types::FieldTable {
    use lapin::types::{AMQPValue, FieldTable};
    let mut args = FieldTable::default();
    args.insert(
        "x-dead-letter-exchange".into(),
        AMQPValue::LongString("".into()),
    );
    args.insert(
        "x-dead-letter-routing-key".into(),
        AMQPValue::LongString("repo_ingest_failed".into()),
    );
    args
}
pub async fn declare_repo_queue(channel: &Channel) -> Result<(), Error> {
    use lapin::{options::QueueDeclareOptions, types::FieldTable};
    let options = QueueDeclareOptions {
        durable: true,
        ..Default::default()
    };
    channel
        .queue_declare("repo_ingest_failed", options, FieldTable::default())
        .await?;
    channel
        .queue_declare("repo_ingest", options, repo_queue_arguments())
        .await?;
    Ok(())
}

#[cfg(test)]
mod broker_tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires a disposable RabbitMQ instance via AMQP_ADDR"]
    async fn routed_confirmations() -> Result<(), Error> {
        let connection = crate::connect_to_rabbitmq().await;
        let channel = connection.create_channel().await?;
        channel
            .confirm_select(lapin::options::ConfirmSelectOptions::default())
            .await?;
        let name = format!("confirmation-test-{}", uuid::Uuid::new_v4());
        assert!(publish(&channel, &name, b"unroutable").await.is_err());
        channel
            .queue_declare(
                &name,
                lapin::options::QueueDeclareOptions {
                    exclusive: true,
                    auto_delete: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await?;
        publish(&channel, &name, b"routed").await?;
        declare_repo_queue(&channel).await?;
        let message = channel
            .basic_get(&name, lapin::options::BasicGetOptions::default())
            .await?
            .expect("confirmed delivery");
        assert_eq!(message.data, b"routed");
        message
            .ack(lapin::options::BasicAckOptions::default())
            .await?;
        publish(&channel, "repo_ingest", b"invalid artifact").await?;
        let message = channel
            .basic_get("repo_ingest", lapin::options::BasicGetOptions::default())
            .await?
            .expect("ingestion delivery");
        message
            .reject(lapin::options::BasicRejectOptions { requeue: false })
            .await?;
        let mut dead_letter = None;
        for _ in 0..20 {
            dead_letter = channel
                .basic_get(
                    "repo_ingest_failed",
                    lapin::options::BasicGetOptions::default(),
                )
                .await?;
            if dead_letter.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let message = dead_letter.expect("invalid artifact must be preserved in dead-letter queue");
        assert_eq!(message.data, b"invalid artifact");
        message
            .ack(lapin::options::BasicAckOptions::default())
            .await?;
        connection.close(200, "test complete").await?;
        Ok(())
    }
}

/// Periodic repository maintenance, serialized with ingestion.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionRequest {
    pub keep_versions: std::num::NonZeroUsize,
}
