use common::config::{Configurable, NotifierConfig};
use common::environment::{VERSION, load_dotenv};
use common::errors::get_error_descriptions;
use common::types::{Notification, ReviewNotification};
use common::{connect_to_rabbitmq, get_rand_string};
use futures_util::stream::StreamExt;
use lapin::options::{BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicQosOptions};
use lapin::types::FieldTable;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use log::{error, info};
use std::error::Error;
use std::process::exit;
use tera::{Context, Tera};

#[tokio::main]
async fn main() {
    load_dotenv().ok();
    simple_logger::init_with_env().unwrap();

    info!("Starting Aur-Builder Notifier v{VERSION}");

    let tera = match Tera::new("notifier/src/templates/**/*.html") {
        Ok(t) => t,
        Err(e) => {
            error!("Could not load email templates: {:?}", e);
            exit(1);
        }
    };

    let config = NotifierConfig::new(None).unwrap();

    let conn = connect_to_rabbitmq().await;

    let rx_channel = conn.create_channel().await.unwrap();
    rx_channel
        .basic_qos(1, BasicQosOptions::default())
        .await
        .unwrap();
    let mut consumer = rx_channel
        .basic_consume(
            "notifications",
            format!("aur-builder-notifier-{}", get_rand_string()).as_str(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();

    info!("Notifier ready; waiting for notifications");
    while let Some(delivery) = consumer.next().await {
        let delivery = delivery.expect("error in consumer");
        let notification: Notification = match serde_json::from_slice(&delivery.data) {
            Ok(notification) => notification,
            Err(e) => {
                error!("Rejecting malformed notification: {e}");
                delivery
                    .reject(lapin::options::BasicRejectOptions { requeue: false })
                    .await
                    .unwrap();
                continue;
            }
        };
        match receive_delivery(&notification, &tera, &config).await {
            Ok(_) => {
                delivery.ack(BasicAckOptions::default()).await.unwrap();
            }
            Err(e) => {
                error!(
                    "Notification delivery failed; requeuing for retry in 5s: {:?}",
                    e
                );
                delivery
                    .nack(BasicNackOptions {
                        requeue: true,
                        ..Default::default()
                    })
                    .await
                    .unwrap();
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

fn render_notification(
    notification: &Notification,
    tera: &Tera,
    maillogo: &str,
) -> Result<(String, String), Box<dyn Error>> {
    let mut context = Context::new();
    context.insert("maillogo", maillogo);
    let (subject, template) = match notification {
        Notification::Build(build_result) => {
            context.insert("build_result", build_result);
            context.insert(
                "error_description",
                &get_error_descriptions(build_result.status_code),
            );
            (
                if build_result.success {
                    format!("Updated {}", build_result.task.name)
                } else {
                    format!("Failed to update {}", build_result.task.name)
                },
                "build_notification.html",
            )
        }
        Notification::Review(ReviewNotification::ReviewRequired {
            package_name,
            version,
            revision,
            review_url,
            reason,
            ..
        }) => {
            context.insert("package_name", package_name);
            context.insert("release_version", version);
            context.insert("revision", revision);
            context.insert("review_url", review_url);
            context.insert("reason", reason);
            (
                format!("Review required: {package_name} {version}"),
                "review_notification.html",
            )
        }
    };
    if subject.contains(['\r', '\n']) {
        return Err("invalid email subject".into());
    }
    Ok((
        subject,
        css_inline::inline(&tera.render(template, &context)?)?,
    ))
}
async fn receive_delivery(
    notification: &Notification,
    tera: &Tera,
    config: &NotifierConfig,
) -> Result<(), Box<dyn Error>> {
    let (subject, mail_content) = render_notification(notification, tera, &config.maillogo)?;
    let email = Message::builder()
        .from(config.smtp.from.parse()?)
        .to(config.smtp.to.parse()?)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(mail_content)?;

    let creds = Credentials::new(config.smtp.user.clone(), config.smtp.pass.clone());

    let mailer = SmtpTransport::relay(config.smtp.host.as_str())?
        .credentials(creds)
        .build();

    mailer.send(&email)?;
    match notification {
        Notification::Build(result) => info!(
            "Build notification sent: package={}, attempt={}",
            result.task.name, result.task.attempt
        ),
        Notification::Review(ReviewNotification::ReviewRequired {
            candidate_id,
            package_name,
            ..
        }) => info!("Review notification sent: package={package_name}, candidate={candidate_id}"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_email_contains_action_link_and_escapes_source_text() {
        let tera = Tera::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/**/*.html"
        ))
        .unwrap();
        let notification = Notification::Review(ReviewNotification::ReviewRequired {
            candidate_id: "candidate".into(),
            package_name: "example".into(),
            version: "2-1".into(),
            revision: "a".repeat(40),
            review_url: "https://builder.example.com/reviews/candidate".into(),
            reason: "<script>unsafe</script>".into(),
        });
        let (subject, body) = render_notification(&notification, &tera, "").unwrap();
        assert_eq!(subject, "Review required: example 2-1");
        assert!(body.contains("https://builder.example.com/reviews/candidate"));
        assert!(body.contains("&lt;script&gt;"));
        assert!(!body.contains("<script>"));
    }
    #[test]
    fn legacy_build_messages_still_render() {
        let tera = Tera::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/**/*.html"
        ))
        .unwrap();
        let value = serde_json::json!({"task":{"id":1,"attempt":"a","object_key":"bundle","name":"example","version":"1","source":null,"subfolder":null,"options":null,"env":null},"status_code":0,"log_lines":[],"success":true,"timestamps":{"start":"2026-11-02T10:00:00","end":"2026-11-02T10:01:00"}});
        let message: Notification = serde_json::from_value(value).unwrap();
        let (subject, body) = render_notification(&message, &tera, "").unwrap();
        assert_eq!(subject, "Updated example");
        assert!(body.contains("successfully updated"));
        assert!(serde_json::from_str::<Notification>(r#"{"type":"unknown"}"#).is_err());
        if let Notification::Build(mut malicious) = message {
            malicious.task.name = "fixture\r\nBcc: attacker@example.com".into();
            assert!(render_notification(&Notification::Build(malicious), &tera, "").is_err());
        }
    }
}
