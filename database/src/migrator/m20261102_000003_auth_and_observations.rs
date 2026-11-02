use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub struct Migration;
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for sql in [
            "ALTER TABLE web_session ADD COLUMN auth_context TEXT NOT NULL DEFAULT ''",
            // Legacy cookies were not bound to OIDC configuration and stored unhashed.
            "DELETE FROM web_session",
            "DELETE FROM oidc_login",
            "CREATE TABLE release_observation (package_id INTEGER PRIMARY KEY, fingerprint TEXT NOT NULL)",
            "INSERT INTO release_observation (package_id,fingerprint) SELECT c.package_id,c.fingerprint FROM release_candidate c WHERE c.id=(SELECT n.id FROM release_candidate n WHERE n.package_id=c.package_id ORDER BY n.created_at DESC,n.id DESC LIMIT 1)",
        ] {
            m.get_connection().execute_unprepared(sql).await?;
        }
        Ok(())
    }
    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for sql in [
            "DROP TABLE release_observation",
            "DELETE FROM web_session",
            "ALTER TABLE web_session DROP COLUMN auth_context",
        ] {
            m.get_connection().execute_unprepared(sql).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{Database, migrator::Migrator};
    use sea_orm_migration::MigratorTrait;
    #[tokio::test]
    async fn upgrade_clears_legacy_auth_and_backfills_observations() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        Migrator::up(&db.db, Some(11)).await.unwrap();
        db.review_execute("INSERT INTO web_session (id,user_id,csrf,expires_at) VALUES ('raw-cookie','user','csrf',9999999999)",vec![]).await.unwrap();
        db.review_execute(
            "INSERT INTO oidc_login (id,payload,expires_at) VALUES ('state','{}',9999999999)",
            vec![],
        )
        .await
        .unwrap();
        db.review_execute("INSERT INTO release_candidate (id,package_id,fingerprint,revision,payload,snapshot,created_at) VALUES ('old',1,'first','revision','{}','{}',1)",vec![]).await.unwrap();
        db.review_execute("INSERT INTO release_candidate (id,package_id,fingerprint,revision,payload,snapshot,created_at) VALUES ('latest',1,'second','revision','{}','{}',2)",vec![]).await.unwrap();
        Migrator::up(&db.db, None).await.unwrap();
        assert!(
            db.review_rows("SELECT id FROM web_session", vec![])
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            db.review_rows("SELECT id FROM oidc_login", vec![])
                .await
                .unwrap()
                .is_empty()
        );
        let rows = db
            .review_rows(
                "SELECT fingerprint FROM release_observation WHERE package_id=1",
                vec![],
            )
            .await
            .unwrap();
        assert_eq!(
            rows[0].try_get::<String>("", "fingerprint").unwrap(),
            "second"
        );
        assert_eq!(
            db.review_rows("SELECT id FROM release_candidate", vec![])
                .await
                .unwrap()
                .len(),
            2
        );
    }
}
