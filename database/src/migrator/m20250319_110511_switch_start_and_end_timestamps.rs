use crate::entities::prelude::BuildResults;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(BuildResults)
                    .rename_column(Alias::new("finished_at"), Alias::new("finished_at_old"))
                   .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(BuildResults)
                    .rename_column(Alias::new("started_at"), Alias::new("finished_at"))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(BuildResults)
                    .rename_column(Alias::new("finished_at_old"), Alias::new("started_at"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(BuildResults)
                    .rename_column(Alias::new("finished_at"), Alias::new("finished_at_old"))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(BuildResults)
                    .rename_column(Alias::new("started_at"), Alias::new("finished_at"))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(BuildResults)
                    .rename_column(Alias::new("finished_at_old"), Alias::new("started_at"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
