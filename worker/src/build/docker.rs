use bollard::Docker;
use bollard::container::{
    Config, CreateContainerOptions, LogOutput, LogsOptions, RemoveContainerOptions,
    StartContainerOptions, WaitContainerOptions,
};
use bollard::errors::Error;
use bollard::image::CreateImageOptions;
use bollard::models::HostConfig;
use bytes::Bytes;
use common::config::{Configurable, WorkerConfig};
use common::get_rand_string;
use common::types::{BuildResultTransmissionFormat, BuildTaskTransmissionFormat, Timestamps};
use futures_util::{StreamExt, TryStreamExt};
use sea_orm::sqlx::types::chrono::Utc;
use std::env;

const IMAGE: &str = "ghcr.io/neferin12/aur-builder-build-container";
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn get_image_name() -> String {
    let config = WorkerConfig::new(env::var("AB_CONFIG_PATH").ok()).unwrap();
    format!(
        "{}:{}",
        config.builder.unwrap_or(IMAGE.to_string()),
        config.builder_tag.unwrap_or(VERSION.to_string())
    )
}

pub async fn pull_docker_image() -> Result<(), Box<dyn std::error::Error>> {
    info!("Pulling docker image...");
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

    info!("Image pulled successfully!");
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

pub async fn build(
    task: &BuildTaskTransmissionFormat,
    source_url: String,
    subfolder: &Option<String>,
) -> Result<BuildResultTransmissionFormat, Box<dyn std::error::Error>> {
    match pull_docker_image().await {
        Err(_) => warn!("Builder image could not be pulled"),
        _ => {}
    }
    info!("Building package {}", task.name);
    let build_start_time = Utc::now().naive_utc();

    let docker = Docker::connect_with_local_defaults()?;
    let container_name = format!("build-{}-{}", task.name, get_rand_string());
    let create_container_options = CreateContainerOptions {
        name: container_name,
        ..Default::default()
    };

    let mut env: Vec<String> = vec![
        format!("AB_SOURCE={source_url}"),
        format!(
            "AB_REVISION={}",
            task.revision.as_ref().ok_or("missing approved revision")?
        ),
        format!(
            "AB_SUBFOLDER={}",
            subfolder.clone().unwrap_or("".to_string())
        ),
        format!(
            "AB_UPLOAD_URL={}",
            task.upload.as_ref().ok_or("missing upload grant")?.url
        ),
        format!(
            "AB_OPTIONS={}",
            task.clone().options.unwrap_or("".to_string())
        ),
    ];

    if let Some(task_env) = task.env.clone() {
        for env_var in task_env {
            if !env_var.name.starts_with("AB_") {
                env.push(format!("{}={}", env_var.name, env_var.value));
            }
        }
    }

    let image = get_image_name();

    let create_container_config = Config {
        image: Some(image),
        user: Some("builder".to_string()),
        env: Some(env),
        host_config: Some(HostConfig {
            // Keep the container until its final logs have been collected.
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

    let result = async {
        docker
            .start_container(&container.id, None::<StartContainerOptions<String>>)
            .await?;

        attach_logs(docker.clone(), container.id.clone());

        let mut wait_stream =
            docker.wait_container(&container.id, None::<WaitContainerOptions<String>>);
        while let Some(res) = wait_stream.next().await {
            let mut logs = docker.logs(
                &container.id,
                Some(LogsOptions::<String> {
                    stdout: true,
                    stderr: true,
                    // tail: "all",  // optional
                    ..Default::default()
                }),
            );
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

            let build_end_time = Utc::now().naive_utc();

            let mut results = BuildResultTransmissionFormat {
                task: {
                    let mut t = task.clone();
                    t.upload = None;
                    t
                },
                status_code: -5,
                log_lines: logs_vec,
                success: true,
                timestamps: Timestamps {
                    start: build_start_time,
                    end: build_end_time,
                },
            };

            return match res {
                Ok(exit) => {
                    info!("Build container exited with: {:?}", exit.status_code);

                    results.status_code = exit.status_code;
                    results.success = exit.status_code == 0;

                    Ok(results)
                }
                Err(e) => {
                    match e {
                        Error::DockerContainerWaitError { code, .. } => {
                            results.status_code = code;
                            results.success = false;
                            Ok(results)
                        }
                        _ => Err(e.into()),
                    }
                    // error!("Error while waiting for build container: {:?}", e);
                    // return Err(results);
                }
            };
        }

        Err("Unexpected end of wait stream".into())
    }
    .await;

    // Also clean up after startup errors and broken wait streams. Force removal
    // stops a container that may still be running after a Docker API failure.
    let cleanup = docker
        .remove_container(
            &container.id,
            Some(RemoveContainerOptions {
                force: true,
                v: true,
                ..Default::default()
            }),
        )
        .await;
    match cleanup {
        Ok(())
        | Err(Error::DockerResponseServerError {
            status_code: 404, ..
        }) => result,
        Err(error) => {
            error!(
                "Failed to remove build container {}: {}",
                container.id, error
            );
            match result {
                Ok(_) => Err(error.into()),
                Err(build_error) => Err(build_error),
            }
        }
    }
}
