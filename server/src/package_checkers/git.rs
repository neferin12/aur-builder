use std::arch::x86_64::_mm256_cvtps_ph;
use common::types::{GitPackageSettings, PackageSearchResult};
use git2::Repository;
use log::error;
use std::io::Error;
use std::path::PathBuf;
use std::process::exit;
use srcinfo::Srcinfo;
use tempdir::TempDir;

pub async fn get_git_data(pkg: &GitPackageSettings) -> Result<PackageSearchResult, Error> {
    let temp_dir = TempDir::new("aur-builder-git-tmp")?;
    let mut dir: PathBuf = temp_dir.into_path();
    let repo = match Repository::clone(pkg.source.as_str(), &dir) {
        Ok(repo) => repo,
        Err(e) => {
            error!("failed to clone: {}", e);
            exit(1)
        }
    };
    if let Some(subfolder) = &pkg.subfolder {
        dir = dir.join(subfolder);
    }
    let srcinfo = match Srcinfo::parse_file(&dir.join(".SRCINFO")) {
        Ok(srcinfo) => srcinfo,
        Err(e) => {
            error!("failed to read SRCINFO \"{}\": {}",&dir.join(".SRCINFO").to_str().unwrap(), e);
            exit(1)
        }
    };
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    let time = head.time();

    // Time since Unix epoch (seconds)
    let epoch_seconds = time.seconds();
    Ok(PackageSearchResult {
        name: srcinfo.base.pkgbase,
        version: srcinfo.base.pkgver,
        maintainer: "unknown".to_string(),
        last_modified: epoch_seconds,
        source: Some(pkg.source.clone()),
        subfolder: pkg.subfolder.clone(),
    })
}
