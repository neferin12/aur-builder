use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub struct Migration;
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for sql in [
            "CREATE TABLE release_candidate (id TEXT PRIMARY KEY, package_id INTEGER NOT NULL, fingerprint TEXT NOT NULL, revision TEXT NOT NULL, baseline TEXT, payload TEXT NOT NULL, snapshot TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'pending', created_at BIGINT NOT NULL, UNIQUE(package_id,fingerprint))",
            "CREATE TABLE review_event (id TEXT PRIMARY KEY, candidate_id TEXT NOT NULL, actor TEXT NOT NULL, decision TEXT NOT NULL, comment TEXT NOT NULL, created_at BIGINT NOT NULL)",
            "CREATE TABLE ai_review (candidate_id TEXT PRIMARY KEY, result TEXT NOT NULL)",
            "CREATE TABLE web_user (id TEXT PRIMARY KEY, issuer TEXT NOT NULL, subject TEXT NOT NULL, display_name TEXT NOT NULL, admin BOOLEAN NOT NULL DEFAULT FALSE, disabled BOOLEAN NOT NULL DEFAULT FALSE, can_view BOOLEAN NOT NULL DEFAULT FALSE, can_rebuild BOOLEAN NOT NULL DEFAULT FALSE, can_approve BOOLEAN NOT NULL DEFAULT FALSE, UNIQUE(issuer,subject))",
            "CREATE TABLE web_session (id TEXT PRIMARY KEY, user_id TEXT NOT NULL, csrf TEXT NOT NULL, expires_at BIGINT NOT NULL)",
            "CREATE TABLE oidc_login (id TEXT PRIMARY KEY, payload TEXT NOT NULL, expires_at BIGINT NOT NULL)",
            "CREATE TABLE access_audit (id TEXT PRIMARY KEY, actor TEXT NOT NULL, target TEXT NOT NULL, detail TEXT NOT NULL, created_at BIGINT NOT NULL)",
            "CREATE INDEX candidate_package ON release_candidate(package_id, created_at)",
        ] {
            m.get_connection().execute_unprepared(sql).await?;
        }
        Ok(())
    }
    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            "access_audit",
            "oidc_login",
            "web_session",
            "web_user",
            "ai_review",
            "review_event",
            "release_candidate",
        ] {
            m.get_connection()
                .execute_unprepared(&format!("DROP TABLE {table}"))
                .await?;
        }
        Ok(())
    }
}
