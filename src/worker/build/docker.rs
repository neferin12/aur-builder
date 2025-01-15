use bollard::Docker;
use bollard::image::CreateImageOptions;
use futures_util::{StreamExt, TryStreamExt};
use bollard::container::{Config, CreateContainerOptions, LogOutput, LogsOptions, StartContainerOptions, WaitContainerOptions};
use bollard::models::HostConfig;
use rand::RngCore;
use aur_builder_commons::environment::get_environment_variable;

// const IMAGE: &str = "git.pollinger.dev/public/aur-builder-build-container:latest";
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn get_image_name() -> String {
    let image = format!("git.pollinger.dev/public/aur-builder-build-container:{}", VERSION);

    return image;
}


pub async fn pull_docker_image() -> Result<(), Box<dyn std::error::Error>> {
    let docker = Docker::connect_with_local_defaults()?;
    let image = get_image_name();
    let options = Some(CreateImageOptions {
        from_image: image.as_str(),
        ..Default::default()
    });
    let pull_stream = docker.create_image(options, None, None);
    pull_stream
        .try_for_each(|_chunk| async {
            // We do nothing with progress updates here
            Ok(())
        })
        .await?;

    Ok(())
}

fn attach_logs(docker_for_logs: Docker, container_id_for_logs: String) {
    tokio::spawn(async move {
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
    });
}



pub async fn build(name: &String, source_url: String) -> Result<(), Box<dyn std::error::Error>> {
    info!("Building package {}", name);

    let random_suffix = rand::thread_rng().next_u32();

    let docker = Docker::connect_with_local_defaults()?;
    let container_name = format!("build-{}-{}", name, random_suffix);
    let create_container_options = CreateContainerOptions {
        name: container_name,
        ..Default::default()
    };


    let gitea_url = get_environment_variable("AB_GITEA_REPO");
    let gitea_user = get_environment_variable("AB_GITEA_USER");
    let gitea_token = get_environment_variable("AB_GITEA_TOKEN");


    let env_source = &*format!("AUR_BUILDER_SOURCE={source_url}");
    let env_gitea_url = &*format!("AB_GITEA_REPO={}", gitea_url);
    let env_gitea_user = &*format!("AB_GITEA_USER={}", gitea_user);
    let env_gitea_token = &*format!("AB_GITEA_TOKEN={}", gitea_token);

    let image = get_image_name();
    
    let create_container_config = Config {
        image: Some(image.as_str()),
        user: Some("builder"),
        env: Some(vec![
            env_source,
            env_gitea_url,
            env_gitea_user,
            env_gitea_token,
        ]),
        host_config: Some(HostConfig {
            // e.g., to remove container automatically upon exit:
            auto_remove: Some(true),
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

    attach_logs(docker.clone(), container.id.clone());

    let mut wait_stream =
        docker.wait_container(&container.id, None::<WaitContainerOptions<String>>);
    while let Some(res) = wait_stream.next().await {
        match res {
            Ok(exit) => {
                info!("Build container exited with: {:?}", exit.status_code);
                break;
            }
            Err(e) => {
                error!("Error while waiting for build container: {:?}", e);
                return Err(e.into());
            }
        }
    }

    Ok(())
}

