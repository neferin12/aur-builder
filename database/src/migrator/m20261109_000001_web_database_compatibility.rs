//! SeaORM's SQLite `datetime_text` declaration cannot be read by Prisma's native
//! driver. TEXT retains exactly the same stored UTC timestamps and remains
//! readable by SeaORM's chrono decoder. PostgreSQL already uses native timestamps.
use sea_orm_migration::prelude::*;
use sea_orm::{ConnectionTrait, TransactionTrait};
#[derive(DeriveMigrationName)]
pub struct Migration;
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        retype(manager, "TEXT").await
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        retype(manager, "datetime_text").await
    }
}
async fn retype(manager: &SchemaManager<'_>, timestamp_type: &str) -> Result<(), DbErr> {
    if manager.get_database_backend() != sea_orm::DatabaseBackend::Sqlite {
        return Ok(());
    }
    let connection = manager.get_connection();
    let db = connection.begin().await?;
    db.execute_unprepared(&format!("CREATE TABLE build_results_web_compat (id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT, package_id BIGINT NOT NULL, exit_code INTEGER NOT NULL, build_log TEXT, success BOOLEAN NOT NULL DEFAULT FALSE, started_at {timestamp_type}, finished_at {timestamp_type}, version TEXT, FOREIGN KEY(package_id) REFERENCES package_metadata(id) ON DELETE CASCADE ON UPDATE CASCADE)")).await?;
    db.execute_unprepared("INSERT INTO build_results_web_compat (id,package_id,exit_code,build_log,success,started_at,finished_at,version) SELECT id,package_id,exit_code,build_log,success,started_at,finished_at,version FROM build_results").await?;
    db.execute_unprepared("DROP TABLE build_results").await?;
    db.execute_unprepared("ALTER TABLE build_results_web_compat RENAME TO build_results").await?;
    db.commit().await
}

#[cfg(test)]
mod tests {
    use crate::{Database, migrator::Migrator};
    use sea_orm_migration::MigratorTrait;
    #[tokio::test]
    async fn preserves_existing_builds_and_seaorm_timestamp_decoding() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        Migrator::up(&db.db, Some(12)).await.unwrap();
        db.review_execute("INSERT INTO package_metadata (id,name,version,maintainer,last_modified) VALUES (1,'example','1','Alice',1)", vec![]).await.unwrap();
        db.review_execute("INSERT INTO build_results (id,package_id,exit_code,build_log,success,started_at,finished_at,version) VALUES (7,1,0,'preserved',TRUE,'2026-11-01 12:00:00','2026-11-01 12:01:00','1')", vec![]).await.unwrap();
        Migrator::up(&db.db, None).await.unwrap();
        let build = db.get_build_result(7).await.unwrap().unwrap();
        assert_eq!(build.build_log.as_deref(), Some("preserved"));
        assert!(build.success);
        assert_eq!(build.started_at.unwrap().to_string(), "2026-11-01 12:00:00");
        assert_eq!(build.finished_at.unwrap().to_string(), "2026-11-01 12:01:00");
    }
}
