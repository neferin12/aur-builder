use axum::extract::Path;
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Extension, Router};
use cached::proc_macro::cached;
use common::environment::{VERSION, load_dotenv};
use common::errors::get_error_descriptions;
use database::entities::package_metadata;
use database::{Database, connect_to_db};
use log::{error, info};
use reqwest::StatusCode;
use std::collections::HashMap;
use std::process::exit;
use tera::{Context, Tera, Value, to_value};

fn error_desc_filter(error: &Value, _: &HashMap<String, Value>) -> Result<Value, tera::Error> {
    let code = error.as_i64().unwrap_or(-1);
    Ok(to_value(get_error_descriptions(code))?)
}

#[tokio::main]
async fn main() {
    load_dotenv().ok();
    simple_logger::init_with_env().unwrap();

    info!("Starting Aur-Builder Web v{VERSION}");

    let db = connect_to_db().await;
    let mut tera = match Tera::new("web/src/templates/**/*.html") {
        Ok(t) => t,
        Err(e) => {
            error!("Parsing error(s): {}", e);
            exit(1);
        }
    };
    tera.register_filter("err_desc", error_desc_filter);

    // build our application with a route
    let app = Router::new()
        // `GET /` goes to `root`
        .route("/", get(render_packages_function))
        .route("/build-results/{pid}", get(render_build_results_function))
        .route("/build-log/{pid}", get(render_build_log_function))
        .route("/force-rebuild/{pid}", post(init_force_rebuild))
        .nest_service(
            "/assets",
            tower_http::services::ServeDir::new("web/src/assets"),
        )
        .layer(Extension(tera))
        .layer(Extension(db));

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn render_packages_function(
    Extension(tera): Extension<Tera>,
    Extension(db): Extension<Database>,
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
    Path(pid): Path<i32>,
) -> Result<Html<String>, StatusCode> {
    let mut context = Context::new();

    let package: package_metadata::Model = match db.get_package(pid).await.unwrap() {
        None => {
            return Err(StatusCode::NOT_FOUND);
        }
        Some(p) => p,
    };
    context.insert("version", VERSION);

    context.insert("package", &package);

    let build_results = db.get_build_results(package.id).await.unwrap();
    context.insert("build_results", &build_results);

    Ok(Html(tera.render("build-results.html", &context).unwrap()))
}

#[cached(
    key = "String",
    convert = r#"{ format!("logs-{}",pid) }"#,
    result = true
)]
async fn render_build_log_function(
    Extension(tera): Extension<Tera>,
    Extension(db): Extension<Database>,
    Path(pid): Path<i32>,
) -> Result<Html<String>, StatusCode> {
    let mut context = Context::new();

    let build_result = db.get_build_result(pid).await.unwrap();
    context.insert("build_result", &build_result);

    Ok(Html(tera.render("build-log.html", &context).unwrap()))
}

async fn init_force_rebuild(
    Extension(tera): Extension<Tera>,
    Extension(db): Extension<Database>,
    Path(pid): Path<i32>,
) -> Result<Html<String>, StatusCode> {
    let mut context = Context::new();

    let package: package_metadata::Model = match db.get_package(pid).await.unwrap() {
        None => {
            return Err(StatusCode::NOT_FOUND);
        }
        Some(p) => p,
    };
    context.insert("version", VERSION);

    context.insert("package", &package);

    db.reset_package_last_modified(package.id).await;

    Ok(Html(tera.render("force-rebuild.html", &context).unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn setup_test_app() -> Router {
        let db = database::Database::new("sqlite::memory:".to_string())
            .await
            .unwrap();
        db.migrate().await;

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let pattern = format!("{manifest_dir}/src/templates/**/*.html");
        let mut tera = Tera::new(&pattern).unwrap();
        tera.register_filter("err_desc", error_desc_filter);

        Router::new()
            .route("/", get(render_packages_function))
            .route("/build-results/{pid}", get(render_build_results_function))
            .route("/build-log/{pid}", get(render_build_log_function))
            .route("/force-rebuild/{pid}", post(init_force_rebuild))
            .layer(Extension(tera))
            .layer(Extension(db))
    }

    #[test]
    fn test_error_desc_filter_known_code() {
        let value = tera::to_value(0i64).unwrap();
        let result = error_desc_filter(&value, &std::collections::HashMap::new()).unwrap();
        assert_eq!(result.as_str().unwrap(), "Success");
    }

    #[test]
    fn test_error_desc_filter_build_failed_code() {
        let value = tera::to_value(105i64).unwrap();
        let result = error_desc_filter(&value, &std::collections::HashMap::new()).unwrap();
        assert_eq!(result.as_str().unwrap(), "Failed to build package");
    }

    #[test]
    fn test_error_desc_filter_unknown_code() {
        let value = tera::to_value(9999i64).unwrap();
        let result = error_desc_filter(&value, &std::collections::HashMap::new()).unwrap();
        assert_eq!(result.as_str().unwrap(), "Unknown error");
    }

    #[test]
    fn test_error_desc_filter_negative_code() {
        let value = tera::to_value(-1i64).unwrap();
        let result = error_desc_filter(&value, &std::collections::HashMap::new()).unwrap();
        assert_eq!(result.as_str().unwrap(), "Unknown error");
    }

    #[tokio::test]
    async fn test_index_route_returns_ok() {
        let app = setup_test_app().await;
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_index_route_returns_html_content() {
        let app = setup_test_app().await;
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("AUR Builder") || body_str.contains("Packages"));
    }

    #[tokio::test]
    async fn test_build_results_route_returns_404_for_nonexistent_package() {
        let app = setup_test_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/build-results/99999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_force_rebuild_route_returns_404_for_nonexistent_package() {
        let app = setup_test_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/force-rebuild/99999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_build_results_route_returns_ok_for_existing_package() {
        let app = setup_test_app().await;
        let _ = app; // discard; we'll build our own app with data below

        // Create a fresh in-memory database with a known package
        let db = database::Database::new("sqlite::memory:".to_string())
            .await
            .unwrap();
        db.migrate().await;
        let data = common::types::PackageSearchResult {
            name: "test-web-pkg".to_string(),
            version: "1.0.0".to_string(),
            maintainer: "dev".to_string(),
            last_modified: 1000,
            source: None,
            subfolder: None,
            options: None,
            environment: None,
        };
        db.update_metadata(&data).await;
        let pkg = db
            .get_package_by_name(&"test-web-pkg".to_string())
            .await
            .unwrap()
            .unwrap();

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let pattern = format!("{manifest_dir}/src/templates/**/*.html");
        let mut tera = Tera::new(&pattern).unwrap();
        tera.register_filter("err_desc", error_desc_filter);

        let app_with_data = Router::new()
            .route("/build-results/{pid}", get(render_build_results_function))
            .layer(Extension(tera))
            .layer(Extension(db));

        let response = app_with_data
            .oneshot(
                Request::builder()
                    .uri(format!("/build-results/{}", pkg.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_force_rebuild_route_resets_package_timestamp() {
        let db = database::Database::new("sqlite::memory:".to_string())
            .await
            .unwrap();
        db.migrate().await;
        let data = common::types::PackageSearchResult {
            name: "rebuild-test-pkg".to_string(),
            version: "1.0.0".to_string(),
            maintainer: "dev".to_string(),
            last_modified: 5000,
            source: None,
            subfolder: None,
            options: None,
            environment: None,
        };
        db.update_metadata(&data).await;
        let pkg = db
            .get_package_by_name(&"rebuild-test-pkg".to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pkg.last_modified, 5000);

        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let pattern = format!("{manifest_dir}/src/templates/**/*.html");
        let mut tera = Tera::new(&pattern).unwrap();
        tera.register_filter("err_desc", error_desc_filter);

        let app = Router::new()
            .route("/force-rebuild/{pid}", post(init_force_rebuild))
            .layer(Extension(tera))
            .layer(Extension(db.clone()));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/force-rebuild/{}", pkg.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let updated = db
            .get_package_by_name(&"rebuild-test-pkg".to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.last_modified, 0, "Expected last_modified to be reset to 0");
    }
}
