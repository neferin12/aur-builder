use axum::{
    Router,
    extract::{Path as RoutePath, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use base64::Engine;
use common::{
    storage::{Error, Storage},
    types::BuildResultTransmissionFormat,
};
use futures_util::StreamExt;
use lapin::{options::*, types::FieldTable};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, sync::Arc};
use tokio::sync::RwLock;
#[derive(Clone, Default, Serialize, Deserialize)]
struct Catalog {
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default)]
    bases: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    attempts: BTreeMap<String, String>,
    #[serde(default)]
    releases: BTreeMap<String, Vec<Release>>,
    #[serde(default)]
    pending_deletions: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Release {
    version: String,
    files: BTreeMap<String, String>,
}

fn prune(catalog: &mut Catalog, keep: usize) {
    let mut expired = BTreeMap::new();
    for releases in catalog.releases.values_mut() {
        if releases.len() > keep {
            for release in releases.drain(..releases.len() - keep) {
                expired.extend(release.files);
            }
        }
    }
    let protected: std::collections::HashSet<_> = catalog
        .releases
        .values()
        .flatten()
        .flat_map(|r| r.files.values())
        .cloned()
        .collect();
    let active: std::collections::HashSet<_> = catalog
        .bases
        .values()
        .flatten()
        .flat_map(|name| [name.clone(), format!("{name}.sig")])
        .collect();
    for (name, object) in expired {
        if !protected.contains(&object) && !active.contains(&name) {
            if catalog.files.get(&name) == Some(&object) {
                catalog.files.remove(&name);
            }
            if !catalog.files.values().any(|key| key == &object) {
                catalog.pending_deletions.push(object);
            }
        }
    }
}

async fn cleanup(app: &App, db: &database::Database, keep: usize) -> Result<(), Error> {
    let mut next = app.catalog.read().await.clone();
    // Recover only files whose immutable object prefix establishes ownership.
    let mut publication_order = BTreeMap::new();
    for (index, task) in db.retention_attempts().await?.into_iter().enumerate() {
        publication_order
            .entry((task.name.clone(), task.version.clone()))
            .or_insert(index);
        let prefix = format!("published/{}/", task.attempt);
        let files: BTreeMap<_, _> = next
            .files
            .iter()
            .filter(|(name, key)| key.starts_with(&prefix) && name.contains(".pkg.tar."))
            .map(|(name, key)| (name.clone(), key.clone()))
            .collect();
        if files.is_empty() {
            continue;
        }
        let releases = next.releases.entry(task.name).or_default();
        if let Some(release) = releases.iter_mut().find(|r| r.version == task.version) {
            release.files.extend(files);
        } else {
            releases.push(Release {
                version: task.version,
                files,
            });
        }
    }
    for (base, releases) in &mut next.releases {
        releases.sort_by_key(|r| {
            publication_order
                .get(&(base.clone(), r.version.clone()))
                .copied()
                .unwrap_or(usize::MAX)
        });
    }
    let tracked: std::collections::HashSet<_> = next
        .releases
        .values()
        .flatten()
        .flat_map(|r| r.files.keys())
        .collect();
    if next
        .files
        .keys()
        .any(|name| name.contains(".pkg.tar.") && !tracked.contains(name))
    {
        log::warn!("Preserving legacy package files with unknown release ownership");
    }
    prune(&mut next, keep);
    db.save_catalog(&serde_json::to_string(&next)?).await?;
    *app.catalog.write().await = next.clone();
    let client = storage_client();
    let mut remaining = Vec::new();
    for object in &next.pending_deletions {
        if next.files.values().any(|key| key == object) {
            continue;
        }
        let response = client
            .delete(app.storage.sign("DELETE", object, 900, false)?)
            .send()
            .await;
        match response {
            Ok(r) if r.status().is_success() || r.status() == StatusCode::NOT_FOUND => {}
            _ => {
                log::warn!("Repository object deletion failed; will retry");
                remaining.push(object.clone());
            }
        }
    }
    next.pending_deletions = remaining;
    db.save_catalog(&serde_json::to_string(&next)?).await?;
    *app.catalog.write().await = next;
    Ok(())
}
struct App {
    storage: Storage,
    catalog: RwLock<Catalog>,
    auth: String,
    name: String,
    ttl: u64,
}
#[derive(Debug)]
struct InvalidArtifact(String);
impl std::fmt::Display for InvalidArtifact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for InvalidArtifact {}
fn invalid(message: &str) -> Error {
    Box::new(InvalidArtifact(message.to_owned()))
}
fn storage_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(900))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("HTTP client configuration")
}
fn safe_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('.')
        && !s.starts_with('-')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"@_+.-".contains(&b))
}
async fn download(storage: &Storage, key: &str, path: &Path, limit: u64) -> Result<(), Error> {
    let response = storage_client()
        .get(storage.sign("GET", key, 900, false)?)
        .send()
        .await
        .map_err(|_| "storage download failed")?;
    if !response.status().is_success() {
        return Err("storage download failed".into());
    }
    let mut stream = response.bytes_stream();
    let mut size = 0u64;
    let mut file = tokio::fs::File::create(path).await?;
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "storage stream failed")?;
        size += chunk.len() as u64;
        if size > limit {
            return Err(invalid("artifact size limit exceeded"));
        }
        file.write_all(&chunk).await?;
    }
    Ok(())
}
async fn upload(storage: &Storage, key: &str, path: &Path) -> Result<(), Error> {
    let file = tokio::fs::File::open(path).await?;
    let length = file.metadata().await?.len();
    let r = storage_client()
        .put(storage.sign("PUT", key, 900, false)?)
        .header("content-length", length)
        .body(reqwest::Body::wrap_stream(
            tokio_util::io::ReaderStream::new(file),
        ))
        .send()
        .await
        .map_err(|_| "storage upload failed")?;
    if !r.status().is_success() {
        return Err("storage upload failed".into());
    }
    Ok(())
}
fn command(program: &str, args: &[&str], dir: &Path) -> Result<(), Error> {
    let output = std::process::Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()?;
    if !output.status.success() {
        return Err(format!("{program} failed").into());
    }
    Ok(())
}
fn sign(path: &Path, key: &str) -> Result<(), Error> {
    command(
        "gpg",
        &[
            "--batch",
            "--yes",
            "--pinentry-mode",
            "error",
            "--local-user",
            key,
            "--detach-sign",
            path.to_str().ok_or("invalid path")?,
        ],
        path.parent().unwrap(),
    )
}
fn file_hash(path: &Path) -> Result<Vec<u8>, Error> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hash.finalize().to_vec())
}
fn package_metadata(path: &Path) -> Result<String, Error> {
    use std::{
        io::Read,
        process::{Command, Stdio},
    };
    let mut child = Command::new("bsdtar")
        .args([
            "-xOf",
            path.to_str().ok_or("invalid package path")?,
            ".PKGINFO",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or("missing metadata stream")?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() > 1024 * 1024 {
        child.kill().ok();
        child.wait().ok();
        return Err(invalid("invalid package metadata"));
    }
    if !child.wait()?.success() {
        return Err(invalid("invalid package metadata"));
    }
    String::from_utf8(bytes).map_err(|_| invalid("invalid package metadata encoding"))
}
async fn ingest(
    app: &App,
    db: &database::Database,
    result: &BuildResultTransmissionFormat,
) -> Result<(), Error> {
    let task = &result.task;
    if !result.success
        || result.status_code != 0
        || !db.valid_attempt(task).await?
        || !db.latest_attempt(task).await?
    {
        return Ok(());
    }
    let old = app.catalog.read().await.clone();
    if old.attempts.get(&task.name) == Some(&task.attempt) {
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let dir = temp.path();
    let bundle = dir.join("bundle.tar");
    let limit = std::env::var("AB_REPO_MAX_BYTES")
        .unwrap_or("4294967296".into())
        .parse::<u64>()?;
    download(&app.storage, &task.object_key, &bundle, limit).await?;
    let mut archive = tar::Archive::new(std::fs::File::open(&bundle)?);
    let mut packages = Vec::new();
    let mut total = 0u64;
    for entry in archive
        .entries()
        .map_err(|_| invalid("invalid bundle archive"))?
    {
        let mut entry = entry.map_err(|_| invalid("invalid bundle archive entry"))?;
        let p = entry
            .path()
            .map_err(|_| invalid("invalid bundle path"))?
            .into_owned();
        let normalized = p.strip_prefix(".").unwrap_or(&p);
        let name = normalized
            .to_str()
            .ok_or_else(|| invalid("invalid filename"))?
            .to_string();
        if !safe_name(&name)
            || !name.contains(".pkg.tar.")
            || name.ends_with(".sig")
            || !entry.header().entry_type().is_file()
            || packages.contains(&name)
        {
            return Err(invalid("invalid bundle entry"));
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| invalid("size overflow"))?;
        if total > limit {
            return Err(invalid("extracted size limit"));
        }
        // Ignore archive permissions and timestamps; only package bytes are accepted.
        let mut file = std::fs::File::create(dir.join(&name))?;
        std::io::copy(&mut entry, &mut file).map_err(|error| {
            if matches!(
                error.kind(),
                std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::InvalidData
            ) {
                invalid("truncated bundle entry")
            } else {
                Box::new(error) as Error
            }
        })?;
        packages.push(name);
    }
    if packages.is_empty() {
        return Err(invalid("empty bundle"));
    }
    let key = std::env::var("AB_REPO_GPG_KEY")?;
    let mut identities = std::collections::HashSet::new();
    for name in &packages {
        let info = package_metadata(&dir.join(name))?;
        let field = |key: &str| {
            info.lines()
                .find_map(|l| l.strip_prefix(&format!("{key} = ")))
        };
        let arch = field("arch").ok_or_else(|| invalid("missing arch"))?;
        let pkgname = field("pkgname").ok_or_else(|| invalid("missing name"))?;
        if !matches!(arch, "any" | "x86_64")
            || !safe_name(pkgname)
            || field("pkgver").is_none()
            || field("pkgbase").unwrap_or(pkgname) != task.name
            || !identities.insert(pkgname.to_string())
        {
            return Err(invalid("unexpected package identity"));
        }
        if let Some(previous) = old.files.get(name) {
            let previous_path = dir.join("previous-package");
            download(&app.storage, previous, &previous_path, limit).await?;
            if file_hash(&previous_path)? != file_hash(&dir.join(name))? {
                return Err(invalid(
                    "package filename already published with different bytes; increase pkgrel",
                ));
            }
            download(
                &app.storage,
                old.files
                    .get(&format!("{name}.sig"))
                    .ok_or("missing retained signature")?,
                &dir.join(format!("{name}.sig")),
                1024 * 1024,
            )
            .await?;
        } else {
            sign(&dir.join(name), &key)?;
        }
    }
    let mut next = old.clone();
    // Retain download mappings for clients that still hold older database snapshots.
    next.bases.remove(&task.name);
    // Rebuild from the full active package set so removed split outputs disappear.
    for names in next.bases.values() {
        for name in names {
            if packages.contains(name) {
                return Err(invalid("filename conflicts with another package base"));
            }
            download(
                &app.storage,
                next.files.get(name).ok_or("missing catalog entry")?,
                &dir.join(name),
                limit,
            )
            .await?;
            let info = package_metadata(&dir.join(name))?;
            let identity = info
                .lines()
                .find_map(|l| l.strip_prefix("pkgname = "))
                .ok_or_else(|| invalid("missing package identity"))?;
            if !identities.insert(identity.to_string()) {
                return Err(invalid(
                    "package identity conflicts with another package base",
                ));
            }
            download(
                &app.storage,
                next.files
                    .get(&format!("{name}.sig"))
                    .ok_or("missing signature")?,
                &dir.join(format!("{name}.sig")),
                1024 * 1024,
            )
            .await?;
        }
    }
    let prefix = format!("published/{}", task.attempt);
    for name in &packages {
        if old.files.contains_key(name) {
            continue;
        }
        for n in [name.clone(), format!("{name}.sig")] {
            let object = format!("{prefix}/{n}");
            upload(&app.storage, &object, &dir.join(&n)).await?;
            next.files.insert(n, object);
        }
    }
    let release_files = packages
        .iter()
        .flat_map(|name| [name.clone(), format!("{name}.sig")])
        .map(|name| (name.clone(), next.files[&name].clone()))
        .collect();
    let releases = next.releases.entry(task.name.clone()).or_default();
    // Merge repeated versions without consuming a retention slot.
    if let Some(previous) = releases.iter_mut().find(|r| r.version == task.version) {
        previous.files.extend(release_files);
    } else {
        releases.push(Release {
            version: task.version.clone(),
            files: release_files,
        });
    }
    next.bases.insert(task.name.clone(), packages);
    let dbname = format!("{}.db.tar.gz", app.name);
    let mut args = vec![dbname.clone()];
    for names in next.bases.values() {
        args.extend(names.iter().cloned());
    }
    command(
        "repo-add",
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
        dir,
    )?;
    for extension in ["db", "files"] {
        let name = format!("{}.{extension}.tar.gz", app.name);
        sign(&dir.join(&name), &key)?;
        for suffix in ["", ".sig"] {
            let filename = format!("{name}{suffix}");
            let object = format!("{prefix}/{filename}");
            upload(&app.storage, &object, &dir.join(&filename)).await?;
            next.files.insert(filename, object.clone());
            next.files
                .insert(format!("{}.{extension}{suffix}", app.name), object);
        }
    }
    next.attempts
        .insert(task.name.clone(), task.attempt.clone());
    if !db
        .publish_catalog(task, &serde_json::to_string(&next)?)
        .await?
    {
        return Ok(());
    }
    *app.catalog.write().await = next;
    db.attempt_state(&task.attempt, "published").await?;
    Ok(())
}
async fn serve(
    State(app): State<Arc<App>>,
    RoutePath((repo, arch, file)): RoutePath<(String, String, String)>,
    headers: HeaderMap,
    method: Method,
) -> Response {
    let provided = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    // Compare every byte, including for invalid lengths.
    let diff = provided.len() ^ app.auth.len();
    let mismatch = provided
        .bytes()
        .zip(app.auth.bytes())
        .fold(diff, |d, (a, b)| d | usize::from(a ^ b));
    if mismatch != 0 {
        return (
            StatusCode::UNAUTHORIZED,
            [
                ("www-authenticate", "Basic realm=\"aur-builder\""),
                ("cache-control", "no-store"),
            ],
        )
            .into_response();
    }
    if repo != app.name || arch != "x86_64" || !safe_name(&file) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let catalog = app.catalog.read().await;
    let Some(key) = catalog.files.get(&file) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match app.storage.sign(
        if method == Method::HEAD {
            "HEAD"
        } else {
            "GET"
        },
        key,
        app.ttl,
        true,
    ) {
        Ok(url) => ([("cache-control", "no-store")], Redirect::temporary(&url)).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
#[tokio::main]
async fn main() -> Result<(), Error> {
    common::environment::load_dotenv().ok();
    simple_logger::init_with_env().unwrap();
    let user = std::env::var("AB_REPO_USER")?;
    let password = std::env::var("AB_REPO_PASSWORD")?;
    if user.is_empty() || password.is_empty() || user.contains(':') {
        return Err("invalid repository credentials".into());
    }
    let name = std::env::var("AB_REPO_NAME").unwrap_or("aur-builder".into());
    if !safe_name(&name) {
        return Err("invalid repository name".into());
    }
    let key = std::env::var("AB_REPO_GPG_KEY")?;
    command(
        "gpg",
        &["--batch", "--list-secret-keys", &key],
        Path::new("/tmp"),
    )?;
    let db = database::connect_to_db().await;
    db.migrate().await;
    let app = Arc::new(App {
        storage: Storage::from_env()?,
        catalog: RwLock::new(serde_json::from_str(&db.load_catalog().await?)?),
        auth: format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
        ),
        name,
        ttl: std::env::var("AB_DOWNLOAD_TTL")
            .unwrap_or("900".into())
            .parse()?,
    });
    if !(1..=604800).contains(&app.ttl) {
        return Err("invalid download URL lifetime".into());
    }
    let conn = common::connect_to_rabbitmq().await;
    let channel = conn.create_channel().await?;
    common::messaging::declare_repo_queue(&channel).await?;
    channel.basic_qos(1, BasicQosOptions::default()).await?;
    let mut consumer = channel
        .basic_consume(
            "repo_ingest",
            "repo",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;
    let state = app.clone();
    common::messaging::supervise(async move {
        let _conn = conn;
        while let Some(Ok(delivery)) = consumer.next().await {
            if let Ok(request) =
                serde_json::from_slice::<common::messaging::RetentionRequest>(&delivery.data)
            {
                match cleanup(&state, &db, request.keep_versions.get()).await {
                    Ok(()) => {
                        delivery.ack(BasicAckOptions::default()).await.ok();
                    }
                    Err(e) => {
                        log::error!("Repository cleanup failed: {e}");
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                        delivery
                            .nack(BasicNackOptions {
                                requeue: true,
                                ..Default::default()
                            })
                            .await
                            .ok();
                    }
                }
                continue;
            }
            let result = serde_json::from_slice::<BuildResultTransmissionFormat>(&delivery.data);
            match result {
                Ok(result) => match ingest(&state, &db, &result).await {
                    Ok(()) => {
                        delivery.ack(BasicAckOptions::default()).await.ok();
                    }
                    Err(e) => {
                        log::error!("Repository ingestion failed: {e}");
                        if e.is::<InvalidArtifact>() {
                            delivery
                                .reject(BasicRejectOptions { requeue: false })
                                .await
                                .ok();
                            continue;
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                        delivery
                            .nack(BasicNackOptions {
                                requeue: true,
                                ..Default::default()
                            })
                            .await
                            .ok();
                    }
                },
                Err(_) => {
                    delivery
                        .reject(BasicRejectOptions { requeue: false })
                        .await
                        .ok();
                }
            }
        }
    });
    let router = Router::new()
        .route("/{repo}/os/{arch}/{file}", get(serve))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await?;
    axum::serve(listener, router).await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retention_keeps_latest_releases_and_active_files() {
        let mut catalog = Catalog::default();
        for version in ["1", "2", "3"] {
            let name = format!("pkg-{version}.pkg.tar.gz");
            let files = BTreeMap::from([
                (name.clone(), name.clone()),
                (format!("{name}.sig"), format!("{name}.sig")),
            ]);
            catalog.files.extend(files.clone());
            catalog
                .releases
                .entry("pkg".into())
                .or_default()
                .push(Release {
                    version: version.into(),
                    files,
                });
        }
        catalog
            .bases
            .insert("pkg".into(), vec!["pkg-3.pkg.tar.gz".into()]);
        prune(&mut catalog, 2);
        assert_eq!(catalog.releases["pkg"].len(), 2);
        assert!(!catalog.files.contains_key("pkg-1.pkg.tar.gz"));
        assert_eq!(catalog.pending_deletions.len(), 2);
        prune(&mut catalog, 1);
        assert!(catalog.files.contains_key("pkg-3.pkg.tar.gz"));
        assert_eq!(catalog.files.len(), 2);
        prune(&mut catalog, 1);
        assert_eq!(catalog.pending_deletions.len(), 4);
    }
    #[test]
    fn reject_paths() {
        for n in ["../a", "/a", "a/b", ".hidden", "-option", ""] {
            assert!(!safe_name(n));
        }
        assert!(safe_name("foo-1.0-1-x86_64.pkg.tar.zst"));
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    /// Run with a disposable Garage bucket and an isolated GNUPGHOME; see STORAGE.md.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires Garage and a test GPG signing key"]
    async fn private_signed_publication() -> Result<(), Error> {
        let storage = Storage::from_env()?;
        let client = reqwest::Client::new();
        let key = "integration/probe +.txt";
        let put = storage.sign("PUT", key, 60, true)?;
        assert!(
            client
                .put(put)
                .body("private artifact")
                .send()
                .await?
                .status()
                .is_success()
        );
        let signed_get = storage.sign("GET", key, 60, true)?;
        assert_eq!(
            client.get(&signed_get).send().await?.text().await?,
            "private artifact"
        );
        let partial = client
            .get(&signed_get)
            .header("range", "bytes=0-6")
            .send()
            .await?;
        assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(partial.text().await?, "private");
        assert!(
            !client
                .get(signed_get.split('?').next().unwrap())
                .send()
                .await?
                .status()
                .is_success()
        );
        assert!(
            client
                .head(storage.sign("HEAD", key, 60, true)?)
                .send()
                .await?
                .status()
                .is_success()
        );
        let expired = storage.sign("GET", key, 1, true)?;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        assert!(!client.get(expired).send().await?.status().is_success());
        let db = database::Database::new("sqlite::memory:".into()).await?;
        db.migrate().await;
        let app = Arc::new(App {
            storage,
            catalog: RwLock::new(Catalog::default()),
            auth: "Basic dXNlcjpwYXNz".into(),
            name: "aur-builder".into(),
            ttl: 60,
        });
        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join(".PKGINFO"),
            "pkgname = storage-test\npkgbase = storage-test\npkgver = 1.0-1\npkgdesc = Integration test\nurl = https://example.com\nbuilddate = 1\npackager = Test\nsize = 0\narch = any\nlicense = MIT\n",
        )?;
        command(
            "bsdtar",
            &["-czf", "storage-test-1.0-1-any.pkg.tar.gz", ".PKGINFO"],
            dir.path(),
        )?;
        let info = std::fs::read_to_string(dir.path().join(".PKGINFO"))?;
        std::fs::write(
            dir.path().join(".PKGINFO"),
            info.replace("pkgname = storage-test\n", "pkgname = storage-test-tools\n"),
        )?;
        command(
            "bsdtar",
            &[
                "-czf",
                "storage-test-tools-1.0-1-any.pkg.tar.gz",
                ".PKGINFO",
            ],
            dir.path(),
        )?;
        command(
            "bsdtar",
            &[
                "-cf",
                "bundle.tar",
                "storage-test-1.0-1-any.pkg.tar.gz",
                "storage-test-tools-1.0-1-any.pkg.tar.gz",
            ],
            dir.path(),
        )?;
        let data = common::types::PackageSearchResult {
            name: "storage-test".into(),
            version: "1.0-1".into(),
            source: Some("https://example.com/source.git".into()),
            subfolder: None,
            options: None,
            environment: None,
            maintainer: "test".into(),
            last_modified: 1,
        };
        db.observe_release(
            &data,
            &"a".repeat(40),
            "storage-test",
            &database::reviews::Snapshot {
                files: vec![],
                complete: true,
            },
        )
        .await?;
        let candidate = db.candidates().await?.remove(0);
        db.decide_release(&candidate.id, "reviewer", true, "", |t| {
            app.storage.grant(t)
        })
        .await?;
        let mut task = db.pending_attempts().await?.remove(0);
        app.storage.renew(&mut task)?;
        let url = task.upload.take().unwrap().url;
        assert!(
            client
                .put(url)
                .body(std::fs::read(dir.path().join("bundle.tar"))?)
                .send()
                .await?
                .status()
                .is_success()
        );
        let timestamp = common::types::Timestamps {
            start: chrono::Utc::now().naive_utc(),
            end: chrono::Utc::now().naive_utc(),
        };
        let result = BuildResultTransmissionFormat {
            task,
            status_code: 0,
            log_lines: vec![],
            success: true,
            timestamps: timestamp,
        };
        ingest(&app, &db, &result).await?;
        let catalog = app.catalog.read().await.clone();
        assert!(catalog.files.contains_key("aur-builder.db"));
        assert!(
            catalog
                .files
                .contains_key("storage-test-tools-1.0-1-any.pkg.tar.gz")
        );
        ingest(&app, &db, &result).await?;
        assert_eq!(db.load_catalog().await?, serde_json::to_string(&catalog)?);
        for file in ["aur-builder.db", "storage-test-1.0-1-any.pkg.tar.gz"] {
            download(
                &app.storage,
                &catalog.files[file],
                &dir.path().join(file),
                1024 * 1024,
            )
            .await?;
            download(
                &app.storage,
                &catalog.files[&format!("{file}.sig")],
                &dir.path().join(format!("{file}.sig")),
                1024 * 1024,
            )
            .await?;
            command(
                "gpg",
                &["--batch", "--verify", &format!("{file}.sig"), file],
                dir.path(),
            )?;
        }
        let response = serve(
            State(app.clone()),
            RoutePath((
                "aur-builder".into(),
                "x86_64".into(),
                "aur-builder.db".into(),
            )),
            HeaderMap::new(),
            Method::GET,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let mut headers = HeaderMap::new();
        headers.insert("authorization", app.auth.parse()?);
        let response = serve(
            State(app.clone()),
            RoutePath((
                "aur-builder".into(),
                "x86_64".into(),
                "aur-builder.db".into(),
            )),
            headers,
            Method::GET,
        )
        .await;
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        assert!(
            client
                .get(response.headers()["location"].to_str()?)
                .send()
                .await?
                .status()
                .is_success()
        );
        // A failed or superseded attempt must preserve the last published catalog.
        let mut broken = result.clone();
        app.storage.grant(&mut broken.task)?;
        broken.task.upload = None;
        db.record_attempt(&broken.task).await?;
        assert!(ingest(&app, &db, &broken).await.is_err());
        assert_eq!(db.load_catalog().await?, serde_json::to_string(&catalog)?);
        ingest(&app, &db, &result).await?;
        assert_eq!(db.load_catalog().await?, serde_json::to_string(&catalog)?);
        // Verify pacman's actual HTTP, Basic auth, redirect and signature handling.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = Router::new()
            .route("/{repo}/os/{arch}/{file}", get(serve))
            .with_state(app.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let config = format!(
            "[options]\nArchitecture = x86_64\nSigLevel = Required DatabaseRequired\n[aur-builder]\nServer = http://user:pass@{address}/aur-builder/os/x86_64\n"
        );
        std::fs::write(dir.path().join("pacman.conf"), config)?;
        std::fs::create_dir(dir.path().join("db"))?;
        std::fs::create_dir(dir.path().join("cache"))?;
        std::fs::create_dir(dir.path().join("root"))?;
        let home = std::env::var("GNUPGHOME")?;
        let run_dir = dir.path().to_owned();
        let output = tokio::task::spawn_blocking(move || {
            std::process::Command::new("fakeroot")
                .args([
                    "pacman",
                    "-Sy",
                    "--root",
                    run_dir.join("root").to_str().unwrap(),
                    "--noconfirm",
                    "--nodeps",
                    "--config",
                    run_dir.join("pacman.conf").to_str().unwrap(),
                    "--dbpath",
                    run_dir.join("db").to_str().unwrap(),
                    "--cachedir",
                    run_dir.join("cache").to_str().unwrap(),
                    "--logfile",
                    run_dir.join("pacman.log").to_str().unwrap(),
                    "--gpgdir",
                    &home,
                    "storage-test",
                ])
                .output()
        })
        .await??;
        server.abort();
        assert!(
            output.status.success(),
            "pacman failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            dir.path()
                .join("cache/storage-test-1.0-1-any.pkg.tar.gz")
                .exists()
        );
        assert!(dir.path().join("db/local/storage-test-1.0-1/desc").exists());
        // Publish a new version with one split output removed.
        let info = std::fs::read_to_string(dir.path().join(".PKGINFO"))?;
        std::fs::write(
            dir.path().join(".PKGINFO"),
            info.replace("pkgname = storage-test-tools", "pkgname = storage-test")
                .replace("pkgver = 1.0-1", "pkgver = 2.0-1"),
        )?;
        command(
            "bsdtar",
            &["-czf", "storage-test-2.0-1-any.pkg.tar.gz", ".PKGINFO"],
            dir.path(),
        )?;
        command(
            "bsdtar",
            &["-cf", "bundle.tar", "storage-test-2.0-1-any.pkg.tar.gz"],
            dir.path(),
        )?;
        let mut upgraded = result.clone();
        upgraded.task.version = "2.0-1".into();
        app.storage.grant(&mut upgraded.task)?;
        db.record_attempt(&upgraded.task).await?;
        let grant = upgraded.task.upload.take().unwrap();
        assert!(
            client
                .put(grant.url)
                .body(std::fs::read(dir.path().join("bundle.tar"))?)
                .send()
                .await?
                .status()
                .is_success()
        );
        ingest(&app, &db, &upgraded).await?;
        let upgraded_catalog = app.catalog.read().await.clone();
        assert_eq!(
            upgraded_catalog.bases["storage-test"],
            vec!["storage-test-2.0-1-any.pkg.tar.gz"]
        );
        assert_eq!(
            upgraded_catalog.files["storage-test-1.0-1-any.pkg.tar.gz"],
            catalog.files["storage-test-1.0-1-any.pkg.tar.gz"]
        );
        assert!(
            client
                .get(app.storage.sign(
                    "GET",
                    &upgraded_catalog.files["storage-test-1.0-1-any.pkg.tar.gz"],
                    60,
                    true
                )?)
                .send()
                .await?
                .status()
                .is_success()
        );
        let recovered: Catalog = serde_json::from_str(&db.load_catalog().await?)?;
        assert_eq!(recovered.files, upgraded_catalog.files);
        download(
            &app.storage,
            &upgraded_catalog.files["aur-builder.db"],
            &dir.path().join("new.db"),
            1024 * 1024,
        )
        .await?;
        let contents = std::process::Command::new("bsdtar")
            .args(["-tf", dir.path().join("new.db").to_str().unwrap()])
            .output()?;
        let contents = String::from_utf8(contents.stdout)?;
        assert!(contents.contains("storage-test-2.0-1/"));
        assert!(!contents.contains("storage-test-tools"));
        // A different package base cannot take over an active package identity.
        let info = std::fs::read_to_string(dir.path().join(".PKGINFO"))?;
        std::fs::write(
            dir.path().join(".PKGINFO"),
            info.replace("pkgbase = storage-test", "pkgbase = other-base")
                .replace("pkgver = 2.0-1", "pkgver = 3.0-1"),
        )?;
        command(
            "bsdtar",
            &["-czf", "storage-test-3.0-1-any.pkg.tar.gz", ".PKGINFO"],
            dir.path(),
        )?;
        command(
            "bsdtar",
            &["-cf", "bundle.tar", "storage-test-3.0-1-any.pkg.tar.gz"],
            dir.path(),
        )?;
        let mut conflict = result.clone();
        conflict.task.id = 2;
        conflict.task.name = "other-base".into();
        app.storage.grant(&mut conflict.task)?;
        db.record_attempt(&conflict.task).await?;
        let grant = conflict.task.upload.take().unwrap();
        assert!(
            client
                .put(grant.url)
                .body(std::fs::read(dir.path().join("bundle.tar"))?)
                .send()
                .await?
                .status()
                .is_success()
        );
        assert!(
            ingest(&app, &db, &conflict)
                .await
                .unwrap_err()
                .is::<InvalidArtifact>()
        );
        assert_eq!(
            db.load_catalog().await?,
            serde_json::to_string(&upgraded_catalog)?
        );
        // Metadata output is capped while decompressing, before it can fill memory.
        std::fs::write(dir.path().join(".PKGINFO"), vec![b'x'; 1024 * 1024 + 1])?;
        command(
            "bsdtar",
            &["-czf", "oversized.pkg.tar.gz", ".PKGINFO"],
            dir.path(),
        )?;
        assert!(
            package_metadata(&dir.path().join("oversized.pkg.tar.gz"))
                .unwrap_err()
                .is::<InvalidArtifact>()
        );

        Ok(())
    }
}
