use std::error::Error;
use common::types::{GitPackageSettings, PackageSearchResult};
use git2::Repository;
use srcinfo::Srcinfo;

pub async fn get_git_data(pkg: &GitPackageSettings) -> Result<PackageSearchResult, Box<dyn Error>> {
    let temp_dir = tempfile::tempdir()?;
    let mut dir = temp_dir.as_ref().canonicalize()?;
    let repo = Repository::clone(pkg.source.as_str(), &dir)?;
    if let Some(subfolder) = &pkg.subfolder {
        dir = dir.join(subfolder);
    }
    let srcinfo = Srcinfo::parse_file(&dir.join(".SRCINFO"))?;
    let head = repo.head()?.peel_to_commit()?;
    let time = head.time();

    // Time since Unix epoch (seconds)
    let epoch_seconds = time.seconds();
    temp_dir.close()?;
    Ok(PackageSearchResult {
        name: srcinfo.base.pkgbase,
        version: srcinfo.base.pkgver,
        maintainer: "unknown".to_string(),
        last_modified: epoch_seconds,
        source: Some(pkg.source.clone()),
        subfolder: pkg.subfolder.clone(),
        options: pkg.options.clone(),
        environment: pkg.env.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, Signature};
    use std::fs;
    use tempfile::tempdir;

    fn create_test_repo_with_srcinfo(srcinfo_content: &str) -> tempfile::TempDir {
        let temp_dir = tempdir().unwrap();
        let repo = Repository::init(temp_dir.path()).unwrap();

        let srcinfo_path = temp_dir.path().join(".SRCINFO");
        fs::write(&srcinfo_path, srcinfo_content).unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new(".SRCINFO")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = Signature::now("Test Author", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();

        temp_dir
    }

    #[tokio::test]
    async fn test_get_git_data_basic_package() {
        let srcinfo = "pkgbase = my-package\n\tpkgver = 1.2.3\n\tpkgrel = 1\n\tarch = x86_64\n\npkgname = my-package\n";
        let temp_dir = create_test_repo_with_srcinfo(srcinfo);
        let source_url = format!("file://{}", temp_dir.path().display());

        let pkg = GitPackageSettings {
            source: source_url,
            subfolder: None,
            env: None,
            options: None,
        };
        let result = get_git_data(&pkg).await;
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result.err());
        let data = result.unwrap();
        assert_eq!(data.name, "my-package");
        assert_eq!(data.version, "1.2.3");
        assert_eq!(data.maintainer, "unknown");
        assert!(data.last_modified > 0);
    }

    #[tokio::test]
    async fn test_get_git_data_preserves_source_url() {
        let srcinfo = "pkgbase = pkg-with-source\n\tpkgver = 2.0.0\n\tpkgrel = 1\n\tarch = x86_64\n\npkgname = pkg-with-source\n";
        let temp_dir = create_test_repo_with_srcinfo(srcinfo);
        let source_url = format!("file://{}", temp_dir.path().display());

        let pkg = GitPackageSettings {
            source: source_url.clone(),
            subfolder: None,
            env: None,
            options: None,
        };
        let result = get_git_data(&pkg).await.unwrap();
        assert_eq!(result.source, Some(source_url));
        assert!(result.subfolder.is_none());
    }

    #[tokio::test]
    async fn test_get_git_data_with_subfolder() {
        let temp_dir = tempdir().unwrap();
        let repo = Repository::init(temp_dir.path()).unwrap();

        let sub_dir = temp_dir.path().join("subpkg");
        fs::create_dir(&sub_dir).unwrap();
        let srcinfo = "pkgbase = sub-package\n\tpkgver = 0.5.0\n\tpkgrel = 1\n\tarch = x86_64\n\npkgname = sub-package\n";
        fs::write(sub_dir.join(".SRCINFO"), srcinfo).unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("subpkg/.SRCINFO")).unwrap();
        index.write().unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = Signature::now("Author", "a@b.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Init", &tree, &[]).unwrap();

        let source_url = format!("file://{}", temp_dir.path().display());
        let pkg = GitPackageSettings {
            source: source_url,
            subfolder: Some("subpkg".to_string()),
            env: None,
            options: None,
        };
        let result = get_git_data(&pkg).await.unwrap();
        assert_eq!(result.name, "sub-package");
        assert_eq!(result.version, "0.5.0");
        assert_eq!(result.subfolder, Some("subpkg".to_string()));
    }

    #[tokio::test]
    async fn test_get_git_data_preserves_options() {
        let srcinfo = "pkgbase = options-pkg\n\tpkgver = 1.0.0\n\tpkgrel = 1\n\tarch = x86_64\n\npkgname = options-pkg\n";
        let temp_dir = create_test_repo_with_srcinfo(srcinfo);
        let source_url = format!("file://{}", temp_dir.path().display());

        let pkg = GitPackageSettings {
            source: source_url,
            subfolder: None,
            env: None,
            options: Some("--sign".to_string()),
        };
        let result = get_git_data(&pkg).await.unwrap();
        assert_eq!(result.options, Some("--sign".to_string()));
    }

    #[tokio::test]
    async fn test_get_git_data_invalid_url_returns_error() {
        let pkg = GitPackageSettings {
            source: "file:///nonexistent/path/to/repo".to_string(),
            subfolder: None,
            env: None,
            options: None,
        };
        let result = get_git_data(&pkg).await;
        assert!(result.is_err());
    }
}
