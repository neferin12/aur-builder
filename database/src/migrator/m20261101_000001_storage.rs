use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub struct Migration;
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for sql in [
            "CREATE TABLE build_attempt (attempt TEXT PRIMARY KEY, package_id INTEGER NOT NULL, object_key TEXT NOT NULL, task TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'queued', created_at BIGINT NOT NULL)",
            "CREATE TABLE repo_catalog (id INTEGER PRIMARY KEY, catalog TEXT NOT NULL)",
        ] {
            m.get_connection().execute_unprepared(sql).await?;
        }
        Ok(())
    }
    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.get_connection()
            .execute_unprepared("DROP TABLE repo_catalog")
            .await?;
        m.get_connection()
            .execute_unprepared("DROP TABLE build_attempt")
            .await?;
        Ok(())
    }
}
