use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20240624_000001_package_metadata_table"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PackageMetadata::Table)
                    .col(
                        ColumnDef::new(PackageMetadata::Id)
                            .big_unsigned()
                            .not_null()
                            .primary_key()
                            .auto_increment(),
                    )
                    .col(ColumnDef::new(PackageMetadata::Name).string().not_null())
                    .col(ColumnDef::new(PackageMetadata::Version).string().not_null())
                    .col(ColumnDef::new(PackageMetadata::Maintainer).string().not_null())
                    .col(ColumnDef::new(PackageMetadata::LastModified).big_integer().not_null())
                    .to_owned(),
            )
            .await
    }

    // Define how to rollback this migration
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PackageMetadata::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
pub enum PackageMetadata {
    Table,
    Id,
    Name,
    Version,
    Maintainer,
    LastModified
}