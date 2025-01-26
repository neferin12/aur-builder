use aur_builder_commons::database::{connect_to_db, Database};
use aur_builder_commons::environment::{load_dotenv, VERSION};
use axum::routing::{get, post};
use axum::{Extension, Router};
use log::{error, info};
use std::process::exit;
use axum::extract::Path;
use axum::response::Html;
use cached::proc_macro::{cached};
use reqwest::StatusCode;
use tera::{Context, Tera};
use aur_builder_commons::database::entities::package_metadata;

#[tokio::main]
async fn main() {
    load_dotenv().unwrap();
    pretty_env_logger::init();

    info!("Starting Aur-Builder Web v{VERSION}");

    let db = connect_to_db().await;
    let tera = match Tera::new("src/web/templates/**/*.html") {
        Ok(t) => t,
        Err(e) => {
            error!("Parsing error(s): {}", e);
            exit(1);
        }
    };

    // build our application with a route
    let app = Router::new()
        // `GET /` goes to `root`
        .route("/", get(render_packages_function))
        .route("/build-results/{pid}", get(render_build_results_function))
        .route("/build-log/{pid}", get(render_build_log_function))
        .route("/force-rebuild/{pid}", post(init_force_rebuild))
        .nest_service("/assets", tower_http::services::ServeDir::new("src/web/assets"))
        .layer(Extension(tera))
        .layer(Extension(db));
    

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn render_packages_function(
    Extension(tera): Extension<Tera>,
    Extension(db): Extension<Database>
) -> Html<String> {
    let mut context = Context::new();
    let packages = db.get_packages().await.unwrap();

    context.insert("version", VERSION);

    context.insert("packages", &packages);

    Html(tera.render("index.html", &context).unwrap())
}

async fn render_build_results_function(
    Extension(tera): Extension<Tera>,
    Extension(db): Extension<Database>,
    Path(pid): Path<i32>
) -> Result<Html<String>, StatusCode> {
    let mut context = Context::new();

    let package: package_metadata::Model = match db.get_package(pid).await.unwrap() {
        None => {
            return Err(StatusCode::NOT_FOUND);
        }
        Some(p) => {p}
    };
    context.insert("version", VERSION);

    context.insert("package", &package);

    let build_results = db.get_build_results(package.id).await.unwrap();
    context.insert("build_results", &build_results);

    Ok(Html(tera.render("build-results.html", &context).unwrap()))
}

#[cached(
    key="String",
    convert=r#"{ format!("logs-{}",pid) }"#,
    result=true
)]
async fn render_build_log_function(
    Extension(tera): Extension<Tera>,
    Extension(db): Extension<Database>,
    Path(pid): Path<i32>
) -> Result<Html<String>, StatusCode> {
    let mut context = Context::new();
    

    let build_result = db.get_build_result(pid).await.unwrap();
    context.insert("build_result", &build_result);

    Ok(Html(tera.render("build-log.html", &context).unwrap()))
}

async fn init_force_rebuild(
    Extension(tera): Extension<Tera>,
    Extension(db): Extension<Database>,
    Path(pid): Path<i32>
) -> Result<Html<String>, StatusCode> {
    let mut context = Context::new();

    let package: package_metadata::Model = match db.get_package(pid).await.unwrap() {
        None => {
            return Err(StatusCode::NOT_FOUND);
        }
        Some(p) => {p}
    };
    context.insert("version", VERSION);
    
    context.insert("package", &package);

    db.reset_package_last_modified(package.id).await;

    Ok(Html(tera.render("force-rebuild.html", &context).unwrap()))
}
