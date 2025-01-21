use sea_orm_migration::prelude::*;
use crate::database::entities::prelude::BuildResults;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(BuildResults)
                    .add_column(
                        ColumnDef::new(Alias::new("started_at"))
                            .date_time()
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(BuildResults)
                    .add_column(
                        ColumnDef::new(Alias::new("version"))
                            .string()
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(
            Table::alter()
                .table(BuildResults)
                .drop_column(Alias::new("started_at"))
                .to_owned(),
        ).await?;
        manager.alter_table(
            Table::alter()
                .table(BuildResults)
                .drop_column(Alias::new("version"))
                .to_owned(),
        ).await?;

        Ok(())
    }
}

