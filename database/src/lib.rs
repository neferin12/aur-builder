use log::{LevelFilter, error};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, ConnectOptions, DatabaseConnection, DbErr,
    EntityTrait, QueryFilter, QueryOrder,
};
use sea_orm_migration::MigratorTrait;
use std::process::exit;
use std::time::Duration;
use tokio::time::sleep;

pub mod access;
pub mod entities;
pub mod migrator;
pub mod reviews;

use common::environment::get_environment_variable;
use common::types::{BuildResultTransmissionFormat, PackageSearchResult};
use common::{CONNECTION_RETRY_NUMBER, RETRY_TIMEOUT};
use entities::prelude::*;
use entities::*;
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
    /// use database::Database;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), sea_orm::DbErr> {
    /// let db = Database::new("sqlite::memory:".to_owned()).await?;
    /// # Ok(())
    /// # }
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
    /// use database::Database;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), sea_orm::DbErr> {
    /// let db = Database::new("sqlite::memory:".to_owned()).await?;
    /// db.migrate().await;
    /// assert!(db.get_packages().await?.is_empty());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn migrate(&self) {
        println!("Applying migrations...");
        Migrator::up(&self.db, None).await.unwrap();
    }

    /// This asynchronous function updates the metadata of a package in the database.
    ///
    /// # Arguments
    ///
    /// * `data` - A reference to `PackageSearchResult` that holds the metadata of the package.
    ///
    /// # Returns
    ///
    /// * `bool` - A boolean value that indicates whether the metadata was updated.
    ///
    /// # Example
    ///
    /// ```
    /// use common::types::PackageSearchResult;
    /// use database::Database;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), sea_orm::DbErr> {
    /// let db = Database::new("sqlite::memory:".to_owned()).await?;
    /// db.migrate().await;
    /// let data = PackageSearchResult {
    ///     name: "example".to_owned(),
    ///     version: "1.0-1".to_owned(),
    ///     maintainer: "builder".to_owned(),
    ///     last_modified: 1,
    ///     source: None,
    ///     subfolder: None,
    ///     options: None,
    ///     environment: None,
    /// };
    /// assert!(db.update_metadata(&data).await);
    /// assert!(!db.update_metadata(&data).await);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn update_metadata(&self, data: &PackageSearchResult) -> bool {
        let mut new_timestamp = false;

        let existing = self.get_package_by_name(&data.name).await.unwrap();

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

    pub async fn get_package_by_name(
        &self,
        name: &String,
    ) -> Result<Option<package_metadata::Model>, DbErr> {
        PackageMetadata::find()
            .filter(package_metadata::Column::Name.eq(name))
            .one(&self.db)
            .await
    }

    pub async fn reset_package_last_modified(&self, id: i32) {
        let p = PackageMetadata::find_by_id(id)
            .one(&self.db)
            .await
            .unwrap()
            .unwrap();
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

    pub async fn latest_build_statuses(
        &self,
    ) -> Result<std::collections::HashMap<i32, bool>, DbErr> {
        use sea_orm::ConnectionTrait;
        let rows=self.db.query_all(sea_orm::Statement::from_string(self.db.get_database_backend(),"SELECT b.package_id,b.success FROM build_results b WHERE b.id=(SELECT r.id FROM build_results r WHERE r.package_id=b.package_id ORDER BY (r.started_at IS NULL) ASC,r.started_at DESC,r.id DESC LIMIT 1)".to_string())).await?;
        rows.into_iter()
            .map(|r| {
                Ok((
                    r.try_get::<i64>("", "package_id")? as i32,
                    r.try_get("", "success")?,
                ))
            })
            .collect()
    }
    pub async fn get_build_result(
        &self,
        build_result_id: i32,
    ) -> Result<Option<build_results::Model>, DbErr> {
        BuildResults::find_by_id(build_result_id)
            .one(&self.db)
            .await
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

impl Database {
    pub(crate) async fn storage_query(
        &self,
        sql: &str,
        values: Vec<sea_orm::Value>,
    ) -> Result<sea_orm::Statement, DbErr> {
        use sea_orm::ConnectionTrait;
        let backend = self.db.get_database_backend();
        let sql = if backend == sea_orm::DatabaseBackend::Postgres {
            let mut n = 0;
            sql.chars()
                .map(|c| {
                    if c == '?' {
                        n += 1;
                        format!("${n}")
                    } else {
                        c.to_string()
                    }
                })
                .collect::<String>()
        } else {
            sql.to_owned()
        };
        Ok(sea_orm::Statement::from_sql_and_values(
            backend, sql, values,
        ))
    }
    /// Active attempts in dispatch order; upload credentials are never exposed.
    pub async fn build_activity(&self) -> Result<Vec<serde_json::Value>, DbErr> {
        use sea_orm::ConnectionTrait;
        let query = self.storage_query(
            "SELECT task,state FROM build_attempt WHERE state IN ('queued','dispatched','building') ORDER BY created_at,attempt",
            vec![],
        ).await?;
        let mut activity = Vec::new();
        for row in self.db.query_all(query).await? {
            let task: common::types::BuildTaskTransmissionFormat =
                serde_json::from_str(&row.try_get::<String>("", "task")?)
                    .map_err(|e| DbErr::Custom(e.to_string()))?;
            activity.push(serde_json::json!({"id":task.id,"name":task.name,"version":task.version,"state":row.try_get::<String>("", "state")?}));
        }
        Ok(activity)
    }
    pub async fn pending_attempts(
        &self,
    ) -> Result<Vec<common::types::BuildTaskTransmissionFormat>, DbErr> {
        use sea_orm::ConnectionTrait;
        let query = self
            .storage_query(
                "SELECT task FROM build_attempt WHERE state='queued' ORDER BY created_at",
                vec![],
            )
            .await?;
        let mut tasks = Vec::new();
        for row in self.db.query_all(query).await? {
            let json: String = row.try_get("", "task")?;
            tasks.push(serde_json::from_str(&json).map_err(|e| DbErr::Custom(e.to_string()))?);
        }
        Ok(tasks)
    }
    async fn attempt_insert(
        &self,
        task: &common::types::BuildTaskTransmissionFormat,
    ) -> Result<sea_orm::Statement, DbErr> {
        let mut clean = task.clone();
        clean.upload = None;
        self.storage_query("INSERT INTO build_attempt (attempt,package_id,object_key,task,created_at) VALUES (?,?,?,?,?)",vec![task.attempt.clone().into(),task.id.into(),task.object_key.clone().into(),serde_json::to_string(&clean).map_err(|e|DbErr::Custom(e.to_string()))?.into(),sea_orm::sqlx::types::chrono::Utc::now().timestamp_nanos_opt().expect("timestamp out of range").into()]).await
    }
    pub async fn record_attempt(
        &self,
        task: &common::types::BuildTaskTransmissionFormat,
    ) -> Result<(), DbErr> {
        use sea_orm::ConnectionTrait;
        self.db.execute(self.attempt_insert(task).await?).await?;
        Ok(())
    }
    pub async fn valid_attempt(
        &self,
        task: &common::types::BuildTaskTransmissionFormat,
    ) -> Result<bool, DbErr> {
        use sea_orm::ConnectionTrait;
        if !self.approved_task(task).await? {
            return Ok(false);
        }
        let q = self
            .storage_query(
                "SELECT task FROM build_attempt WHERE attempt=? AND package_id=? AND object_key=?",
                vec![
                    task.attempt.clone().into(),
                    task.id.into(),
                    task.object_key.clone().into(),
                ],
            )
            .await?;
        let Some(row) = self.db.query_one(q).await? else {
            return Ok(false);
        };
        let expected: String = row.try_get("", "task")?;
        let mut clean = task.clone();
        clean.upload = None;
        Ok(expected == serde_json::to_string(&clean).unwrap())
    }
    pub async fn latest_attempt(
        &self,
        task: &common::types::BuildTaskTransmissionFormat,
    ) -> Result<bool, DbErr> {
        use sea_orm::ConnectionTrait;
        let q=self.storage_query("SELECT attempt FROM build_attempt WHERE package_id=? ORDER BY created_at DESC, attempt DESC LIMIT 1",vec![task.id.into()]).await?;
        Ok(self
            .db
            .query_one(q)
            .await?
            .map(|r| r.try_get::<String>("", "attempt").unwrap_or_default() == task.attempt)
            .unwrap_or(false))
    }
    pub async fn attempt_needs_build(
        &self,
        task: &common::types::BuildTaskTransmissionFormat,
    ) -> Result<bool, DbErr> {
        use sea_orm::ConnectionTrait;
        let q=self.storage_query("SELECT attempt FROM build_attempt WHERE attempt=? AND state IN ('queued','dispatched','building')",vec![task.attempt.clone().into()]).await?;
        Ok(self.db.query_one(q).await?.is_some())
    }
    /// Retire waiting attempts replaced by a newer attempt for the same package.
    pub async fn cleanup_orphan_waiting_attempts(&self) -> Result<u64, DbErr> {
        use sea_orm::ConnectionTrait;
        let query = self.storage_query(
            "UPDATE build_attempt SET state='superseded' WHERE state IN ('queued','dispatched') AND EXISTS (SELECT 1 FROM build_attempt AS newer WHERE newer.package_id=build_attempt.package_id AND newer.created_at>build_attempt.created_at)",
            vec![],
        ).await?;
        Ok(self.db.execute(query).await?.rows_affected())
    }
    pub async fn finish_attempt(&self, attempt: &str) -> Result<(), DbErr> {
        use sea_orm::ConnectionTrait;
        let q=self.storage_query("UPDATE build_attempt SET state='finished' WHERE attempt=? AND state IN ('queued','dispatched','building')",vec![attempt.into()]).await?;
        self.db.execute(q).await?;
        Ok(())
    }
    pub async fn attempt_state(&self, attempt: &str, state: &str) -> Result<(), DbErr> {
        use sea_orm::ConnectionTrait;
        let q = self
            .storage_query(
                if state == "dispatched" {
                    "UPDATE build_attempt SET state=? WHERE attempt=? AND state='queued'"
                } else {
                    "UPDATE build_attempt SET state=? WHERE attempt=?"
                },
                vec![state.into(), attempt.into()],
            )
            .await?;
        self.db.execute(q).await?;
        Ok(())
    }
    pub async fn retention_attempts(
        &self,
    ) -> Result<Vec<common::types::BuildTaskTransmissionFormat>, DbErr> {
        use sea_orm::ConnectionTrait;
        let q = self
            .storage_query(
                "SELECT task FROM build_attempt ORDER BY created_at, attempt",
                vec![],
            )
            .await?;
        self.db
            .query_all(q)
            .await?
            .into_iter()
            .map(|row| {
                let task: String = row.try_get("", "task")?;
                serde_json::from_str(&task).map_err(|e| DbErr::Custom(e.to_string()))
            })
            .collect()
    }
    pub async fn load_catalog(&self) -> Result<String, DbErr> {
        use sea_orm::ConnectionTrait;
        let q = self
            .storage_query("SELECT catalog FROM repo_catalog WHERE id=1", vec![])
            .await?;
        Ok(self
            .db
            .query_one(q)
            .await?
            .map(|r| r.try_get("", "catalog").unwrap())
            .unwrap_or("{}".into()))
    }
    /// Check attempt freshness and switch the catalog in one SQL statement.
    pub async fn publish_catalog(
        &self,
        task: &common::types::BuildTaskTransmissionFormat,
        catalog: &str,
    ) -> Result<bool, DbErr> {
        use sea_orm::ConnectionTrait;
        if !self.valid_attempt(task).await? {
            return Ok(false);
        }
        let q=self.storage_query("INSERT INTO repo_catalog (id,catalog) SELECT 1,? WHERE ?=(SELECT attempt FROM build_attempt WHERE package_id=? ORDER BY created_at DESC,attempt DESC LIMIT 1) ON CONFLICT(id) DO UPDATE SET catalog=excluded.catalog",vec![catalog.into(),task.attempt.clone().into(),task.id.into()]).await?;
        Ok(self.db.execute(q).await?.rows_affected() == 1)
    }
    pub async fn save_catalog(&self, catalog: &str) -> Result<(), DbErr> {
        use sea_orm::ConnectionTrait;
        let q=self.storage_query("INSERT INTO repo_catalog (id,catalog) VALUES (1,?) ON CONFLICT(id) DO UPDATE SET catalog=excluded.catalog",vec![catalog.into()]).await?;
        self.db.execute(q).await?;
        Ok(())
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;
    #[tokio::test]
    async fn cleanup_only_retires_older_waiting_attempts() {
        use sea_orm::ConnectionTrait;
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        for (attempt, package, state, created) in [
            ("old-queued", 1, "queued", 1),
            ("old-dispatched", 1, "dispatched", 2),
            ("running", 1, "building", 3),
            ("finished", 1, "finished", 4),
            ("latest", 1, "queued", 5),
            ("other-package", 2, "dispatched", 1),
        ] {
            let query = db.storage_query(
                "INSERT INTO build_attempt (attempt,package_id,object_key,task,state,created_at) VALUES (?,?,?,'{}',?,?)",
                vec![attempt.into(), package.into(), attempt.into(), state.into(), created.into()],
            ).await.unwrap();
            db.db.execute(query).await.unwrap();
        }
        assert_eq!(db.cleanup_orphan_waiting_attempts().await.unwrap(), 2);
        for (attempt, expected) in [
            ("old-queued", "superseded"), ("old-dispatched", "superseded"),
            ("running", "building"), ("finished", "finished"),
            ("latest", "queued"), ("other-package", "dispatched"),
        ] {
            let query = db.storage_query("SELECT state FROM build_attempt WHERE attempt=?", vec![attempt.into()]).await.unwrap();
            let row = db.db.query_one(query).await.unwrap().unwrap();
            assert_eq!(row.try_get::<String>("", "state").unwrap(), expected);
        }
        assert_eq!(db.cleanup_orphan_waiting_attempts().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn latest_status_uses_build_start_order() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let data = PackageSearchResult {
            name: "status".into(),
            version: "1".into(),
            maintainer: "test".into(),
            last_modified: 1,
            source: None,
            subfolder: None,
            options: None,
            environment: None,
        };
        let task = approved_fixture(&db, &data, "status-1").await;
        let now = sea_orm::sqlx::types::chrono::Utc::now().naive_utc();
        let mut result = BuildResultTransmissionFormat {
            task: task.clone(),
            status_code: 0,
            log_lines: vec![],
            success: true,
            timestamps: common::types::Timestamps {
                start: now,
                end: now,
            },
        };
        db.save_build_results(&result).await.unwrap();
        result.success = false;
        result.status_code = 105;
        result.timestamps.start = now - std::time::Duration::from_secs(600);
        db.save_build_results(&result).await.unwrap();
        assert_eq!(
            db.latest_build_statuses().await.unwrap().get(&task.id),
            Some(&true)
        );
    }
    #[tokio::test]
    async fn attempts_validation_and_catalog_replacement() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let data = PackageSearchResult {
            name: "test".into(),
            version: "1".into(),
            maintainer: "test".into(),
            last_modified: 1,
            source: Some("https://example.com/source.git".into()),
            subfolder: None,
            options: None,
            environment: None,
        };
        let mut task = approved_fixture(&db, &data, "a").await;
        assert!(db.valid_attempt(&task).await.unwrap());
        assert!(db.latest_attempt(&task).await.unwrap());
        task.object_key = "another-object".into();
        assert!(!db.valid_attempt(&task).await.unwrap());
        task.attempt = "b".into();
        db.record_attempt(&task).await.unwrap();
        assert!(db.latest_attempt(&task).await.unwrap());
        task.attempt = "a".into();
        assert!(!db.latest_attempt(&task).await.unwrap());
        assert!(!db.publish_catalog(&task, "stale").await.unwrap());
        assert_eq!(db.load_catalog().await.unwrap(), "{}");
        task.attempt = "b".into();
        assert!(db.publish_catalog(&task, "current").await.unwrap());
        assert_eq!(db.load_catalog().await.unwrap(), "current");
        task.revision = None;
        assert!(!db.valid_attempt(&task).await.unwrap());
        assert!(!db.publish_catalog(&task, "legacy").await.unwrap());
    }
    async fn approved_fixture(
        db: &Database,
        data: &PackageSearchResult,
        attempt: &str,
    ) -> common::types::BuildTaskTransmissionFormat {
        db.observe_release(
            data,
            &"a".repeat(40),
            "first",
            &reviews::Snapshot {
                files: vec![],
                complete: true,
            },
        )
        .await
        .unwrap();
        let c = db.candidates().await.unwrap().remove(0);
        assert!(
            db.decide_release(&c.id, "reviewer", true, "", |t| {
                t.attempt = attempt.into();
                t.object_key = format!("builds/{attempt}/bundle.tar");
                Ok(())
            })
            .await
            .unwrap()
        );
        db.pending_attempts().await.unwrap().remove(0)
    }
}
