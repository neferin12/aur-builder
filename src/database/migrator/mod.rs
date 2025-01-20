mod m20240624_000001_package_metadata_table;
mod m20250120_205654_build_results;
mod m20250120_223506_build_results_add_success;

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20240624_000001_package_metadata_table::Migration),
            Box::new(m20250120_205654_build_results::Migration),
            Box::new(m20250120_223506_build_results_add_success::Migration),
        ]
    }
}
