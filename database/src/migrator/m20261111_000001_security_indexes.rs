//! Keep paginated history/reviews and expired-auth cleanup off full table scans.
use sea_orm::ConnectionTrait;
use sea_orm_migration::prelude::*;
#[derive(DeriveMigrationName)]
pub struct Migration;
const INDEXES: &[(&str, &str)] = &[
    (
        "build_results_package_page",
        "build_results (package_id, id DESC)",
    ),
    (
        "candidate_state_page",
        "release_candidate (state, created_at DESC, id DESC)",
    ),
    (
        "candidate_all_page",
        "release_candidate (created_at DESC, id DESC)",
    ),
    ("oidc_login_expiry", "oidc_login (expires_at)"),
    ("web_session_expiry", "web_session (expires_at)"),
];
#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for (name, table) in INDEXES {
            manager
                .get_connection()
                .execute_unprepared(&format!("CREATE INDEX IF NOT EXISTS {name} ON {table}"))
                .await?;
        }
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for (name, _) in INDEXES {
            manager
                .get_connection()
                .execute_unprepared(&format!("DROP INDEX IF EXISTS {name}"))
                .await?;
        }
        Ok(())
    }
}
