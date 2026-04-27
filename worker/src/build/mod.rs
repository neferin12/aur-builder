use common::types::{BuildResultTransmissionFormat, BuildTaskTransmissionFormat};

pub mod docker;

pub async fn build_package(task: &BuildTaskTransmissionFormat) -> Result<BuildResultTransmissionFormat, Box<dyn std::error::Error>> {
    let source_url = match &task.source {
        None => { format!("https://aur.archlinux.org/{}.git", task.name) }
        Some(s) => { s.to_owned() }
    };
    
    docker::build(task, source_url, &task.subfolder).await

}
#[cfg(test)]
mod tests {
    use common::types::BuildTaskTransmissionFormat;

    fn make_task(name: &str, source: Option<String>) -> BuildTaskTransmissionFormat {
        BuildTaskTransmissionFormat {
            id: 1,
            name: name.to_string(),
            version: "1.0.0".to_string(),
            source,
            subfolder: None,
            options: None,
            env: None,
        }
    }

    #[test]
    fn test_source_url_for_aur_package_uses_aur_git_url() {
        let task = make_task("my-aur-package", None);
        let source_url = match &task.source {
            None => format!("https://aur.archlinux.org/{}.git", task.name),
            Some(s) => s.to_owned(),
        };
        assert_eq!(source_url, "https://aur.archlinux.org/my-aur-package.git");
    }

    #[test]
    fn test_source_url_for_git_package_uses_provided_url() {
        let task = make_task("custom-pkg", Some("https://github.com/example/repo.git".to_string()));
        let source_url = match &task.source {
            None => format!("https://aur.archlinux.org/{}.git", task.name),
            Some(s) => s.to_owned(),
        };
        assert_eq!(source_url, "https://github.com/example/repo.git");
    }

    #[test]
    fn test_source_url_for_package_with_special_chars_in_name() {
        let task = make_task("lib32-some-pkg", None);
        let source_url = match &task.source {
            None => format!("https://aur.archlinux.org/{}.git", task.name),
            Some(s) => s.to_owned(),
        };
        assert_eq!(source_url, "https://aur.archlinux.org/lib32-some-pkg.git");
    }
}
