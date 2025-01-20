use sea_orm_migration::prelude::*;
use database::entities::prelude::*;
use crate::database;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Replace the sample below with your own migration scripts
        let db = manager.get_connection();
        db.execute_unprepared(
            "CREATE TABLE `build_results` (
	                `id`	INTEGER NOT NULL UNIQUE,
                    `package_id`	INTEGER NOT NULL,
                    `exit_code`	INTEGER NOT NULL,
                    `build_log`	TEXT,
                    PRIMARY KEY(`id` AUTOINCREMENT),
                    FOREIGN KEY(`package_id`) REFERENCES `package_metadata`(`id`) ON UPDATE CASCADE
            )"
        )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(BuildResults).to_owned())
            .await
    }
}
