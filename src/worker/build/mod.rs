use aur_builder_commons::types::{BuildResultTransmissionFormat, BuildTaskTransmissionFormat};

pub mod docker;

pub async fn build_package(task: &BuildTaskTransmissionFormat) -> Result<BuildResultTransmissionFormat, Box<dyn std::error::Error>> {
    let source_url = format!("https://aur.archlinux.org/{}.git", task.name);

    docker::build(task, source_url).await

}