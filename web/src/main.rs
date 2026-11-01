use axum::extract::Path;
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Extension, Router};
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
    let statuses = db.latest_build_statuses().await.unwrap();
    let successful = statuses.values().filter(|&&s| s).count();
    let failed = statuses.values().filter(|&&s| !s).count();
    let mut rows = to_value(&packages).unwrap();
    for (row, package) in rows.as_array_mut().unwrap().iter_mut().zip(&packages) {
        row.as_object_mut().unwrap().insert(
            "build_status".into(),
            to_value(match statuses.get(&package.id) {
                Some(true) => "success",
                Some(false) => "failed",
                None => "unbuilt",
            })
            .unwrap(),
        );
    }
    context.insert("successful", &successful);
    context.insert("failed", &failed);
    context.insert(
        "unbuilt",
        &(packages.len().saturating_sub(successful + failed)),
    );

    context.insert("version", VERSION);
    context.insert("current_page", "packages");

    context.insert("packages", &rows);

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

    context.insert("current_page", "details");
    context.insert("package", &package);

    let build_results = db.get_build_results(package.id).await.unwrap();
    context.insert("build_results", &build_results);

    Ok(Html(tera.render("build-results.html", &context).unwrap()))
}

async fn render_build_log_function(
    Extension(tera): Extension<Tera>,
    Extension(db): Extension<Database>,
    Path(pid): Path<i32>,
) -> Result<Html<String>, StatusCode> {
    let mut context = Context::new();

    let build_result = db
        .get_build_result(pid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let package = db
        .get_package(build_result.package_id as i32)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    context.insert("version", VERSION);
    context.insert("current_page", "details");
    context.insert("package", &package);
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

    context.insert("current_page", "details");
    context.insert("package", &package);

    db.reset_package_last_modified(package.id).await;

    Ok(Html(tera.render("force-rebuild.html", &context).unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn render_pages_and_escape_logs() {
        let mut tera = Tera::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/**/*.html"
        ))
        .unwrap();
        tera.register_filter("err_desc", error_desc_filter);
        let package = json!({"id":1,"name":"example","version":"1.0-1","maintainer":"alice","last_modified":1700000000,"source":null,"build_status":"success"});
        let result = json!({"id":2,"package_id":1,"success":false,"version":"1.0-1","exit_code":105,"started_at":null,"finished_at":null,"build_log":"stdout: hello\nstderr: <script>alert(1)</script>"});
        let mut context=Context::from_value(json!({"version":"test","current_page":"packages","packages":[package.clone()],"package":package,"successful":1,"failed":0,"unbuilt":0,"build_results":[result.clone()],"build_result":result})).unwrap();
        for name in [
            "index.html",
            "build-results.html",
            "build-log.html",
            "force-rebuild.html",
        ] {
            let rendered = tera.render(name, &context).unwrap();
            assert!(rendered.contains("AUR Builder"));
            assert!(!rendered.contains("<script>alert(1)</script>"));
            assert!(!rendered.contains("1970-01-01"));
        }
        let log = tera.render("build-log.html", &context).unwrap();
        assert!(log.contains("&lt;script&gt;"));
        assert!(log.contains("Failed"));
        context.insert("packages", &Vec::<Value>::new());
        context.insert("build_results", &Vec::<Value>::new());
        assert!(
            tera.render("index.html", &context)
                .unwrap()
                .contains("No packages tracked yet")
        );
        assert!(
            tera.render("build-results.html", &context)
                .unwrap()
                .contains("No builds recorded")
        );
    }
}
