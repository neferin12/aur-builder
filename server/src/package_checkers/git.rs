use std::arch::x86_64::_mm256_cvtps_ph;
use std::error::Error;
use common::types::{GitPackageSettings, PackageSearchResult};
use git2::Repository;
use log::error;
use std::path::PathBuf;
use std::process::exit;
use srcinfo::Srcinfo;
use tempdir::TempDir;

pub async fn get_git_data(pkg: &GitPackageSettings) -> Result<PackageSearchResult, Box<dyn Error>> {
    let temp_dir = TempDir::new("aur-builder-git-tmp")?;
    let mut dir: PathBuf = temp_dir.into_path();
    let repo = Repository::clone(pkg.source.as_str(), &dir)?;
    if let Some(subfolder) = &pkg.subfolder {
        dir = dir.join(subfolder);
    }
    let srcinfo = Srcinfo::parse_file(&dir.join(".SRCINFO"))?;
    let head = repo.head()?.peel_to_commit()?;
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
        options: pkg.options.clone(),
        environment: pkg.env.clone()
    })
}
