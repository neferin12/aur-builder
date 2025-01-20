use sea_orm_migration::{prelude::*, schema::*};
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
                        ColumnDef::new(Alias::new("success"))
                            .boolean()
                            .not_null()
                            .default(false),
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
                .drop_column(Alias::new("success"))
                .to_owned(),
        ).await?;
        
        Ok(())
    }
}
