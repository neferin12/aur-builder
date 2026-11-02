mod m20240624_000001_package_metadata_table;
mod m20250120_205654_build_results;
mod m20250120_223506_build_results_add_success;
mod m20250121_131141_build_results_add_finished_at;
mod m20250121_160432_build_results_add_started_at_and_version;
mod m20250316_173233_source;
mod m20250316_182755_subfolder;
mod m20250319_110511_switch_start_and_end_timestamps;
mod m20261101_000001_storage;
mod m20261102_000001_reviews;
mod m20261102_000002_review_notifications;
mod m20261102_000003_auth_and_observations;

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20240624_000001_package_metadata_table::Migration),
            Box::new(m20250120_205654_build_results::Migration),
            Box::new(m20250120_223506_build_results_add_success::Migration),
            Box::new(m20250121_131141_build_results_add_finished_at::Migration),
            Box::new(m20250121_160432_build_results_add_started_at_and_version::Migration),
            Box::new(m20250316_173233_source::Migration),
            Box::new(m20250316_182755_subfolder::Migration),
            Box::new(m20250319_110511_switch_start_and_end_timestamps::Migration),
            Box::new(m20261101_000001_storage::Migration),
            Box::new(m20261102_000001_reviews::Migration),
            Box::new(m20261102_000002_review_notifications::Migration),
            Box::new(m20261102_000003_auth_and_observations::Migration),
        ]
    }
}
