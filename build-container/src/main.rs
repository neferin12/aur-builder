use anyhow::{bail, Context, Result};
use aur_depends::{Flags, PkgbuildRepo, Resolver};
use srcinfo::{ArchVecs, Srcinfo};
use std::{
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

fn run(command: &mut Command) -> Result<()> {
    // Never print commands: arguments or environment can contain upload grants.
    let status = command
        .stdin(Stdio::null())
        .status()
        .context("starting child process")?;
    if !status.success() {
        bail!("child process failed with {status}");
    }
    Ok(())
}

fn metadata(dir: &Path) -> Result<Srcinfo> {
    let output = Command::new("makepkg")
        .arg("--printsrcinfo")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()?;
    if !output.status.success() {
        bail!("makepkg --printsrcinfo failed");
    }
    Srcinfo::from_buf(output.stdout.as_slice()).context("parsing package metadata")
}

fn options(value: &str) -> Vec<String> {
    // Match Bash's former unquoted expansion, without pathname expansion.
    value.split_ascii_whitespace().map(str::to_owned).collect()
}

fn include_checks(info: &mut Srcinfo, enabled: bool) {
    if enabled {
        info.base.makedepends = ArchVecs::from(
            info.base
                .makedepends
                .iter()
                .cloned()
                .chain(info.base.checkdepends.iter().cloned())
                .collect::<Vec<_>>(),
        );
    }
}

fn external_dependencies<'a>(info: &'a Srcinfo, arch: &'a str) -> Vec<&'a str> {
    info.pkgs
        .iter()
        .flat_map(|p| p.depends.arch(arch))
        .chain(info.base.makedepends.arch(arch))
        .filter(|dep| {
            let dep = alpm::Depend::new(*dep);
            !info.pkgs.iter().any(|pkg| {
                alpm_utils::depends::satisfies(
                    &dep,
                    &pkg.pkgname,
                    alpm::Version::new(info.version()),
                    pkg.provides.arch(arch).map(alpm::Depend::new),
                )
            })
        })
        .collect()
}

async fn dependencies(dir: &Path, opts: &[String]) -> Result<()> {
    let mut info = metadata(dir)?;
    include_checks(&mut info, !opts.iter().any(|o| o == "--nocheck"));
    let conf = alpm_utils::config::Config::new()?;
    let alpm = alpm_utils::alpm_with_conf(&conf)?;
    let mut cache = raur::Cache::new();
    let raur = raur::Handle::new();
    let ambiguous = Arc::new(Mutex::new(Vec::new()));
    let choices = ambiguous.clone();
    let actions = Resolver::new(
        &alpm,
        &mut cache,
        &raur,
        Flags::new() & !Flags::CHECK_DEPENDS,
    )
    .pkgbuild_repos(vec![PkgbuildRepo {
        name: "build-target",
        pkgs: vec![&info],
    }])
    .provider_callback(move |dep, providers| {
        if providers.len() > 1 {
            choices
                .lock()
                .unwrap()
                .push(format!("{dep}: {}", providers.join(", ")));
        }
        0
    })
    .resolve_targets(
        &info
            .pkgnames()
            .map(|n| format!("build-target/{n}"))
            .collect::<Vec<_>>(),
    )
    .await?;
    let choices = ambiguous.lock().unwrap();
    if !choices.is_empty() {
        bail!(
            "ambiguous AUR providers; choose an explicit dependency: {}",
            choices.join("; ")
        );
    }
    if !actions.missing.is_empty() {
        bail!("unresolved dependencies: {:?}", actions.missing);
    }
    let conflicts = actions.calculate_inner_conflicts(true);
    if !conflicts.is_empty() {
        bail!("conflicting dependencies: {conflicts:?}");
    }
    let repo: Vec<_> = actions
        .install
        .iter()
        .map(|p| format!("{}/{}", p.pkg.db().unwrap().name(), p.pkg.name()))
        .collect();
    let bases: Vec<_> = actions
        .build
        .iter()
        .filter(|b| matches!(b, aur_depends::Base::Aur(_)))
        .map(|b| b.package_base().to_owned())
        .collect();
    drop(actions);
    drop(alpm);
    if !repo.is_empty() {
        run(Command::new("sudo")
            .args(["pacman", "-S", "--needed", "--noconfirm", "--"])
            .args(repo))?;
    }
    let fetch = aur_fetch::Fetch::with_cache_dir("/build/.cache/aur-builder");
    if !bases.is_empty() {
        let fetched = fetch.download(&bases)?;
        fetch.merge(&fetched)?;
    }
    // Check actual PKGBUILD metadata, not just RPC metadata. Build only bases whose
    // dependencies are installed; a stalled queue reports cycles or stale metadata.
    let arch = conf
        .architecture
        .first()
        .context("pacman has no architecture")?;
    let mut pending = bases
        .iter()
        .map(|base| {
            let path = fetch.clone_dir.join(base);
            Ok((path.clone(), metadata(&path)?))
        })
        .collect::<Result<Vec<_>>>()?;
    while !pending.is_empty() {
        let mut ready = None;
        let mut blocked = Vec::new();
        for (i, (path, info)) in pending.iter().enumerate() {
            let deps = external_dependencies(info, arch);
            let output = Command::new("pacman")
                .args(["-T", "--"])
                .args(&deps)
                .stdin(Stdio::null())
                .output()?;
            if output.status.success() {
                ready = Some(i);
                break;
            }
            if output.status.code() != Some(127) {
                bail!("pacman dependency check failed");
            }
            blocked.push(format!(
                "{}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stdout).trim()
            ));
        }
        let i = ready.with_context(|| {
            format!(
                "dependency cycle or changed AUR metadata: {}",
                blocked.join("; ")
            )
        })?;
        let (path, _) = pending.remove(i);
        run(&mut build_command(&path, &[], true))?;
        let packages = package_list(&path, &[])?;
        run(Command::new("sudo")
            .args(["pacman", "-U", "--needed", "--noconfirm", "--"])
            .args(packages))?;
    }
    Ok(())
}

fn package_list(dir: &Path, opts: &[String]) -> Result<Vec<PathBuf>> {
    let output = Command::new("makepkg")
        .current_dir(dir)
        .arg("--packagelist")
        .args(opts)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()?;
    if !output.status.success() {
        bail!("could not list package outputs");
    }
    let paths: Vec<_> = String::from_utf8(output.stdout)?
        .lines()
        .map(PathBuf::from)
        .map(|p| if p.is_absolute() { p } else { dir.join(p) })
        .collect();
    // makepkg lists potential debug archives even when no debug files were
    // produced. Keep only archives that actually exist, as the old glob did.
    existing_packages(paths)
}

fn existing_packages(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let paths: Vec<_> = paths
        .into_iter()
        .filter(|p| p.is_file() && is_package(p))
        .collect();
    if paths.is_empty() {
        bail!("expected package output is missing or invalid");
    }
    Ok(paths)
}

fn is_package(path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    name.contains(".pkg.tar.") && !name.ends_with(".sig")
}

fn build_command(dir: &Path, opts: &[String], dependency: bool) -> Command {
    let mut command = Command::new("makepkg");
    command
        .current_dir(dir)
        .args(["-s", "-c", "-C", "--noconfirm", "--noprogressbar"]);
    if dependency {
        command.arg("--nocheck");
    } else {
        command.args(opts);
    }
    command
}

fn build(dir: &Path, opts: &[String], log: &Path) -> Result<()> {
    build_with(&mut build_command(dir, opts, false), log)
}

fn build_with(command: &mut Command, log: &Path) -> Result<()> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let file = Arc::new(Mutex::new(fs::File::create(log)?));
    fn pump(
        mut input: impl Read,
        mut out: impl Write,
        file: Arc<Mutex<fs::File>>,
    ) -> io::Result<()> {
        let mut buf = [0; 8192];
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 {
                return Ok(());
            }
            out.write_all(&buf[..n])?;
            out.flush()?;
            file.lock().unwrap().write_all(&buf[..n])?;
        }
    }
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let other = file.clone();
    let a = std::thread::spawn(move || pump(stdout, io::stdout(), file));
    let b = std::thread::spawn(move || pump(stderr, io::stderr(), other));
    let status = child.wait()?;
    a.join()
        .map_err(|_| anyhow::anyhow!("stdout reader panicked"))??;
    b.join()
        .map_err(|_| anyhow::anyhow!("stderr reader panicked"))??;
    if !status.success() {
        bail!("makepkg failed with {status}");
    }
    Ok(())
}

fn collect(dir: &Path, opts: &[String], log: &Path, results: &Path, bundle: &Path) -> Result<()> {
    fs::create_dir_all(results)?;
    let packages = package_list(dir, opts)?;
    let mut tar = tar::Builder::new(fs::File::create(bundle)?);
    for package in packages {
        let name = package.file_name().context("package has no filename")?;
        fs::copy(&package, results.join(name))?;
        tar.append_path_with_name(&package, Path::new(name))?;
    }
    tar.finish()?;
    fs::copy(log, results.join(log.file_name().unwrap()))?;
    Ok(())
}

fn upload(bundle: &Path, url: &str) -> Result<()> {
    // reqwest errors can include the URL; discard them at this boundary.
    let result = (|| {
        let file = fs::File::open(bundle)?;
        let len = file.metadata()?.len();
        let response = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?
            .put(url)
            .header(reqwest::header::CONTENT_LENGTH, len)
            .body(file)
            .send()?;
        if !matches!(response.status().as_u16(), 200 | 201 | 204) {
            bail!("upload rejected");
        }
        Ok::<_, anyhow::Error>(())
    })();
    result.map_err(|_| anyhow::anyhow!("bundle upload failed"))
}

fn checkout_approved_source(root: &Path, revision: &str) -> Result<()> {
    if revision.len() != 40 || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("missing or invalid approved revision");
    }
    run(Command::new("git")
        .current_dir(root)
        .args(["checkout", "--detach", revision]))
}

async fn execute() -> std::result::Result<(), (i32, anyhow::Error)> {
    let url = env::var("AB_UPLOAD_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| (107, anyhow::anyhow!("missing task upload URL")))?;
    let source = env::var("AB_SOURCE")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| (101, anyhow::anyhow!("AB_SOURCE is not set")))?;
    let home = PathBuf::from(env::var("HOME").unwrap_or_else(|_| "/build".into()));
    run(Command::new("git")
        .current_dir(&home)
        .args(["clone", "--", &source, "source"]))
    .map_err(|e| (102, e))?;
    let revision = env::var("AB_REVISION").map_err(|e| (102, e.into()))?;
    checkout_approved_source(&home.join("source"), &revision).map_err(|e| (102, e))?;
    let dir = home
        .join("source")
        .join(env::var("AB_SUBFOLDER").unwrap_or_default());
    let dir = dir.canonicalize().map_err(|e| (100, e.into()))?;
    if !dir.starts_with(
        home.join("source")
            .canonicalize()
            .map_err(|e| (100, e.into()))?,
    ) {
        return Err((
            100,
            anyhow::anyhow!("package subfolder escapes source repository"),
        ));
    }
    let opts = options(&env::var("AB_OPTIONS").unwrap_or_default());
    println!("INFO: updating packages...");
    run(Command::new("sudo").args(["pacman", "-Syu", "--noconfirm"])).map_err(|e| (103, e))?;
    println!("INFO: installing missing dependencies...");
    dependencies(&dir, &opts).await.map_err(|e| (104, e))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let log = home.join(format!("{timestamp}-build.log"));
    build(&dir, &opts, &log).map_err(|e| (105, e))?;
    let bundle = home.join("bundle.tar");
    collect(&dir, &opts, &log, Path::new("/results"), &bundle).map_err(|e| (106, e))?;
    // Blocking HTTP runs outside the Tokio runtime.
    std::thread::spawn(move || upload(&bundle, &url))
        .join()
        .map_err(|_| (107, anyhow::anyhow!("upload thread failed")))?
        .map_err(|e| (107, e))
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err((code, error)) = execute().await {
        let message = format!("{error:#}");
        let message = match env::var("AB_UPLOAD_URL") {
            Ok(url) if !url.is_empty() => message.replace(&url, "[redacted]"),
            _ => message,
        };
        eprintln!("ERROR: {message}");
        std::process::exit(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn fixture() -> Srcinfo {
        Srcinfo::from_buf(&b"pkgbase = sample\npkgver = 2\npkgrel = 1\narch = x86_64\narch = aarch64\nmakedepends = cmake>=3\ncheckdepends = test-runner\ncheckdepends_x86_64 = arch-test\npkgname = sample\ndepends = sibling>=2\ndepends = virtual=2\ndepends_x86_64 = native>=1\ndepends_aarch64 = foreign\npkgname = sibling\nprovides = virtual=2\n"[..]).unwrap()
    }

    #[test]
    fn metadata_preserves_versions_architecture_and_split_provides() {
        let info = fixture();
        let deps = external_dependencies(&info, "x86_64");
        assert_eq!(deps, ["native>=1", "cmake>=3"]);
        let deps = external_dependencies(&info, "aarch64");
        assert_eq!(deps, ["foreign", "cmake>=3"]);
    }

    #[test]
    fn target_checks_are_optional_and_dependency_checks_are_excluded() {
        let mut info = fixture();
        include_checks(&mut info, false);
        assert!(!external_dependencies(&info, "x86_64").contains(&"test-runner"));
        include_checks(&mut info, true);
        let deps = external_dependencies(&info, "x86_64");
        assert!(deps.contains(&"test-runner"));
        assert!(deps.contains(&"arch-test"));
        assert!(!external_dependencies(&fixture(), "x86_64").contains(&"arch-test"));
    }

    #[test]
    fn arguments_are_literal_and_ask_is_not_injected() {
        assert_eq!(
            options(" --nocheck\t--foo=$(touch /tmp/no) * "),
            ["--nocheck", "--foo=$(touch", "/tmp/no)", "*"]
        );
        assert!(options("").is_empty());
    }

    #[test]
    fn archives_exclude_signatures() {
        assert!(is_package(Path::new("sample-1-1-any.pkg.tar.zst")));
        assert!(!is_package(Path::new("sample-1-1-any.pkg.tar.zst.sig")));
        assert!(!is_package(Path::new("build.log")));
    }

    #[test]
    fn optional_debug_archive_can_be_absent() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("sample-1-1-any.pkg.tar.zst");
        let debug = temp.path().join("sample-debug-1-1-any.pkg.tar.zst");
        let signature = temp.path().join("sample-1-1-any.pkg.tar.zst.sig");
        fs::write(&package, "archive").unwrap();
        fs::write(&signature, "signature").unwrap();
        assert_eq!(
            existing_packages(vec![package.clone(), debug.clone(), signature]).unwrap(),
            vec![package]
        );
        assert!(existing_packages(vec![debug]).is_err());
    }

    #[test]
    fn command_failure_is_propagated() {
        assert!(run(Command::new("sh").args(["-c", "exit 3"])).is_err());
        assert!(run(&mut Command::new("nonexistent-aur-builder-test-command")).is_err());
    }

    #[test]
    fn build_flags_and_both_log_streams() {
        let command = build_command(Path::new("/tmp"), &["--nocheck".into()], false);
        let args: Vec<_> = command.get_args().map(|a| a.to_str().unwrap()).collect();
        assert!(args.contains(&"--nocheck"));
        assert!(!args.contains(&"--ask"));
        let command = build_command(Path::new("/tmp"), &["--skipinteg".into()], true);
        let args: Vec<_> = command.get_args().map(|a| a.to_str().unwrap()).collect();
        assert!(args.contains(&"--nocheck"));
        assert!(!args.contains(&"--skipinteg"));
        let log = tempfile::NamedTempFile::new().unwrap();
        let result = build_with(
            Command::new("sh").args(["-c", "echo output; echo error >&2; exit 3"]),
            log.path(),
        );
        assert!(result.is_err());
        let text = fs::read_to_string(log.path()).unwrap();
        assert!(text.contains("output"));
        assert!(text.contains("error"));
    }

    #[test]
    fn upload_statuses_and_secret_redaction() {
        for status in [200, 201, 204, 302, 403, 500] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/secret-grant", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let mut body = [0; 6];
                stream.read_exact(&mut body).unwrap();
                assert_eq!(&body, b"bundle");
                assert!(request.starts_with(b"PUT /secret-grant HTTP/1.1"));
                write!(
                    stream,
                    "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            });
            let temp = tempfile::NamedTempFile::new().unwrap();
            fs::write(temp.path(), b"bundle").unwrap();
            let result = upload(temp.path(), &url);
            assert_eq!(result.is_ok(), matches!(status, 200 | 201 | 204));
            if let Err(e) = result {
                assert!(!format!("{e:#}").contains("secret-grant"));
            }
            server.join().unwrap();
        }
    }
}

#[cfg(test)]
mod approval_tests {
    use super::*;
    #[test]
    fn checks_out_approved_commit_and_rejects_unpinned_input() {
        let dir = tempfile::tempdir().unwrap();
        run(Command::new("git")
            .current_dir(dir.path())
            .args(["init", "--quiet"]))
        .unwrap();
        std::fs::write(dir.path().join("PKGBUILD"), "approved").unwrap();
        run(Command::new("git")
            .current_dir(dir.path())
            .args(["add", "PKGBUILD"]))
        .unwrap();
        run(Command::new("git").current_dir(dir.path()).args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--no-gpg-sign",
            "--quiet",
            "-m",
            "approved",
        ]))
        .unwrap();
        let output = Command::new("git")
            .current_dir(dir.path())
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let revision = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        std::fs::write(dir.path().join("PKGBUILD"), "unapproved").unwrap();
        run(Command::new("git")
            .current_dir(dir.path())
            .args(["add", "PKGBUILD"]))
        .unwrap();
        run(Command::new("git").current_dir(dir.path()).args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--no-gpg-sign",
            "--quiet",
            "-m",
            "unapproved",
        ]))
        .unwrap();
        assert!(checkout_approved_source(dir.path(), "HEAD").is_err());
        checkout_approved_source(dir.path(), &revision).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("PKGBUILD")).unwrap(),
            "approved"
        );
        assert!(checkout_approved_source(dir.path(), &"0".repeat(40)).is_err());
    }
}
