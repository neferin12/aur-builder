//! Contract check used by the Nuxt integration suite against disposable databases.
#[tokio::main]
async fn main() {
    let db = database::Database::new(std::env::var("DATABASE_URL").expect("DATABASE_URL"))
        .await
        .expect("connect fixture database");
    let tasks = db
        .pending_attempts()
        .await
        .expect("decode tasks with Rust scheduler types");
    assert!(!tasks.is_empty(), "expected a queued task from Nuxt");
    for task in tasks {
        assert!(
            db.valid_attempt(&task)
                .await
                .expect("validate complete Nuxt task JSON")
        );
        assert!(
            db.approved_task(&task)
                .await
                .expect("check approved revision")
        );
        assert!(
            db.latest_attempt(&task)
                .await
                .expect("check attempt ordering")
        );
        assert!(
            task.upload.is_none(),
            "capability URLs must not be persisted"
        );
    }
}
