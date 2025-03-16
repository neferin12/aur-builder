use sea_orm_migration::{prelude::*, schema::*};
use crate::entities::prelude::PackageMetadata;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(PackageMetadata)
                    .add_column(
                        ColumnDef::new(Alias::new("source"))
                            .string()
                            .null()
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(PackageMetadata)
                    .drop_column(Alias::new("source"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}