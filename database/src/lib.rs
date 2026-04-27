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

use entities::*;
use common::environment::get_environment_variable;
use common::types::{BuildResultTransmissionFormat, PackageSearchResult};
use common::{CONNECTION_RETRY_NUMBER, RETRY_TIMEOUT};
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
    /// ```ignore
    /// use database::Database;
    /// let db = Database::new("postgres://localhost/test".parse().unwrap()).await.unwrap();
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
    /// ```ignore
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
    /// ```ignore
    /// let updated = db.update_metadata(&data).await;
    /// ```
    pub async fn update_metadata(&self, data: &PackageSearchResult) -> bool {
        let mut new_timestamp = false;

        let existing =
            self.get_package_by_name(&data.name)
                .await.unwrap();

        let mut db_data = package_metadata::ActiveModel {
            id: ActiveValue::NotSet,
            name: ActiveValue::Set(data.name.to_owned()),
            version: ActiveValue::Set(data.version.to_owned()),
            maintainer: ActiveValue::Set(data.maintainer.to_owned()),
            last_modified: ActiveValue::Set(data.last_modified.to_owned()),
            source: ActiveValue::Set(data.source.to_owned()),
            subfolder: ActiveValue::Set(data.subfolder.to_owned()),
        };

        if let Some(m) = existing {
            db_data.id = ActiveValue::Set(m.id);
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

    pub async fn get_package(&self, id: i32) -> Result<Option<package_metadata::Model>, DbErr> {
        PackageMetadata::find_by_id(id).one(&self.db).await
    }

    pub async fn get_package_by_name(&self, name: &String) -> Result<Option<package_metadata::Model>, DbErr> {
        PackageMetadata::find()
            .filter(package_metadata::Column::Name.eq(name))
            .one(&self.db)
            .await
    }

    pub async fn reset_package_last_modified(&self, id: i32) {
        let p = PackageMetadata::find_by_id(id).one(&self.db).await.unwrap().unwrap();
        let mut am = package_metadata::ActiveModel::from(p);
        am.last_modified = ActiveValue::Set(0);
        am.update(&self.db).await.unwrap();
    }

    pub async fn get_build_results(
        &self,
        package_id: i32,
    ) -> Result<Vec<build_results::Model>, Box<dyn std::error::Error>> {
        let results = BuildResults::find()
            .filter(build_results::Column::PackageId.eq(package_id))
            .order_by_desc(build_results::Column::StartedAt)
            .all(&self.db)
            .await?;

        Ok(results)
    }

    pub async fn get_build_result(&self, build_result_id: i32) -> Result<Option<build_results::Model>, DbErr> {
        BuildResults::find_by_id(build_result_id).one(&self.db).await
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
            package_id: ActiveValue::Set(package.id as i64),
            exit_code: ActiveValue::Set(data.status_code as i32),
            build_log: ActiveValue::Set(Some(data.log_lines.join(""))),
            success: ActiveValue::Set(data.success),
            finished_at: ActiveValue::Set(Some(data.timestamps.end)),
            started_at: ActiveValue::Set(Some(data.timestamps.start)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::{
        BuildResultTransmissionFormat, BuildTaskTransmissionFormat, PackageSearchResult, Timestamps,
    };
    use sea_orm::sqlx::types::chrono::NaiveDateTime;

    async fn setup_db() -> Database {
        let db = Database::new("sqlite::memory:".to_string()).await.unwrap();
        db.migrate().await;
        db
    }

    fn make_package_search_result(name: &str, version: &str, last_modified: i64) -> PackageSearchResult {
        PackageSearchResult {
            name: name.to_string(),
            version: version.to_string(),
            maintainer: "testmaintainer".to_string(),
            last_modified,
            source: None,
            subfolder: None,
            options: None,
            environment: None,
        }
    }

    fn make_build_result(task: BuildTaskTransmissionFormat, success: bool, code: i64) -> BuildResultTransmissionFormat {
        let start = NaiveDateTime::parse_from_str("2024-01-01 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let end = NaiveDateTime::parse_from_str("2024-01-01 10:05:00", "%Y-%m-%d %H:%M:%S").unwrap();
        BuildResultTransmissionFormat {
            task,
            status_code: code,
            log_lines: vec!["stdout: build output".to_string()],
            success,
            timestamps: Timestamps { start, end },
        }
    }

    #[tokio::test]
    async fn test_new_package_is_inserted_and_returns_true() {
        let db = setup_db().await;
        let data = make_package_search_result("new-pkg", "1.0.0", 1000);
        let updated = db.update_metadata(&data).await;
        assert!(updated, "Expected true for a new package insertion");
    }

    #[tokio::test]
    async fn test_existing_package_with_newer_timestamp_is_updated() {
        let db = setup_db().await;
        let data_old = make_package_search_result("my-pkg", "1.0.0", 1000);
        db.update_metadata(&data_old).await;

        let data_new = make_package_search_result("my-pkg", "1.1.0", 2000);
        let updated = db.update_metadata(&data_new).await;
        assert!(updated, "Expected true when timestamp is newer");
    }

    #[tokio::test]
    async fn test_existing_package_with_same_timestamp_is_not_updated() {
        let db = setup_db().await;
        let data = make_package_search_result("stable-pkg", "1.0.0", 5000);
        db.update_metadata(&data).await;

        let data_same = make_package_search_result("stable-pkg", "1.0.0", 5000);
        let updated = db.update_metadata(&data_same).await;
        assert!(!updated, "Expected false when timestamp is the same");
    }

    #[tokio::test]
    async fn test_existing_package_with_older_timestamp_is_not_updated() {
        let db = setup_db().await;
        let data_new = make_package_search_result("newer-pkg", "2.0.0", 9000);
        db.update_metadata(&data_new).await;

        let data_old = make_package_search_result("newer-pkg", "1.0.0", 1000);
        let updated = db.update_metadata(&data_old).await;
        assert!(!updated, "Expected false when timestamp is older");
    }

    #[tokio::test]
    async fn test_get_packages_returns_all() {
        let db = setup_db().await;
        db.update_metadata(&make_package_search_result("pkg-a", "1.0", 100)).await;
        db.update_metadata(&make_package_search_result("pkg-b", "2.0", 200)).await;
        db.update_metadata(&make_package_search_result("pkg-c", "3.0", 300)).await;

        let packages = db.get_packages().await.unwrap();
        assert_eq!(packages.len(), 3);
    }

    #[tokio::test]
    async fn test_get_packages_empty_database() {
        let db = setup_db().await;
        let packages = db.get_packages().await.unwrap();
        assert!(packages.is_empty());
    }

    #[tokio::test]
    async fn test_get_package_by_id() {
        let db = setup_db().await;
        db.update_metadata(&make_package_search_result("findable-pkg", "1.0", 100)).await;

        let packages = db.get_packages().await.unwrap();
        let id = packages[0].id;

        let found = db.get_package(id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "findable-pkg");
    }

    #[tokio::test]
    async fn test_get_package_nonexistent_id_returns_none() {
        let db = setup_db().await;
        let result = db.get_package(99999).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_package_by_name() {
        let db = setup_db().await;
        db.update_metadata(&make_package_search_result("named-pkg", "3.2.1", 500)).await;

        let result = db.get_package_by_name(&"named-pkg".to_string()).await.unwrap();
        assert!(result.is_some());
        let pkg = result.unwrap();
        assert_eq!(pkg.name, "named-pkg");
        assert_eq!(pkg.version, "3.2.1");
    }

    #[tokio::test]
    async fn test_get_package_by_name_nonexistent_returns_none() {
        let db = setup_db().await;
        let result = db.get_package_by_name(&"ghost-pkg".to_string()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_reset_package_last_modified_sets_to_zero() {
        let db = setup_db().await;
        db.update_metadata(&make_package_search_result("resettable-pkg", "1.0", 9999)).await;
        let pkg = db.get_package_by_name(&"resettable-pkg".to_string()).await.unwrap().unwrap();
        assert_eq!(pkg.last_modified, 9999);

        db.reset_package_last_modified(pkg.id).await;
        let updated = db.get_package_by_name(&"resettable-pkg".to_string()).await.unwrap().unwrap();
        assert_eq!(updated.last_modified, 0);
    }

    #[tokio::test]
    async fn test_save_and_retrieve_build_results() {
        let db = setup_db().await;
        db.update_metadata(&make_package_search_result("build-pkg", "1.0", 100)).await;
        let pkg = db.get_package_by_name(&"build-pkg".to_string()).await.unwrap().unwrap();

        let task = BuildTaskTransmissionFormat {
            id: pkg.id,
            name: "build-pkg".to_string(),
            version: "1.0".to_string(),
            source: None,
            subfolder: None,
            options: None,
            env: None,
        };
        let result = make_build_result(task, true, 0);
        db.save_build_results(&result).await.unwrap();

        let results = db.get_build_results(pkg.id).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert_eq!(results[0].exit_code, 0);
    }

    #[tokio::test]
    async fn test_save_multiple_build_results() {
        let db = setup_db().await;
        db.update_metadata(&make_package_search_result("multi-build-pkg", "1.0", 100)).await;
        let pkg = db.get_package_by_name(&"multi-build-pkg".to_string()).await.unwrap().unwrap();

        for i in 0..3 {
            let task = BuildTaskTransmissionFormat {
                id: pkg.id,
                name: "multi-build-pkg".to_string(),
                version: format!("1.{i}"),
                source: None,
                subfolder: None,
                options: None,
                env: None,
            };
            db.save_build_results(&make_build_result(task, true, 0)).await.unwrap();
        }

        let results = db.get_build_results(pkg.id).await.unwrap();
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_save_failed_build_result() {
        let db = setup_db().await;
        db.update_metadata(&make_package_search_result("failing-pkg", "1.0", 100)).await;
        let pkg = db.get_package_by_name(&"failing-pkg".to_string()).await.unwrap().unwrap();

        let task = BuildTaskTransmissionFormat {
            id: pkg.id,
            name: "failing-pkg".to_string(),
            version: "1.0".to_string(),
            source: None,
            subfolder: None,
            options: None,
            env: None,
        };
        let result = make_build_result(task, false, 105);
        db.save_build_results(&result).await.unwrap();

        let results = db.get_build_results(pkg.id).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].success);
        assert_eq!(results[0].exit_code, 105);
    }

    #[tokio::test]
    async fn test_get_build_results_empty_for_new_package() {
        let db = setup_db().await;
        db.update_metadata(&make_package_search_result("empty-builds-pkg", "1.0", 100)).await;
        let pkg = db.get_package_by_name(&"empty-builds-pkg".to_string()).await.unwrap().unwrap();

        let results = db.get_build_results(pkg.id).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_get_build_result_by_id() {
        let db = setup_db().await;
        db.update_metadata(&make_package_search_result("result-by-id-pkg", "1.0", 100)).await;
        let pkg = db.get_package_by_name(&"result-by-id-pkg".to_string()).await.unwrap().unwrap();

        let task = BuildTaskTransmissionFormat {
            id: pkg.id,
            name: "result-by-id-pkg".to_string(),
            version: "1.0".to_string(),
            source: None,
            subfolder: None,
            options: None,
            env: None,
        };
        db.save_build_results(&make_build_result(task, true, 0)).await.unwrap();

        let results = db.get_build_results(pkg.id).await.unwrap();
        let result_id = results[0].id;

        let found = db.get_build_result(result_id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, result_id);
    }

    #[tokio::test]
    async fn test_get_build_result_nonexistent_id_returns_none() {
        let db = setup_db().await;
        let result = db.get_build_result(99999).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_update_metadata_stores_source_and_subfolder() {
        let db = setup_db().await;
        let data = PackageSearchResult {
            name: "git-pkg".to_string(),
            version: "1.0.0".to_string(),
            maintainer: "dev".to_string(),
            last_modified: 1000,
            source: Some("https://github.com/example/repo".to_string()),
            subfolder: Some("subdir".to_string()),
            options: None,
            environment: None,
        };
        db.update_metadata(&data).await;
        let pkg = db.get_package_by_name(&"git-pkg".to_string()).await.unwrap().unwrap();
        assert_eq!(pkg.source, Some("https://github.com/example/repo".to_string()));
        assert_eq!(pkg.subfolder, Some("subdir".to_string()));
    }

    #[tokio::test]
    async fn test_build_results_ordered_by_started_at_descending() {
        let db = setup_db().await;
        db.update_metadata(&make_package_search_result("ordered-pkg", "1.0", 100)).await;
        let pkg = db.get_package_by_name(&"ordered-pkg".to_string()).await.unwrap().unwrap();

        let timestamps = [
            ("2024-01-01 09:00:00", "2024-01-01 09:05:00"),
            ("2024-01-01 10:00:00", "2024-01-01 10:05:00"),
            ("2024-01-01 11:00:00", "2024-01-01 11:05:00"),
        ];

        for (start_str, end_str) in &timestamps {
            let start = NaiveDateTime::parse_from_str(start_str, "%Y-%m-%d %H:%M:%S").unwrap();
            let end = NaiveDateTime::parse_from_str(end_str, "%Y-%m-%d %H:%M:%S").unwrap();
            let task = BuildTaskTransmissionFormat {
                id: pkg.id,
                name: "ordered-pkg".to_string(),
                version: "1.0".to_string(),
                source: None,
                subfolder: None,
                options: None,
                env: None,
            };
            let result = BuildResultTransmissionFormat {
                task,
                status_code: 0,
                log_lines: vec![],
                success: true,
                timestamps: Timestamps { start, end },
            };
            db.save_build_results(&result).await.unwrap();
        }

        let results = db.get_build_results(pkg.id).await.unwrap();
        assert_eq!(results.len(), 3);
        // Should be ordered newest first
        assert!(results[0].started_at >= results[1].started_at);
        assert!(results[1].started_at >= results[2].started_at);
    }
}
