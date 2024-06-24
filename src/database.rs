use sea_orm::{DatabaseConnection, DbErr};
use sea_orm_migration::{MigratorTrait, SchemaManager};
use crate::migrator::Migrator;

pub struct Database {
    url: String,
    db: DatabaseConnection,
}

impl Database {
    pub async fn new(url: String) -> Result<Self, DbErr> {
        let db = sea_orm::Database::connect(&url).await?;
        Ok(Self { url, db })
    }

    pub async fn migrate(&self) {
        let schema_manager = SchemaManager::new(&self.db);

        Migrator::refresh(&self.db).await.unwrap();
        assert!(schema_manager.has_table("package_metadata").await.unwrap());
    }
}