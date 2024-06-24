use aur_builder_commons::environment::get_environment_variable;
use std::env;
use aur_builder_commons::database::Database;


#[tokio::main]
async fn main() {
let database_url = get_environment_variable("DATABASE_URL");

    let db = Database::new(database_url).await.unwrap();
}