use bollard::Docker;
use common::{artifact::Receipt, types::BuildTaskTransmissionFormat};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

pub async fn collect_and_upload(
    docker: &Docker,
    container: &str,
    task: &BuildTaskTransmissionFormat,
) -> Result<Receipt, Box<dyn std::error::Error>> {
    let limit = std::env::var("AB_REPO_MAX_BYTES")
        .unwrap_or("4294967296".into())
        .parse::<u64>()?;
    let output_mib = std::env::var("AB_BUILD_OUTPUT_MIB")
        .unwrap_or("1024".into())
        .parse::<u64>()?;
    if !(64..=16384).contains(&output_mib) {
        return Err("invalid output budget".into());
    }
    let limit = limit.min(output_mib * 1024 * 1024);
    let temp = tempfile::tempdir()?;
    let bundle = temp.path().join("bundle.tar");
    let mut file = tokio::fs::File::create(&bundle).await?;
    let (exec, mut stream) = super::protocol::cat(docker, container, "/results/bundle.tar").await?;
    let mut bytes = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let data: &[u8] = chunk.as_ref();
        bytes = bytes
            .checked_add(data.len() as u64)
            .ok_or("artifact size overflow")?;
        if bytes > limit {
            return Err("artifact size limit exceeded".into());
        }
        file.write_all(data).await?;
    }
    file.flush().await?;
    drop(file);
    if docker.inspect_exec(&exec).await?.exit_code != Some(0) {
        return Err("artifact export failed".into());
    }
    // All subsequent hashing and uploads use worker-private bytes. The sandbox
    // cannot alter this file, even if it changed its output during collection.
    docker.pause_container(container).await?;
    let key = std::env::var("AB_ARTIFACT_KEY")?;
    let receipt =
        common::artifact::issue(&key, task, &bundle).map_err(|_| "cannot authenticate artifact")?;
    let storage =
        common::storage::Storage::from_env().map_err(|_| "invalid worker storage configuration")?;
    let ttl = std::env::var("AB_UPLOAD_TTL")
        .unwrap_or("900".into())
        .parse::<u64>()?;
    if !(60..=900).contains(&ttl) {
        return Err("AB_UPLOAD_TTL must be between 60 and 900".into());
    }
    let url = storage
        .sign("PUT", &task.object_key, ttl, false)
        .map_err(|_| "cannot issue upload grant")?;
    let file = tokio::fs::File::open(&bundle).await?;
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(ttl))
        .build()?
        .put(url)
        .header(reqwest::header::CONTENT_LENGTH, receipt.bytes)
        .body(reqwest::Body::wrap_stream(
            tokio_util::io::ReaderStream::new(file),
        ))
        .send()
        .await
        .map_err(|_| "artifact upload failed")?;
    if !response.status().is_success() {
        return Err("artifact upload rejected".into());
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires Docker with alpine image and local HTTP test listener"]
    async fn frozen_tmpfs_bundle_is_uploaded_and_authenticated() {
        use bollard::container::{
            Config, CreateContainerOptions, RemoveContainerOptions, StartContainerOptions,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let app = axum::Router::new().fallback(axum::routing::put(
            move |request: axum::extract::Request| {
                let tx = tx.clone();
                async move {
                    assert!(request.uri().query().unwrap().contains("X-Amz-Expires=900"));
                    let bytes = axum::body::to_bytes(request.into_body(), 1024 * 1024)
                        .await
                        .unwrap();
                    tx.send(bytes).await.unwrap();
                    axum::http::StatusCode::OK
                }
            },
        ));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        for (name, value) in [
            ("AB_ARTIFACT_KEY", "only-worker-and-repo-know-this-key"),
            ("AB_S3_ENDPOINT", endpoint.as_str()),
            ("AB_S3_BUCKET", "test"),
            ("AB_S3_ACCESS_KEY", "test"),
            ("AB_S3_SECRET_KEY", "secret"),
            ("AB_UPLOAD_TTL", "900"),
        ] {
            unsafe {
                std::env::set_var(name, value);
            }
        }
        let docker = Docker::connect_with_local_defaults().unwrap();
        let container = docker.create_container(None::<CreateContainerOptions<String>>, Config {
            image: Some("alpine"), user: Some("1000:1000"),
            cmd: Some(vec!["sh", "-ec", "echo package > /tmp/test.pkg.tar.gz; tar -cf /results/bundle.tar -C /tmp test.pkg.tar.gz; echo 0 > /results/status; sleep 3600"]),
            host_config: Some(bollard::models::HostConfig {
                cap_drop: Some(vec!["ALL".into()]),
                security_opt: Some(vec!["no-new-privileges:true".into(), format!("seccomp={}", include_str!("seccomp.json"))]),
                readonly_rootfs: Some(true),
                tmpfs: Some(std::collections::HashMap::from([
                    ("/results".into(), "size=64m,uid=1000,gid=1000,mode=0700".into()),
                    ("/tmp".into(), "size=64m,mode=1777".into()),
                ])), ..Default::default()
            }), ..Default::default()
        }).await.unwrap();
        docker
            .start_container(&container.id, None::<StartContainerOptions<String>>)
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Some(status) = crate::build::protocol::completed(&docker, &container.id)
                    .await
                    .unwrap()
                {
                    assert_eq!(status, 0);
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        })
        .await
        .unwrap();
        let task = serde_json::from_value(serde_json::json!({"id":1,"attempt":"attempt","object_key":"builds/attempt/bundle.tar","name":"test","version":"1","source":null,"options":null,"subfolder":null,"env":null})).unwrap();
        let result = collect_and_upload(&docker, &container.id, &task).await;
        docker
            .remove_container(
                &container.id,
                Some(RemoveContainerOptions {
                    force: true,
                    v: true,
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        let receipt = result.unwrap();
        let bytes = rx.recv().await.unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), bytes).unwrap();
        common::artifact::verify(
            "only-worker-and-repo-know-this-key",
            &task,
            file.path(),
            &receipt,
        )
        .unwrap();
        server.abort();
    }
}

#[cfg(test)]
mod full_build_test {
    #[tokio::test]
    #[ignore = "requires scripts/test_worker_build.py disposable hanging image and network"]
    async fn blocked_completion_obeys_deadline_and_removes_container() {
        let attempt = format!("timeout-{}", common::get_rand_string());
        let task = serde_json::from_value(serde_json::json!({"id":1,"revision":"a".repeat(40),"attempt":attempt,"object_key":"builds/timeout/bundle.tar","name":"security-timeout","version":"1-1","source":null,"options":null,"subfolder":null,"env":null})).unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            crate::build::build_package(&task),
        )
        .await
        .expect("worker ignored execution deadline");
        let result = result.unwrap();
        assert!(!result.success);
        assert_eq!(result.status_code, 108);
        assert!(result.artifact.is_none());
        let docker = bollard::Docker::connect_with_local_defaults().unwrap();
        let containers = docker
            .list_containers(Some(bollard::container::ListContainersOptions::<String> {
                all: true,
                filters: std::collections::HashMap::from([(
                    "label".into(),
                    vec![format!("org.aur-builder.attempt={attempt}")],
                )]),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert!(containers.is_empty(), "timed-out container was not removed");
    }

    #[tokio::test]
    #[ignore = "requires scripts/test_worker_build.py disposable image and network"]
    async fn approved_source_to_authenticated_bundle() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let app = axum::Router::new().fallback(axum::routing::put(
            move |request: axum::extract::Request| {
                let tx = tx.clone();
                async move {
                    assert!(request.uri().query().unwrap().contains("X-Amz-Expires=900"));
                    let bytes = axum::body::to_bytes(request.into_body(), 1024 * 1024)
                        .await
                        .unwrap();
                    tx.send(bytes).await.unwrap();
                    axum::http::StatusCode::OK
                }
            },
        ));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        for (name, value) in [
            ("AB_ARTIFACT_KEY", "only-worker-and-repo-know-this-key"),
            ("AB_S3_ENDPOINT", endpoint.as_str()),
            ("AB_S3_BUCKET", "test"),
            ("AB_S3_ACCESS_KEY", "test"),
            ("AB_S3_SECRET_KEY", "secret"),
            ("AB_UPLOAD_TTL", "900"),
        ] {
            unsafe {
                std::env::set_var(name, value);
            }
        }
        let task = serde_json::from_value(serde_json::json!({"id":1,"candidate_id":"approved-fixture","revision":std::env::var("AB_TEST_REVISION").unwrap(),"attempt":"attempt","object_key":"builds/attempt/bundle.tar","name":"security-fixture","version":"1-1","source":"file:///fixture","options":null,"subfolder":null,"env":[{"name":"AB_PKG_SECRET","value":"sensitive-fixture-value"},{"name":"PATH","value":"/untrusted"},{"name":"LD_PRELOAD","value":"/untrusted.so"},{"name":"AB_UPLOAD_URL","value":"forbidden-override"}]})).unwrap();
        let result = crate::build::build_package(&task).await.unwrap();
        assert!(result.success, "build failed: {:?}", result.log_lines);
        assert_eq!(result.status_code, 0);
        let log = result.log_lines.join("");
        assert!(!log.contains("sensitive-fixture-value"));
        assert!(log.contains("[REDACTED]"));
        let bytes = rx.recv().await.unwrap();
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), bytes).unwrap();
        common::artifact::verify(
            "only-worker-and-repo-know-this-key",
            &task,
            file.path(),
            result.artifact.as_ref().unwrap(),
        )
        .unwrap();
        let mut archive = tar::Archive::new(std::fs::File::open(file.path()).unwrap());
        let names = archive
            .entries()
            .unwrap()
            .map(|entry| {
                entry
                    .unwrap()
                    .path()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        assert!(
            names
                .iter()
                .any(|name| name.starts_with("security-fixture-1-1-any.pkg.tar."))
        );
        server.abort();
    }
}
