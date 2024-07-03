use std::fs;
use std::process::Command;
use git2::Repository;
use tempdir::TempDir;
use regex::Regex;

pub fn build(name: &String, tmp_dir: &TempDir) -> Result<Vec<String>, String> {
    
    dbg!(tmp_dir);
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
    if !status.success() {
        return Err("Failed to run makepkg".to_owned());
    }
    let paths = fs::read_dir(tmp_dir.path()).unwrap();

    let mut package_file_names: Vec<String> = Vec::new();

    let re = Regex::new(r"(?<pkg>.+-.+-.+\.pkg\..+)").unwrap();

    for path in paths {
        let file_name = path.unwrap().file_name();
        let Some(caps) = re.captures(file_name.to_str().unwrap()) else {
            continue;
        };
        package_file_names.push(caps["pkg"].to_owned());
    }
    dbg!(&package_file_names);
    Ok(package_file_names)
}