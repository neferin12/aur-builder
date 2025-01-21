use crate::database;
use database::entities::prelude::*;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(BuildResults::Table)
                    .col(
                        ColumnDef::new(BuildResults::Id)
                            .big_integer()
                            .not_null()
                            .primary_key()
                            .auto_increment(),
                    )
                    .col(
                        ColumnDef::new(BuildResults::PackageId)
                            .big_integer()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(BuildResults::Table, BuildResults::PackageId)
                            .to(PackageMetadata::Table, PackageMetadata::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .col(ColumnDef::new(BuildResults::ExitCode).integer().not_null())
                    .col(ColumnDef::new(BuildResults::BuildLog).text())
                    .to_owned(),
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

#[derive(Iden)]
pub enum BuildResults {
    Table,
    Id,
    PackageId,
    ExitCode,
    BuildLog,
}

#[allow(dead_code)]
#[derive(Iden)]
pub enum PackageMetadata {
    Table,
    Id,
    Name,
    Version,
    Maintainer,
    LastModified,
}
