use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub struct Migration;
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.get_connection().execute_unprepared("CREATE TABLE review_notification (candidate_id TEXT PRIMARY KEY, payload TEXT NOT NULL, sent BOOLEAN NOT NULL DEFAULT FALSE)").await?;
        Ok(())
    }
    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.get_connection()
            .execute_unprepared("DROP TABLE review_notification")
            .await?;
        Ok(())
    }
}
