use bollard::container::{Config, LogOutput, LogsOptions, RemoveContainerOptions, WaitContainerOptions};
use bollard::container::{CreateContainerOptions, StartContainerOptions};
use bollard::image::CreateImageOptions;
use bollard::models::HostConfig;
use bollard::Docker;
use chrono::prelude::Utc;
use futures_util::{StreamExt, TryStreamExt};
use git2::Repository;
use rand::{Rng, RngCore};
use temp_dir::TempDir;
use tokio::task;

const IMAGE: &str = "git.pollinger.dev/public/aur-builder-build-container:latest";

fn get_image_options<'a>() -> Option<CreateImageOptions<'a, &'a str>> {
    Some(CreateImageOptions {
        from_image: IMAGE,
        ..Default::default()
    })
}

pub async fn pull_docker_image() -> Result<(), Box<dyn std::error::Error>> {
    let docker = Docker::connect_with_local_defaults().unwrap();
    let options = get_image_options();
    let pull_stream = docker.create_image(options, None, None);
    pull_stream
        .try_for_each(|_chunk| async {
            // We do nothing with progress updates here
            Ok(())
        })
        .await?;

    Ok(())
}

pub async fn build_package(name: String) -> Result<(), Box<dyn std::error::Error>> {
    let d = TempDir::new()?;
    info!("Building package {} in {}", name, &d.path().display());

    let random_suffix = rand::thread_rng().next_u32();

    let source_url = format!("https://aur.archlinux.org/{name}.git");
    let repo = Repository::clone(source_url.as_str(), d.child("source"))?;

    let docker = Docker::connect_with_local_defaults().unwrap();
    let container_name = format!("build-{}-{}", name, random_suffix);
    let create_container_options = CreateContainerOptions {
        name: container_name,
        ..Default::default()
    };

    let mount_strings = vec![
        format!("{}:/build/source:rw",d.child("source").to_str().unwrap()),
        format!("{}:/results:rw",d.child("target").to_str().unwrap()),
    ];
    let create_container_config = Config {
        image: Some(IMAGE),
        user: Some("builder"),
        host_config: Some(HostConfig {
            // e.g., to remove container automatically upon exit:
            auto_remove: Some(true),
            binds: Some(mount_strings),
            ..Default::default()
        }),
        ..Default::default()
    };

    let container = docker
        .create_container(Some(create_container_options), create_container_config)
        .await?;

    docker
        .start_container(&container.id, None::<StartContainerOptions<String>>)
        .await?;

    let docker_for_logs = docker.clone();
    let container_id_for_logs = container.id.clone();
    let logs_task = task::spawn(async move {
        debug!("Attaching to logs...");
        let mut logs_stream = docker_for_logs.logs(
            &container_id_for_logs,
            Some(LogsOptions::<String> {
                follow: true,
                stdout: true,
                stderr: true,
                // tail: "all",  // optional
                ..Default::default()
            }),
        );

        while let Some(log_result) = logs_stream.next().await {
            match log_result {
                Ok(LogOutput::StdOut { message }) => {
                    debug!("stdout: {}", String::from_utf8_lossy(&message));
                }
                Ok(LogOutput::StdErr { message }) => {
                    debug!("stderr: {}", String::from_utf8_lossy(&message));
                }
                Ok(LogOutput::Console { message }) => {
                    // For TTY-attached containers
                    debug!("console: {}", String::from_utf8_lossy(&message));
                }
                Err(e) => {
                    error!("Error reading log stream: {}", e);
                    break;
                }
                _ => {}
            }
        }
        debug!("Logs task finished");
    });

    let mut wait_stream =
        docker.wait_container(&container.id, None::<WaitContainerOptions<String>>);
    while let Some(res) = wait_stream.next().await {
        match res {
            Ok(exit) => {
                println!("Container exited with: {:?}", exit.status_code);
                break;
            }
            Err(e) => {
                eprintln!("Error while waiting: {:?}", e);
                break;
            }
        }
    }

    return Ok(());
}
