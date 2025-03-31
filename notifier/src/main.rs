use common::config::{Configurable, NotifierConfig};
use common::environment::{VERSION, load_dotenv};
use common::errors::get_error_descriptions;
use common::types::BuildResultTransmissionFormat;
use common::{connect_to_rabbitmq, get_rand_string};
use futures_util::stream::StreamExt;
use lapin::message::Delivery;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicPublishOptions, BasicQosOptions,
    QueueDeclareOptions,
};
use lapin::types::FieldTable;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use log::{error, info};
use std::any::Any;
use std::env;
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
            error!("Parsing error(s): {}", e);
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
    rx_channel
        .queue_declare(
            "notifications",
            QueueDeclareOptions::default(),
            FieldTable::default(),
        )
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

    while let Some(delivery) = consumer.next().await {
        let delivery = delivery.expect("error in consumer");
        let raw_data = std::str::from_utf8(&*delivery.data).unwrap();

        let build_results: BuildResultTransmissionFormat = serde_json::from_str(raw_data).unwrap();
        match receive_delivery(&build_results, &tera, &config).await {
            Ok(_) => {
                delivery.ack(BasicAckOptions::default()).await.unwrap();
            },
            Err(e) => {
                error!("Error: {}", e);
                delivery.nack(BasicNackOptions::default()).await.unwrap();
            }
        }
    }
}

async fn receive_delivery(
    build_result: &BuildResultTransmissionFormat,
    tera: &Tera,
    config: &NotifierConfig,
) -> Result<(), Box<dyn Error>> {

    let mut context = Context::new();

    context.insert("build_result", &build_result);
    context.insert("maillogo", &config.maillogo);
    context.insert(
        "error_description",
        &get_error_descriptions(build_result.status_code),
    );

    let mail_content =
        css_inline::inline(
            tera.render("build_notification.html", &context).unwrap().as_str()
        )?;

    let subject: String;

    match build_result.success {
        true => subject = format!("Updated {}", build_result.task.name),
        false => subject = format!("Failed to update {}", build_result.task.name),
    }

    let email = Message::builder()
        .from(config.smtp.from.parse().unwrap())
        .to(config.smtp.to.parse().unwrap())
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(mail_content)?;

    let creds = Credentials::new(config.smtp.user.clone(), config.smtp.pass.clone());

    let mailer = SmtpTransport::relay(config.smtp.host.as_str())
        .unwrap()
        .credentials(creds)
        .build();

    mailer.send(&email)?;

    Ok(())
}
