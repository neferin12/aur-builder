use log::{error, LevelFilter};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectOptions,
    DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
};
use sea_orm_migration::MigratorTrait;
use std::process::exit;
use std::time::Duration;
use tokio::time::sleep;

pub mod entities;
pub mod migrator;

use crate::database::entities::*;
use crate::environment::get_environment_variable;
use crate::types::{AurRequestResult, BuildResultTransmissionFormat};
use crate::{CONNECTION_RETRY_NUMBER, RETRY_TIMEOUT};
use entities::prelude::*;
use migrator::Migrator;

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

        let existing = PackageMetadata::find_by_id(data.id)
            .one(&self.db)
            .await
            .unwrap();

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

    pub async fn get_packages(&self) -> Result<Vec<package_metadata::Model>, DbErr> {
        PackageMetadata::find().all(&self.db).await
    }
    
    pub async fn get_package(&self, id: i64) -> Result<Option<package_metadata::Model>, DbErr> {
        PackageMetadata::find_by_id(id).one(&self.db).await
    }

    pub async fn get_build_results(
        &self,
        package_id: i64,
    ) -> Result<Vec<build_results::Model>, Box<dyn std::error::Error>> {
        let results = BuildResults::find()
            .filter(build_results::Column::PackageId.eq(package_id))
            .order_by_desc(build_results::Column::FinishedAt)
            .all(&self.db)
            .await?;

        Ok(results)
    }

    pub async fn save_build_results(
        &self,
        data: &BuildResultTransmissionFormat,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let package = PackageMetadata::find()
            .filter(package_metadata::Column::Id.eq(data.task.id.clone()))
            .one(&self.db)
            .await?
            .unwrap();
        let db_data = build_results::ActiveModel {
            id: ActiveValue::NotSet,
            package_id: ActiveValue::Set(package.id),
            exit_code: ActiveValue::Set(data.status_code as i32),
            build_log: ActiveValue::Set(Some(data.log_lines.join(""))),
            success: ActiveValue::Set(data.success),
            finished_at: ActiveValue::Set(Some(data.timestamps.start)),
            started_at: ActiveValue::Set(Some(data.timestamps.end)),
            version: ActiveValue::Set(Some(data.task.version.clone())),
        };
        db_data.insert(&self.db).await?;

        Ok(())
    }
}

pub async fn connect_to_db() -> Database {
    let db;
    let mut db_retries: u8 = 0;

    loop {
        if db_retries > CONNECTION_RETRY_NUMBER {
            error!("Could not connect to database");
            exit(4);
        };
        let database_url = get_environment_variable("DATABASE_URL");

        let db_result = Database::new(database_url).await;

        if db_result.is_ok() {
            db = db_result.unwrap();
            break;
        }
        error!(
            "Failed to connect to database: {} ==> Retrying in {RETRY_TIMEOUT}s...",
            db_result.err().unwrap().to_string()
        );
        db_retries += 1;
        sleep(Duration::from_secs(RETRY_TIMEOUT as u64)).await;
    }

    db
}
