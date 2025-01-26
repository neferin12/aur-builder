use npm_rs::NpmEnv;
use std::fs;

fn main() {
    let npm = NpmEnv::default()
        .set_path("src/web/bootstrap")
        .init_env();
    npm
        .install(None)
        .run("build")
        .exec().unwrap();
    fs::copy("src/web/bootstrap/dist/bootstrap.css", "src/web/assets/bootstrap.css").unwrap();
}