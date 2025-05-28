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
