use common::types::ReviewNotification;
use database::{Database, reviews::Candidate};

pub async fn queue(
    db: &Database,
    c: &Candidate,
    reason: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let origin = std::env::var("AB_WEB_URL")?;
    queue_with_origin(db, c, reason, &origin).await
}
async fn queue_with_origin(
    db: &Database,
    c: &Candidate,
    reason: &str,
    origin: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let notification = ReviewNotification::ReviewRequired {
        candidate_id: c.id.clone(),
        package_name: c.payload.name.clone(),
        version: c.payload.version.clone(),
        revision: c.revision.clone(),
        review_url: format!("{}/reviews/{}", origin.trim_end_matches('/'), c.id),
        reason: reason.to_owned(),
    };
    db.review_execute("INSERT INTO review_notification (candidate_id,payload) SELECT id,? FROM release_candidate WHERE id=? AND state='pending' ON CONFLICT(candidate_id) DO NOTHING",vec![serde_json::to_string(&notification)?.into(),c.id.clone().into()]).await?;
    Ok(())
}
pub fn start(db: Database, channel: lapin::Channel) {
    common::messaging::supervise(async move {
        loop {
            match db.review_rows("SELECT n.candidate_id,n.payload FROM review_notification n JOIN release_candidate c ON c.id=n.candidate_id WHERE n.sent=FALSE AND c.state='pending'",vec![]).await {
                Ok(rows) => for row in rows {
                    let cid: String = row.try_get("", "candidate_id").unwrap();
                    let payload: String = row.try_get("", "payload").unwrap();
                    match common::messaging::publish(&channel, "notifications", payload.as_bytes()).await {
                        Ok(()) => { if let Err(e)=db.review_execute("UPDATE review_notification SET sent=TRUE WHERE candidate_id=?",vec![cid.into()]).await { log::error!("Could not mark review notification sent: {e}"); } },
                        Err(e) => log::error!("Could not publish review notification: {e}"),
                    }
                },
                Err(e) => log::error!("Review notification lookup failed: {e}"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn durable_alert_is_unique_and_omits_secrets() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let data = common::types::PackageSearchResult {
            name: "example".into(),
            version: "1".into(),
            maintainer: "test".into(),
            last_modified: 1,
            source: None,
            subfolder: None,
            options: None,
            environment: Some(vec![common::types::EnvironmentVariable {
                name: "TOKEN".into(),
                value: "secret-value".into(),
            }]),
        };
        db.observe_release(
            &data,
            &"a".repeat(40),
            "one",
            &database::reviews::Snapshot {
                files: vec![],
                complete: true,
            },
        )
        .await
        .unwrap();
        let c = db.candidates().await.unwrap().remove(0);
        for _ in 0..2 {
            queue_with_origin(
                &db,
                &c,
                "Human approval required",
                "https://builder.example.com/",
            )
            .await
            .unwrap();
        }
        let rows = db
            .review_rows(
                "SELECT payload FROM review_notification WHERE sent=FALSE",
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        let payload: String = rows[0].try_get("", "payload").unwrap();
        assert!(!payload.contains("secret-value"));
        assert!(payload.contains(&format!("https://builder.example.com/reviews/{}", c.id)));
        db.review_execute(
            "UPDATE review_notification SET sent=TRUE WHERE candidate_id=?",
            vec![c.id.clone().into()],
        )
        .await
        .unwrap();
        queue_with_origin(&db, &c, "Repeated polling", "https://builder.example.com")
            .await
            .unwrap();
        assert!(
            db.review_rows(
                "SELECT candidate_id FROM review_notification WHERE sent=FALSE",
                vec![]
            )
            .await
            .unwrap()
            .is_empty()
        );
        db.review_execute(
            "UPDATE review_notification SET sent=FALSE WHERE candidate_id=?",
            vec![c.id.clone().into()],
        )
        .await
        .unwrap();
        db.decide_release(&c.id, "human", false, "rejected", |_| unreachable!())
            .await
            .unwrap();
        assert!(db.review_rows("SELECT n.candidate_id FROM review_notification n JOIN release_candidate c ON c.id=n.candidate_id WHERE n.sent=FALSE AND c.state='pending'",vec![]).await.unwrap().is_empty());
        db.observe_release(
            &data,
            &"b".repeat(40),
            "two",
            &database::reviews::Snapshot {
                files: vec![],
                complete: true,
            },
        )
        .await
        .unwrap();
        let approved = db
            .candidates()
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.state == "pending")
            .unwrap();
        db.decide_release(&approved.id, "AI", true, "approved", |t| {
            t.attempt = "approved-attempt".into();
            t.object_key = "approved-object".into();
            Ok(())
        })
        .await
        .unwrap();
        queue_with_origin(
            &db,
            &approved,
            "Should be skipped",
            "https://builder.example.com",
        )
        .await
        .unwrap();
        assert!(
            db.review_rows(
                "SELECT candidate_id FROM review_notification WHERE candidate_id=?",
                vec![approved.id.into()]
            )
            .await
            .unwrap()
            .is_empty()
        );
    }
}
