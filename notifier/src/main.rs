use common::config::{Configurable, NotifierConfig};
use common::environment::{VERSION, load_dotenv};
use common::errors::get_error_descriptions;
use common::types::BuildResultTransmissionFormat;
use common::{connect_to_rabbitmq, get_rand_string};
use futures_util::stream::StreamExt;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicNackOptions, BasicQosOptions,
    QueueDeclareOptions,
};
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

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::{BuildResultTransmissionFormat, BuildTaskTransmissionFormat, Timestamps};
    use chrono::NaiveDateTime;

    fn make_tera() -> Tera {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let pattern = format!("{manifest_dir}/src/templates/**/*.html");
        Tera::new(&pattern).unwrap()
    }

    fn make_build_result(name: &str, version: &str, success: bool, status_code: i64) -> BuildResultTransmissionFormat {
        let start = NaiveDateTime::parse_from_str("2024-06-01 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let end = NaiveDateTime::parse_from_str("2024-06-01 10:05:00", "%Y-%m-%d %H:%M:%S").unwrap();
        BuildResultTransmissionFormat {
            task: BuildTaskTransmissionFormat {
                id: 1,
                name: name.to_string(),
                version: version.to_string(),
                source: None,
                subfolder: None,
                options: None,
                env: None,
            },
            status_code,
            log_lines: vec!["stdout: some output".to_string()],
            success,
            timestamps: Timestamps { start, end },
        }
    }

    #[test]
    fn test_template_renders_success_notification() {
        let tera = make_tera();
        let build_result = make_build_result("my-package", "1.2.3", true, 0);
        let mut context = Context::new();
        context.insert("build_result", &build_result);
        context.insert("maillogo", "https://example.com/logo.png");
        context.insert("error_description", &get_error_descriptions(0));

        let rendered = tera.render("build_notification.html", &context).unwrap();
        assert!(rendered.contains("my-package"));
        assert!(rendered.contains("1.2.3"));
        assert!(rendered.contains("Updated my-package!"));
    }

    #[test]
    fn test_template_renders_failure_notification() {
        let tera = make_tera();
        let build_result = make_build_result("failing-pkg", "2.0.0", false, 105);
        let mut context = Context::new();
        context.insert("build_result", &build_result);
        context.insert("maillogo", "https://example.com/logo.png");
        context.insert("error_description", &get_error_descriptions(105));

        let rendered = tera.render("build_notification.html", &context).unwrap();
        assert!(rendered.contains("failing-pkg"));
        assert!(rendered.contains("Failed to update failing-pkg!"));
        assert!(rendered.contains("105"));
        assert!(rendered.contains("Failed to build package"));
    }

    #[test]
    fn test_template_renders_maillogo() {
        let tera = make_tera();
        let build_result = make_build_result("logo-pkg", "1.0.0", true, 0);
        let mut context = Context::new();
        context.insert("build_result", &build_result);
        context.insert("maillogo", "https://example.com/custom-logo.png");
        context.insert("error_description", &get_error_descriptions(0));

        let rendered = tera.render("build_notification.html", &context).unwrap();
        // Tera HTML-escapes '/' as '&#x2F;', so check for the logo filename
        assert!(
            rendered.contains("custom-logo.png"),
            "Expected maillogo reference in rendered output"
        );
    }

    #[test]
    fn test_success_subject_format() {
        let build_result = make_build_result("test-pkg", "3.0.0", true, 0);
        let subject = if build_result.success {
            format!("Updated {}", build_result.task.name)
        } else {
            format!("Failed to update {}", build_result.task.name)
        };
        assert_eq!(subject, "Updated test-pkg");
    }

    #[test]
    fn test_failure_subject_format() {
        let build_result = make_build_result("broken-pkg", "1.0.0", false, 105);
        let subject = if build_result.success {
            format!("Updated {}", build_result.task.name)
        } else {
            format!("Failed to update {}", build_result.task.name)
        };
        assert_eq!(subject, "Failed to update broken-pkg");
    }

    #[test]
    fn test_error_description_used_in_context() {
        let desc = get_error_descriptions(102);
        assert_eq!(desc, "Git clone failed");
    }

    #[test]
    fn test_css_inlining_produces_valid_html() {
        let tera = make_tera();
        let build_result = make_build_result("css-pkg", "1.0.0", true, 0);
        let mut context = Context::new();
        context.insert("build_result", &build_result);
        context.insert("maillogo", "");
        context.insert("error_description", &get_error_descriptions(0));

        let rendered = tera.render("build_notification.html", &context).unwrap();
        let inlined = css_inline::inline(&rendered).unwrap();
        // After inlining, the HTML should still be valid
        assert!(inlined.contains("<!DOCTYPE html>") || inlined.contains("<html"));
        assert!(inlined.contains("css-pkg"));
    }
}
