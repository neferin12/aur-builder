use common::types::PackageSearchResult;
use database::reviews::{Snapshot, SourceFile};
use git2::{ObjectType, Repository};
use sha2::{Digest, Sha256};
use std::error::Error;
const MAX_FILE: usize = 512 * 1024;
const MAX_TOTAL: usize = 4 * 1024 * 1024;
fn walk(
    repo: &Repository,
    tree: &git2::Tree,
    prefix: &str,
    files: &mut Vec<SourceFile>,
    total: &mut usize,
    complete: &mut bool,
) -> Result<(), Box<dyn Error>> {
    for entry in tree.iter() {
        let path = format!(
            "{prefix}{}",
            entry.name().map_err(|_| "non UTF-8 source path")?
        );
        match entry.kind() {
            Some(ObjectType::Tree) => walk(
                repo,
                &repo.find_tree(entry.id())?,
                &format!("{path}/"),
                files,
                total,
                complete,
            )?,
            Some(ObjectType::Blob) => {
                let blob = repo.find_blob(entry.id())?;
                let size = blob.size();
                let content = if size <= MAX_FILE && total.saturating_add(size) <= MAX_TOTAL {
                    std::str::from_utf8(blob.content())
                        .ok()
                        .filter(|s| !s.contains('\0'))
                        .map(str::to_owned)
                } else {
                    None
                };
                if content.is_none() {
                    *complete = false;
                } else {
                    *total += size;
                }
                files.push(SourceFile {
                    path,
                    hash: entry.id().to_string(),
                    size,
                    mode: entry.filemode(),
                    content,
                });
            }
            _ => {
                *complete = false;
                files.push(SourceFile {
                    path,
                    hash: entry.id().to_string(),
                    size: 0,
                    mode: entry.filemode(),
                    content: None,
                });
            }
        }
    }
    Ok(())
}
pub fn capture(
    data: &mut PackageSearchResult,
) -> Result<(String, String, Snapshot), Box<dyn Error>> {
    let source = data.source.clone().ok_or("source repository missing")?;
    if let Some(sub) = &data.subfolder {
        if std::path::Path::new(sub)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err("invalid package subfolder".into());
        }
    }
    let dir = tempfile::tempdir()?;
    let repo = Repository::clone(&source, dir.path())?;
    let commit = repo.head()?.peel_to_commit()?;
    let revision = commit.id().to_string();
    let metadata_path = data
        .subfolder
        .as_ref()
        .map(|s| format!("{s}/.SRCINFO"))
        .unwrap_or(".SRCINFO".into());
    let tree = commit.tree()?;
    let entry = tree.get_path(std::path::Path::new(&metadata_path))?;
    let blob = repo.find_blob(entry.id())?;
    let info = srcinfo::Srcinfo::parse_buf(blob.content())?;
    data.version = info.version();
    let mut snapshot = Snapshot {
        files: Vec::new(),
        complete: true,
    };
    walk(
        &repo,
        &commit.tree()?,
        "",
        &mut snapshot.files,
        &mut 0,
        &mut snapshot.complete,
    )?;
    // Ignore timestamps and descriptive metadata; changes to build settings still need review.
    let fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(
        &serde_json::json!({"revision":revision,"source":source,"subfolder":data.subfolder,"options":data.options,"env":data.environment,"version":data.version}),
    )?));
    Ok((revision, fingerprint, snapshot))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captures_exact_source_version_and_settings() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        std::fs::write(dir.path().join("PKGBUILD"), "pkgver=2\n").unwrap();
        std::fs::write(
            dir.path().join(".SRCINFO"),
            "pkgbase = example\n\tpkgver = 2\n\tpkgrel = 3\n\tarch = any\npkgname = example\n",
        )
        .unwrap();
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
            .unwrap();
        let mut data = PackageSearchResult {
            name: "example".into(),
            version: "stale".into(),
            maintainer: "test".into(),
            last_modified: 1,
            source: Some(dir.path().to_str().unwrap().into()),
            subfolder: None,
            options: None,
            environment: None,
        };
        let (revision, fingerprint, snapshot) = capture(&mut data).unwrap();
        assert_eq!(revision, oid.to_string());
        assert_eq!(data.version, "2-3");
        assert!(snapshot.complete);
        assert_eq!(snapshot.files.len(), 2);
        data.options = Some("--nocheck".into());
        let (_, other, _) = capture(&mut data).unwrap();
        assert_ne!(fingerprint, other);
        data.subfolder = Some("../escape".into());
        assert!(capture(&mut data).is_err());
    }
}
