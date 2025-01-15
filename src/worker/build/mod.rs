mod docker;

pub async fn build_package(name: &String) -> Result<(), Box<dyn std::error::Error>> {
    let source_url = format!("https://aur.archlinux.org/{name}.git");

    docker::build(name, source_url).await?;

    return Ok(());
}