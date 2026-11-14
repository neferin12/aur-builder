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
            &format!("ab.{queue}"),
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

/// Periodic repository maintenance, serialized with ingestion.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionRequest {
    pub keep_versions: std::num::NonZeroUsize,
}

#[cfg(test)]
mod broker_tests {
    use super::*;
    use lapin::{Connection, ConnectionProperties, options::*, types::FieldTable};
    async fn channel(role: &str) -> Channel {
        let url = std::env::var(format!("AB_TEST_AMQP_{}", role.to_uppercase())).unwrap();
        let connection = Connection::connect(&url, ConnectionProperties::default())
            .await
            .unwrap();
        let channel = connection.create_channel().await.unwrap();
        channel
            .confirm_select(ConfirmSelectOptions::default())
            .await
            .unwrap();
        channel
    }
    #[tokio::test]
    #[ignore = "requires disposable provisioned broker via scripts/test_broker_integration.py"]
    async fn service_permissions() {
        let server = channel("server").await;
        publish(&server, "pkg_build", b"approved task")
            .await
            .unwrap();
        let worker = channel("worker").await;
        let task = worker
            .basic_get("pkg_build", BasicGetOptions::default())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.data, b"approved task");
        task.ack(BasicAckOptions::default()).await.unwrap();
        publish(&worker, "build_results", b"result").await.unwrap();
        let result = server
            .basic_get("build_results", BasicGetOptions::default())
            .await
            .unwrap()
            .unwrap();
        result.ack(BasicAckOptions::default()).await.unwrap();
        publish(&server, "notifications", b"notification")
            .await
            .unwrap();
        let notifier = channel("notifier").await;
        let notice = notifier
            .basic_get("notifications", BasicGetOptions::default())
            .await
            .unwrap()
            .unwrap();
        notice.ack(BasicAckOptions::default()).await.unwrap();
        publish(&server, "repo_ingest", b"invalid artifact")
            .await
            .unwrap();
        let repo = channel("repo").await;
        let artifact = repo
            .basic_get("repo_ingest", BasicGetOptions::default())
            .await
            .unwrap()
            .unwrap();
        artifact
            .reject(BasicRejectOptions { requeue: false })
            .await
            .unwrap();
        for role in ["worker", "repo", "notifier"] {
            let ch = channel(role).await;
            assert!(
                publish(&ch, "repo_ingest", b"forged retention")
                    .await
                    .is_err()
            );
        }
        for role in ["server", "repo", "notifier"] {
            let ch = channel(role).await;
            assert!(
                ch.basic_get("pkg_build", BasicGetOptions::default())
                    .await
                    .is_err()
            );
        }
        let ch = channel("worker").await;
        assert!(
            ch.basic_publish(
                "",
                "repo_ingest",
                BasicPublishOptions::default(),
                b"forged",
                BasicProperties::default()
            )
            .await
            .unwrap()
            .await
            .is_err()
        );
        let ch = channel("worker").await;
        assert!(
            ch.queue_bind(
                "pkg_build",
                "ab.notifications",
                "notifications",
                QueueBindOptions::default(),
                FieldTable::default()
            )
            .await
            .is_err()
        );
        let ch = channel("worker").await;
        assert!(
            ch.queue_delete("pkg_build", QueueDeleteOptions::default())
                .await
                .is_err()
        );
    }
}
