//! Apply the authoritative schema without starting the scheduler or connecting to RabbitMQ.
#[tokio::main]
async fn main() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL is required");
    database::Database::new(url).await.expect("connect database").migrate().await;
}
