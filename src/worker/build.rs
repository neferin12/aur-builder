use std::process::Command;
use git2::Repository;
use tempdir::TempDir;

pub async fn build(name: &String) -> Result<&String, String> {
    let tmp_dir = TempDir::new(name.as_str()).expect("Failed to create temp dir");
    dbg!(&tmp_dir);
    let mut git_url: String = "https://aur.archlinux.org/".to_owned();
    git_url.push_str(&name);
    git_url.push_str(".git");
    print!("Cloning repo {}", git_url);
    let repo = Repository::clone(&git_url, tmp_dir.path());
    if repo.is_err() {
        return Err(repo.err().unwrap().message().to_owned());
    }
    print!("Cloned repo...");

    let mut makepkg = Command::new("makepkg")
        .current_dir(tmp_dir.path())
        .arg("-s")
        .spawn()
        .expect("Failed to run makepkg");
    let status = makepkg.wait().unwrap();
    dbg!(status);
    if !status.success(){
        return Err("Failed to run makepkg".to_owned());
    }
    Ok(name)
}