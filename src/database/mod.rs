use log::LevelFilter;
use rand::RngCore;
use sea_orm::{ActiveModelTrait, ActiveValue, ColumnTrait, ConnectOptions, DatabaseConnection, DbErr, EntityTrait, QueryFilter};
use sea_orm_migration::MigratorTrait;

pub mod entities;
pub mod migrator;

use migrator::Migrator;
use entities::{prelude::*, *};
use crate::types::{AurRequestResult, BuildResultTransmissionFormat};

/// The `Database` struct represents a database connection.
///
/// It contains methods to create a new database connection, apply migrations, and update metadata.
#[derive(Debug, Clone)]
pub struct Database {
    db: DatabaseConnection,
}


impl Database {
    /// This asynchronous function creates a new database connection.
    ///
    /// # Arguments
    ///
    /// * `url` - A string that holds the URL of the database.
    ///
    /// # Returns
    ///
    /// * `Result<Self, DbErr>` - A Result type that returns `Self` on success, or `DbErr` on failure.
    ///
    /// # Example
    ///
    /// ```
    /// let db = Database::new("postgres://localhost/test").await.unwrap();
    /// ```
    pub async fn new(url: String) -> Result<Self, DbErr> {
        let mut options = ConnectOptions::new(url);
        options.sqlx_logging_level(LevelFilter::Trace);
        let db = sea_orm::Database::connect(options).await?;
        Ok(Self { db })
    }

    /// This asynchronous function applies migrations to the database.
    ///
    /// # Example
    ///
    /// ```
    /// db.migrate().await;
    /// ```
    pub async fn migrate(&self) {
        println!("Applying migrations...");
        Migrator::up(&self.db, None).await.unwrap();
    }

    /// This asynchronous function updates the metadata of a package in the database.
    ///
    /// # Arguments
    ///
    /// * `data` - A reference to `AurRequestResultStruct` that holds the metadata of the package.
    ///
    /// # Returns
    ///
    /// * `bool` - A boolean value that indicates whether the metadata was updated.
    ///
    /// # Example
    ///
    /// ```
    /// let updated = db.update_metadata(&data).await;
    /// ```
    pub async fn update_metadata(&self, data: &AurRequestResult) -> bool {
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

    pub async fn save_build_results(&self, data: &BuildResultTransmissionFormat) -> Result<(), Box<dyn std::error::Error>> {
        let package = PackageMetadata::find()
            .filter(package_metadata::Column::Name.eq(data.name.clone()))
            .one(&self.db)
            .await?.unwrap();
        let db_data = build_results::ActiveModel {
            id: ActiveValue::Set(rand::thread_rng().next_u32() as i32),
            package_id: ActiveValue::Set(package.id as i32),
            exit_code: ActiveValue::Set(data.status_code as i32),
            build_log: ActiveValue::Set(Some(data.log_lines.join(""))),
            success: ActiveValue::Set(data.success),
        };
        db_data.insert(&self.db).await?;

        Ok(())
    }
}
