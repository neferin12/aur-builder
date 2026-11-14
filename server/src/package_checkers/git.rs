use common::types::{GitPackageSettings, PackageSearchResult};
use git2::Repository;
use srcinfo::Srcinfo;
use std::{error::Error, path::Path};

pub async fn get_git_data(pkg: &GitPackageSettings) -> Result<PackageSearchResult, Box<dyn Error>> {
    let temp_dir = tempfile::tempdir()?;
    let mut dir = temp_dir.as_ref().canonicalize()?;
    let repo = Repository::clone(pkg.source.as_str(), &dir)?;
    if let Some(subfolder) = &pkg.subfolder {
        if Path::new(subfolder)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err("invalid source subfolder".into());
        }
        let root = dir.clone();
        dir = dir.join(subfolder).canonicalize()?;
        if !dir.starts_with(root) {
            return Err("source subfolder escapes checkout".into());
        }
    }
    let metadata_path = dir.join(".SRCINFO");
    let metadata = std::fs::symlink_metadata(&metadata_path)?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err("source metadata must be a bounded regular file".into());
    }
    let srcinfo = Srcinfo::parse_file(&metadata_path)?;
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
        environment: pkg.env.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rejects_metadata_symlinks_and_subfolder_escapes() {
        let source = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let metadata =
            "pkgbase = fixture\n\tpkgver = 1\n\tpkgrel = 1\n\tarch = any\npkgname = fixture\n";
        std::fs::write(outside.path().join(".SRCINFO"), metadata).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join(".SRCINFO"),
            source.path().join(".SRCINFO"),
        )
        .unwrap();
        let repo = Repository::init(source.path()).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(".SRCINFO")).unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Fixture", "fixture@localhost").unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .unwrap();
        let mut package = GitPackageSettings {
            source: source.path().to_string_lossy().into_owned(),
            subfolder: None,
            env: None,
            options: None,
        };
        assert!(
            get_git_data(&package)
                .await
                .unwrap_err()
                .to_string()
                .contains("bounded regular file")
        );
        package.subfolder = Some("..".into());
        assert!(
            get_git_data(&package)
                .await
                .unwrap_err()
                .to_string()
                .contains("invalid source subfolder")
        );
    }
}
