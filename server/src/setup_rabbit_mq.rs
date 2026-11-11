use common::connect_to_rabbitmq;
use common::types::BuildResultTransmissionFormat;
use database::Database;
use futures_util::StreamExt;
use lapin::Channel;
use lapin::options::{BasicAckOptions, BasicConsumeOptions, QueueDeclareOptions};
use lapin::types::FieldTable;

pub struct RabbitChannels {
    pub build_tx: Channel,
    pub notify_tx: Channel,
}

pub async fn setup_rabbitmq(db: &Database) -> RabbitChannels {
    let conn = connect_to_rabbitmq().await;

    let build_tx = conn.create_channel().await.unwrap();
    build_tx
        .confirm_select(lapin::options::ConfirmSelectOptions::default())
        .await
        .unwrap();
    build_tx
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

    let notify_tx = conn.create_channel().await.unwrap();
    notify_tx
        .confirm_select(lapin::options::ConfirmSelectOptions::default())
        .await
        .unwrap();
    notify_tx
        .queue_declare(
            "notifications",
            QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await
        .unwrap();

    let results_channel = conn.create_channel().await.unwrap();
    results_channel
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

    let mut results_consumer = results_channel
        .basic_consume(
            "build_results",
            "aur-builder-server",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    common::messaging::declare_repo_queue(&notify_tx)
        .await
        .unwrap();
    let expired = conn.create_channel().await.unwrap();
    expired
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
    let mut expired_rx = expired
        .basic_consume(
            "expired_tasks",
            "server-expired",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();
    let retry_db = db.clone();
    let retry_tx = build_tx.clone();
    common::messaging::supervise(async move {
        let storage = common::storage::Storage::from_env().unwrap();
        while let Some(Ok(d)) = expired_rx.next().await {
            let mut task: common::types::BuildTaskTransmissionFormat =
                match serde_json::from_slice(&d.data) {
                    Ok(task) => task,
                    Err(_) => {
                        d.reject(lapin::options::BasicRejectOptions { requeue: false })
                            .await
                            .unwrap();
                        continue;
                    }
                };
            if retry_db.valid_attempt(&task).await.unwrap()
                && retry_db.latest_attempt(&task).await.unwrap()
                && retry_db.attempt_needs_build(&task).await.unwrap()
            {
                // Preserve the attempt identity so renewal cannot orphan a queued entry.
                storage.renew(&mut task).unwrap();
                common::messaging::publish(
                    &retry_tx,
                    "pkg_build",
                    &serde_json::to_vec(&task).unwrap(),
                )
                .await
                .unwrap();
                retry_db
                    .attempt_state(&task.attempt, "dispatched")
                    .await
                    .unwrap();
            }
            d.ack(BasicAckOptions::default()).await.unwrap();
        }
    });
    let locale_db = db.clone();
    let notification_publisher = notify_tx.clone();
    common::messaging::supervise(async move {
        while let Some(delivery) = results_consumer.next().await {
            let delivery = delivery.expect("error in consumer");
            let mut data: BuildResultTransmissionFormat =
                match serde_json::from_slice(&delivery.data) {
                    Ok(data) => data,
                    Err(_) => {
                        delivery
                            .reject(lapin::options::BasicRejectOptions { requeue: false })
                            .await
                            .unwrap();
                        continue;
                    }
                };
            data.task.upload = None;
            if !locale_db.valid_attempt(&data.task).await.unwrap() {
                delivery
                    .reject(lapin::options::BasicRejectOptions { requeue: false })
                    .await
                    .unwrap();
                continue;
            }
            if data.success && data.status_code == 0 {
                common::messaging::publish(
                    &notify_tx,
                    "repo_ingest",
                    &serde_json::to_vec(&data).unwrap(),
                )
                .await
                .unwrap();
            }
            locale_db.save_build_results(&data).await.unwrap();
            common::messaging::publish(
                &notify_tx,
                "notifications",
                serde_json::to_string(&data).unwrap().as_ref(),
            )
            .await
            .unwrap();

            locale_db.finish_attempt(&data.task.attempt).await.unwrap();
            delivery.ack(BasicAckOptions::default()).await.unwrap();
        }
    });

    RabbitChannels {
        build_tx,
        notify_tx: notification_publisher,
    }
}
