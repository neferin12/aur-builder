use sea_orm::{ActiveModelTrait, ActiveValue, DatabaseConnection, DbErr, EntityTrait};
use sea_orm_migration::{MigratorTrait, SchemaManager};
use crate::AurResultStruct;
use crate::migrator::Migrator;
use crate::entities::{prelude::*, *};

pub struct Database {
    db: DatabaseConnection,
}

impl Database {
    pub async fn new(url: String) -> Result<Self, DbErr> {
        let db = sea_orm::Database::connect(&url).await?;
        Ok(Self { db })
    }

    pub async fn migrate(&self) {
        println!("Applying migrations...");
        Migrator::up(&self.db, None).await.unwrap();

    }

    pub async fn update_metadata(&self, data: &AurResultStruct) -> bool {
        let mut new_timestamp = false;

        let existing = PackageMetadata::find_by_id(data.id).one(&self.db).await.unwrap();

         let db_data = package_metadata::ActiveModel {
            id: ActiveValue::Set(data.id.to_owned()),
            name: ActiveValue::Set(data.name.to_owned()),
            version: ActiveValue::Set(data.version.to_owned()),
            maintainer: ActiveValue::Set(data.maintainer.to_owned()),
            last_modified: ActiveValue::Set(data.last_modified.to_owned()),
        };

        if let Some(m) = existing {
            if m.last_modified < data.last_modified {
                let _ = db_data.update(&self.db).await;
                new_timestamp = true;
            }
        } else {
            let _ = db_data.insert(&self.db).await;
            new_timestamp = true;
        };



        new_timestamp
    }
}