use common::types::{BuildResultTransmissionFormat, BuildTaskTransmissionFormat};

pub mod docker;

pub async fn build_package(task: &BuildTaskTransmissionFormat) -> Result<BuildResultTransmissionFormat, Box<dyn std::error::Error>> {
    let source_url = match &task.source {
        None => { format!("https://aur.archlinux.org/{}.git", task.name) }
        Some(s) => { s.to_owned() }
    };
    
    docker::build(task, source_url, &task.subfolder).await

}