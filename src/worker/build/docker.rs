use aur_builder_commons::environment::get_environment_variable;
use bollard::container::{
    Config, CreateContainerOptions, LogOutput, LogsOptions, StartContainerOptions,
    WaitContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::models::HostConfig;
use bollard::Docker;
use bollard::errors::Error;
use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use sea_orm::sqlx::types::chrono::Utc;
use aur_builder_commons::get_rand_string;
use aur_builder_commons::types::{BuildResultTransmissionFormat, BuildTaskTransmissionFormat, Timestamps};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn get_image_name() -> String {
    format!("ghcr.io/neferin12/aur-builder-build-container:{}", VERSION)
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

fn log_message_to_string(prefix: &str, message: Bytes) -> String {
    format!("{prefix}: {}", String::from_utf8_lossy(&message))
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
                    debug!("{}", log_message_to_string("stdout", message));
                }
                Ok(LogOutput::StdErr { message }) => {
                    debug!("{}", log_message_to_string("stderr", message));
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

pub async fn build(task: &BuildTaskTransmissionFormat, source_url: String) -> Result<BuildResultTransmissionFormat, Box<dyn std::error::Error>> {
    info!("Building package {}", task.name);
    let build_start_time = Utc::now().naive_utc();

    let docker = Docker::connect_with_local_defaults()?;
    let container_name = format!("build-{}-{}", task.name, get_rand_string());
    let create_container_options = CreateContainerOptions {
        name: container_name,
        ..Default::default()
    };

    let gitea_url = get_environment_variable("AB_GITEA_REPO");
    let gitea_user = get_environment_variable("AB_GITEA_USER");
    let gitea_token = get_environment_variable("AB_GITEA_TOKEN");

    let env_source = &*format!("AB_SOURCE={source_url}");
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
            auto_remove: Some(false),
            cpu_period: Some(100000),
            cpu_quota: Some(100000),
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
        let mut logs = docker.logs(&container.id, Some(LogsOptions::<String> {
            stdout: true,
            stderr: true,
            // tail: "all",  // optional
            ..Default::default()
        }));
        let mut logs_vec = Vec::new();

        while let Some(log_result) = logs.next().await {
            match log_result {
                Ok(LogOutput::StdOut { message }) => {
                    logs_vec.push(log_message_to_string("stdout", message));
                }
                Ok(LogOutput::StdErr { message }) => {
                    logs_vec.push(log_message_to_string("stderr", message));
                }
                Err(e) => {
                    error!("Error reading log stream: {}", e);
                    break;
                }
                _ => {}
            }
        }

        docker.remove_container(&container.id, Default::default()).await?;
        
        let build_end_time = Utc::now().naive_utc();

        let mut results = BuildResultTransmissionFormat {
            task: task.to_owned(),
            status_code: -5,
            log_lines: logs_vec,
            success: true,
            timestamps: Timestamps {
                start: build_start_time,
                end: build_end_time,
            }
        };


        return match res {
            Ok(exit) => {
                info!("Build container exited with: {:?}", exit.status_code);

                results.status_code = exit.status_code;
                
                Ok(results)
            }
            Err(e) => {
                match e {
                    Error::DockerContainerWaitError { code, .. } => {
                        results.status_code = code;
                        results.success = false;
                        Ok(results)
                    },
                    _ => Err(e.into())
                }
                // error!("Error while waiting for build container: {:?}", e);
                // return Err(results);

            }
        }
    }

    Err("Unexpected end of wait stream".into())
}
