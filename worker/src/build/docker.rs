use bollard::Docker;
use bollard::container::{
    Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions, StartContainerOptions,
};
use bollard::errors::Error;
use bollard::image::CreateImageOptions;
use bollard::models::{HostConfig, HostConfigLogConfig};
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

fn failure_result(
    task: &BuildTaskTransmissionFormat,
    start: sea_orm::sqlx::types::chrono::NaiveDateTime,
    code: i64,
    message: &str,
) -> BuildResultTransmissionFormat {
    let mut task = task.clone();
    task.upload = None;
    BuildResultTransmissionFormat {
        task,
        artifact: None,
        status_code: code,
        log_lines: vec![message.into()],
        success: false,
        timestamps: Timestamps {
            start,
            end: Utc::now().naive_utc(),
        },
    }
}

pub async fn build(
    task: &BuildTaskTransmissionFormat,
    source_url: String,
    subfolder: &Option<String>,
) -> Result<BuildResultTransmissionFormat, Box<dyn std::error::Error>> {
    match pull_docker_image().await {
        Err(e) => warn!("Builder image could not be pulled: {e:?}"),
        _ => {}
    }
    info!("Building package {:?}", task.name);
    let build_start_time = Utc::now().naive_utc();

    let docker = Docker::connect_with_local_defaults()?;
    let features = docker.info().await?;
    if features.memory_limit != Some(true)
        || features.swap_limit != Some(true)
        || features.pids_limit != Some(true)
        || features.cpu_cfs_quota != Some(true)
        || !features.security_options.as_ref().is_some_and(|options| {
            options
                .iter()
                .any(|option| option.starts_with("name=seccomp"))
        })
    {
        return Err("Docker must enforce memory, swap, PID, CPU and seccomp isolation".into());
    }
    let network = env::var("AB_BUILD_NETWORK").unwrap_or("aur-builder-build".into());
    let isolation = docker
        .inspect_network(
            &network,
            None::<bollard::network::InspectNetworkOptions<String>>,
        )
        .await?;
    if isolation.internal != Some(true)
        || isolation
            .options
            .as_ref()
            .and_then(|o| o.get("com.docker.network.bridge.gateway_mode_ipv4"))
            .map(String::as_str)
            != Some("isolated")
    {
        return Err("build network must be internal with isolated gateway mode".into());
    }
    let container_name = format!("build-{}-{}", task.name, get_rand_string());
    let create_container_options = CreateContainerOptions {
        name: container_name,
        ..Default::default()
    };

    let mut env: Vec<String> = vec![
        "http_proxy=http://build-egress:3128".into(),
        "https_proxy=http://build-egress:3128".into(),
        "HTTP_PROXY=http://build-egress:3128".into(),
        "HTTPS_PROXY=http://build-egress:3128".into(),
        "NO_PROXY=".into(),
        "no_proxy=".into(),
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
            "AB_OPTIONS={}",
            task.clone().options.unwrap_or("".to_string())
        ),
    ];

    if let Some(task_env) = task.env.clone() {
        for env_var in task_env {
            if env_var.name.starts_with("AB_PKG_")
                && env_var.name.len() > 7
                && env_var
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
            {
                env.push(format!("{}={}", env_var.name, env_var.value));
            }
        }
    }

    let secrets = env
        .iter()
        .filter_map(|v| v.split_once('=').map(|(_, value)| value.to_owned()))
        .collect::<Vec<_>>();
    let image = get_image_name();
    let image_policy = docker.inspect_image(&image).await?;
    if image_policy
        .config
        .as_ref()
        .and_then(|config| config.volumes.as_ref())
        .is_some_and(|volumes| !volumes.is_empty())
    {
        return Err("builder images must not declare unbounded persistent volumes".into());
    }

    let host_mib = features
        .mem_total
        .ok_or("Docker did not report host memory")?
        / (1024 * 1024);
    let memory_mib = env::var("AB_BUILD_MEMORY_MIB")
        .unwrap_or_else(|_| (host_mib / 4).min(8192).to_string())
        .parse::<i64>()?;
    let disk_mib = env::var("AB_BUILD_DISK_MIB")
        .unwrap_or_else(|_| (memory_mib * 3 / 4).min(6144).to_string())
        .parse::<i64>()?;
    let output_mib = env::var("AB_BUILD_OUTPUT_MIB")
        .unwrap_or("1024".into())
        .parse::<i64>()?;
    if !(1024..=131072).contains(&memory_mib)
        || !(512..=65536).contains(&disk_mib)
        || !(64..=16384).contains(&output_mib)
    {
        return Err("invalid build resource policy".into());
    }
    let timeout_seconds = env::var("AB_BUILD_TIMEOUT_SECONDS")
        .unwrap_or("14400".into())
        .parse::<u64>()?;
    if !(1..=14400).contains(&timeout_seconds) {
        return Err("build timeout must be between 1 and 14400 seconds".into());
    }
    let create_container_config = Config {
        image: Some(image),
        user: Some("1000:1000".to_string()),
        env: Some(env),
        labels: Some(std::collections::HashMap::from([(
            "org.aur-builder.attempt".into(),
            task.attempt.clone(),
        )])),
        host_config: Some(HostConfig {
            // Keep the container until its final logs have been collected.
            auto_remove: Some(false),
            network_mode: Some(network),
            cap_drop: Some(vec!["ALL".into()]),
            security_opt: Some(vec![
                "no-new-privileges:true".into(),
                format!("seccomp={}", include_str!("seccomp.json")),
            ]),
            readonly_rootfs: Some(true),
            tmpfs: Some(std::collections::HashMap::from([
                (
                    "/sandbox".into(),
                    format!("rw,exec,nosuid,nodev,size={disk_mib}m,uid=1000,gid=1000,mode=0700"),
                ),
                (
                    "/results".into(),
                    format!(
                        "rw,noexec,nosuid,nodev,size={output_mib}m,uid=1000,gid=1000,mode=0700"
                    ),
                ),
                (
                    "/tmp".into(),
                    "rw,exec,nosuid,nodev,size=64m,mode=1777".into(),
                ),
            ])),
            memory: Some(memory_mib * 1024 * 1024),
            memory_swap: Some(memory_mib * 1024 * 1024),
            pids_limit: Some(512),
            log_config: Some(HostConfigLogConfig {
                typ: Some("local".into()),
                config: Some(std::collections::HashMap::from([
                    ("max-size".into(), "10m".into()),
                    ("max-file".into(), "1".into()),
                    ("compress".into(), "false".into()),
                ])),
            }),
            cpu_period: Some(100000),
            cpu_quota: Some(100000),
            ..Default::default()
        }),
        ..Default::default()
    };

    let container = docker
        .create_container(Some(create_container_options), create_container_config)
        .await?;

    let result = tokio::time::timeout(std::time::Duration::from_secs(timeout_seconds), async {
        docker
            .start_container(&container.id, None::<StartContainerOptions<String>>)
            .await?;

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let status = match super::protocol::completed(&docker, &container.id).await {
                Ok(Some(status)) => status,
                Err(error) if error.is::<bollard::errors::Error>() => return Err(error),
                Err(_) => return Ok(failure_result(task, build_start_time, 109, "Invalid build completion record")),
                Ok(None) => {
                    let state = docker.inspect_container(&container.id, None).await?.state;
                    if state.and_then(|s| s.running) != Some(true) {
                        return Ok(failure_result(task, build_start_time, 109, "Builder terminated before completion, possibly due to its resource limits"));
                    }
                    continue;
                }
            };
            let mut logs = docker.logs(
                &container.id,
                Some(LogsOptions::<String> {
                    stdout: true,
                    stderr: true,
                    // tail: "all",  // optional
                    ..Default::default()
                }),
            );
            let mut raw_log = Vec::new();
            while let Some(log_result) = logs.next().await {
                let output = log_result?;
                let data: &[u8] = output.as_ref();
                let available = (10 * 1024 * 1024usize).saturating_sub(raw_log.len());
                raw_log.extend_from_slice(&data[..data.len().min(available)]);
                if data.len() >= available {
                    break;
                }
            }
            // Redact after joining transport frames so chunk boundaries cannot
            // split a credential before scrubbing. Raw output is never emitted.
            let logs_vec = vec![common::redact::log(
                &String::from_utf8_lossy(&raw_log),
                &secrets,
            )];

            let build_end_time = Utc::now().naive_utc();

            let mut results = BuildResultTransmissionFormat {
                artifact: None,
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

            results.status_code = status;
            results.success = status == 0;
            if results.success {
                match tokio::time::timeout(std::time::Duration::from_secs(900),
                    super::artifact::collect_and_upload(&docker, &container.id, task)).await {
                    Ok(Ok(receipt)) => results.artifact = Some(receipt),
                    _ => {
                        results.success = false;
                        results.status_code = 107;
                        results.log_lines.push("Artifact collection or authenticated upload failed".into());
                    }
                }
            }
            return Ok(results);
        }
    })
    .await
    .unwrap_or_else(|_| Ok(failure_result(task, build_start_time, 108, "Build exceeded its execution deadline")));

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
