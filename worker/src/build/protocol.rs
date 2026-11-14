use bollard::{
    Docker,
    container::LogOutput,
    exec::{CreateExecOptions, StartExecResults},
};
use futures_util::StreamExt;
// These commands are fixed and run against the immutable outer image as UID 1000.
// No source text, package names, or environment values enter a shell command.
pub async fn cat(
    docker: &Docker,
    id: &str,
    path: &'static str,
) -> Result<
    (
        String,
        impl futures_util::Stream<Item = Result<LogOutput, bollard::errors::Error>> + Unpin + Send,
    ),
    Box<dyn std::error::Error>,
> {
    assert!(matches!(path, "/results/status" | "/results/bundle.tar"));
    let exec = docker
        .create_exec(
            id,
            CreateExecOptions {
                user: Some("1000:1000"),
                cmd: Some(vec!["/bin/cat", path]),
                attach_stdout: Some(true),
                attach_stderr: Some(false),
                ..Default::default()
            },
        )
        .await?;
    match docker.start_exec(&exec.id, None).await? {
        StartExecResults::Attached { output, .. } => Ok((exec.id, output)),
        _ => Err("detached artifact export is forbidden".into()),
    }
}
pub async fn completed(
    docker: &Docker,
    id: &str,
) -> Result<Option<i64>, Box<dyn std::error::Error>> {
    let (exec, mut stream) = cat(docker, id, "/results/status").await?;
    let mut value = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if value.len() + chunk.as_ref().len() > 4 {
            return Err("oversized build completion record".into());
        }
        value.extend_from_slice(chunk.as_ref());
    }
    if docker.inspect_exec(&exec).await?.exit_code != Some(0) {
        return Ok(None);
    }
    let status = std::str::from_utf8(&value)?.trim().parse::<i64>()?;
    if !(0..=255).contains(&status) {
        return Err("invalid build exit status".into());
    }
    Ok(Some(status))
}
