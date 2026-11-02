use crate::auth::{Identity, check_csrf};
use axum::{
    Extension, Form,
    extract::{Path, Query},
    http::StatusCode,
    response::{Html, Redirect},
};
use database::{
    Database,
    reviews::{Candidate, Snapshot, SourceFile},
};
use serde::{Deserialize, Serialize};
use tera::{Context, Tera};
fn err<E: std::fmt::Display>(e: E) -> StatusCode {
    log::error!("Review request failed: {e}");
    StatusCode::INTERNAL_SERVER_ERROR
}
pub fn context(identity: &Identity, page: &str) -> Context {
    let mut c = Context::new();
    c.insert("version", common::environment::VERSION);
    c.insert("current_page", page);
    c.insert("identity", &identity.user);
    c.insert("csrf", &identity.csrf);
    c
}
// Never serialize candidate payloads into templates: environment values may be secrets.
#[derive(Serialize)]
struct Summary {
    id: String,
    name: String,
    version: String,
    revision: String,
    state: String,
    created_at: i64,
}
impl From<&Candidate> for Summary {
    fn from(c: &Candidate) -> Self {
        Self {
            id: c.id.clone(),
            name: c.payload.name.clone(),
            version: c.payload.version.clone(),
            revision: c.revision.clone(),
            state: c.state.clone(),
            created_at: c.created_at,
        }
    }
}
#[derive(Deserialize)]
pub struct Filter {
    status: Option<String>,
}
pub async fn queue(
    Extension(tera): Extension<Tera>,
    Extension(db): Extension<Database>,
    Extension(identity): Extension<Identity>,
    Query(filter): Query<Filter>,
) -> Result<Html<String>, StatusCode> {
    let mut c = context(&identity, "reviews");
    let status = filter.status.unwrap_or("pending".into());
    let rows = db
        .candidates()
        .await
        .map_err(err)?
        .iter()
        .filter(|c| status == "all" || c.state == status)
        .map(Summary::from)
        .collect::<Vec<_>>();
    c.insert("candidates", &rows);
    c.insert("status", &status);
    Ok(Html(tera.render("reviews.html", &c).map_err(err)?))
}
#[derive(Serialize)]
struct DiffLine {
    kind: String,
    old: Option<usize>,
    new: Option<usize>,
    text: String,
}
#[derive(Serialize)]
struct FileDiff {
    path: String,
    old_path: Option<String>,
    kind: String,
    added: usize,
    removed: usize,
    lines: Vec<DiffLine>,
    old_text: Option<String>,
    new_text: Option<String>,
    message: String,
}
fn file_diff(old: Option<&SourceFile>, new: Option<&SourceFile>) -> FileDiff {
    let path = new.or(old).unwrap().path.clone();
    let kind = match (old, new) {
        (None, Some(_)) => "added",
        (Some(_), None) => "deleted",
        (Some(a), Some(b)) if a.path != b.path => "renamed",
        _ => "modified",
    }
    .to_owned();
    let mut f = FileDiff {
        path,
        old_path: old
            .filter(|a| new.is_some_and(|b| a.path != b.path))
            .map(|a| a.path.clone()),
        kind,
        added: 0,
        removed: 0,
        lines: vec![],
        old_text: old.and_then(|f| f.content.clone()),
        new_text: new.and_then(|f| f.content.clone()),
        message: String::new(),
    };
    if old.is_some_and(|f| f.content.is_none()) || new.is_some_and(|f| f.content.is_none()) {
        f.message = format!(
            "Binary, submodule, or oversized file. Old: {} · New: {}. Review outside the UI before approving.",
            old.map(|f| format!("{} bytes ({})", f.size, f.hash))
                .unwrap_or("absent".into()),
            new.map(|f| format!("{} bytes ({})", f.size, f.hash))
                .unwrap_or("absent".into())
        );
        return f;
    }
    if old.zip(new).is_some_and(|(a, b)| a.mode != b.mode) {
        f.message = format!(
            "File mode changed: {:o} → {:o}",
            old.unwrap().mode,
            new.unwrap().mode
        );
    }
    let before = f.old_text.as_deref().unwrap_or("");
    let after = f.new_text.as_deref().unwrap_or("");
    for change in similar::TextDiff::configure()
        .timeout(std::time::Duration::from_millis(500))
        .diff_lines(before, after)
        .iter_all_changes()
    {
        let kind = match change.tag() {
            similar::ChangeTag::Delete => {
                f.removed += 1;
                "delete"
            }
            similar::ChangeTag::Insert => {
                f.added += 1;
                "insert"
            }
            _ => "equal",
        };
        f.lines.push(DiffLine {
            kind: kind.into(),
            old: change.old_index().map(|i| i + 1),
            new: change.new_index().map(|i| i + 1),
            text: change.value().trim_end_matches('\n').to_owned(),
        });
    }
    f
}
fn diffs(old: &Snapshot, new: &Snapshot) -> Vec<FileDiff> {
    let mut files = vec![];
    let mut consumed = std::collections::HashSet::new();
    for n in &new.files {
        let previous = old.files.iter().find(|o| o.path == n.path).or_else(|| {
            old.files.iter().find(|o| {
                o.hash == n.hash
                    && !new.files.iter().any(|f| f.path == o.path)
                    && !consumed.contains(&o.path)
            })
        });
        if let Some(o) = previous {
            consumed.insert(o.path.clone());
            if o.path == n.path && o.hash == n.hash && o.mode == n.mode {
                continue;
            }
        }
        files.push(file_diff(previous, Some(n)));
    }
    for o in &old.files {
        if !consumed.contains(&o.path) {
            files.push(file_diff(Some(o), None));
        }
    }
    files
}
pub async fn detail(
    Extension(tera): Extension<Tera>,
    Extension(db): Extension<Database>,
    Extension(identity): Extension<Identity>,
    Path(cid): Path<String>,
) -> Result<Html<String>, StatusCode> {
    let candidate = db
        .candidate(&cid)
        .await
        .map_err(err)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let baseline = match &candidate.baseline {
        Some(id) => Some(
            db.candidate(id)
                .await
                .map_err(err)?
                .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?,
        ),
        None => None,
    };
    let mut c = context(&identity, "reviews");
    c.insert("candidate", &Summary::from(&candidate));
    c.insert("baseline_revision", &baseline.as_ref().map(|c| &c.revision));
    c.insert(
        "diffs",
        &diffs(
            &baseline
                .as_ref()
                .map(|c| c.snapshot.clone())
                .unwrap_or(Snapshot {
                    files: vec![],
                    complete: true,
                }),
            &candidate.snapshot,
        ),
    );
    c.insert("incomplete", &!candidate.snapshot.complete);
    c.insert(
        "settings_changed",
        &baseline.as_ref().is_some_and(|b| {
            b.payload.options != candidate.payload.options
                || b.payload.subfolder != candidate.payload.subfolder
                || serde_json::to_string(&b.payload.environment).ok()
                    != serde_json::to_string(&candidate.payload.environment).ok()
                || b.payload.source != candidate.payload.source
        }),
    );
    c.insert(
        "source",
        &candidate
            .payload
            .source
            .as_deref()
            .map(common::config::public_source),
    );
    c.insert("subfolder", &candidate.payload.subfolder);
    c.insert("options", &candidate.payload.options);
    c.insert("events", &db.review_events(&cid).await.map_err(err)?);
    c.insert("ai", &db.ai_result(&cid).await.map_err(err)?);
    Ok(Html(tera.render("review-detail.html", &c).map_err(err)?))
}
#[derive(Deserialize)]
pub struct Decision {
    csrf: String,
    decision: String,
    #[serde(default)]
    comment: String,
}
pub async fn decide(
    Extension(db): Extension<Database>,
    Extension(identity): Extension<Identity>,
    Path(cid): Path<String>,
    Form(form): Form<Decision>,
) -> Result<Redirect, StatusCode> {
    check_csrf(&identity, &form.csrf)?;
    if !["approve", "reject"].contains(&form.decision.as_str())
        || form.comment.len() > 16_000
        || (form.decision == "reject" && form.comment.trim().is_empty())
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let storage = common::storage::Storage::from_env().map_err(err)?;
    if !db
        .decide_release(
            &cid,
            &identity.user.id,
            form.decision == "approve",
            &form.comment,
            |t| storage.grant(t),
        )
        .await
        .map_err(err)?
    {
        return Err(StatusCode::CONFLICT);
    }
    Ok(Redirect::to(&format!("/reviews/{cid}")))
}
pub async fn users(
    Extension(tera): Extension<Tera>,
    Extension(db): Extension<Database>,
    Extension(identity): Extension<Identity>,
) -> Result<Html<String>, StatusCode> {
    let mut c = context(&identity, "users");
    c.insert("users", &db.users().await.map_err(err)?);
    Ok(Html(tera.render("users.html", &c).map_err(err)?))
}
#[derive(Deserialize)]
pub struct Grants {
    csrf: String,
    can_view: Option<String>,
    can_rebuild: Option<String>,
    can_approve: Option<String>,
    disabled: Option<String>,
}
pub async fn update_user(
    Extension(db): Extension<Database>,
    Extension(identity): Extension<Identity>,
    Path(uid): Path<String>,
    Form(form): Form<Grants>,
) -> Result<Redirect, StatusCode> {
    check_csrf(&identity, &form.csrf)?;
    if uid == identity.user.id && form.disabled.is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    db.update_user_access(
        &identity.user.id,
        &uid,
        form.can_view.is_some(),
        form.can_rebuild.is_some(),
        form.can_approve.is_some(),
        form.disabled.is_some(),
    )
    .await
    .map_err(err)?;
    Ok(Redirect::to("/admin/users"))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn f(path: &str, text: &str) -> SourceFile {
        SourceFile {
            path: path.into(),
            content: Some(text.into()),
            hash: text.into(),
            mode: 0o100644,
            size: text.len(),
        }
    }
    #[test]
    fn diff_counts_and_renames() {
        let old = Snapshot {
            files: vec![f("a", "one\ntwo\n"), f("b", "same\n")],
            complete: true,
        };
        let new = Snapshot {
            files: vec![f("a", "one\nthree\n"), f("c", "same\n")],
            complete: true,
        };
        let result = diffs(&old, &new);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].added, 1);
        assert_eq!(result[0].removed, 1);
        assert_eq!(result[1].kind, "renamed");
    }
    #[test]
    fn binary_is_explicit() {
        let mut file = f("a", "");
        file.content = None;
        assert!(!file_diff(None, Some(&file)).message.is_empty());
    }
}

#[cfg(test)]
mod rendering_tests {
    use super::*;
    #[tokio::test]
    async fn renders_reviews_and_users_without_exposing_secrets() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let user = db
            .login_user("https://issuer", "admin", "Admin", true)
            .await
            .unwrap();
        let identity = Identity {
            user,
            csrf: "csrf".into(),
            session_id: "session".into(),
        };
        let tera = Tera::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/templates/**/*.html"
        ))
        .unwrap();
        let data = common::types::PackageSearchResult {
            name: "example".into(),
            version: "1".into(),
            maintainer: "test".into(),
            last_modified: 1,
            source: Some(
                "https://user:repository-token@example.com/pkg.git?token=query-secret".into(),
            ),
            subfolder: None,
            options: None,
            environment: Some(vec![common::types::EnvironmentVariable {
                name: "TOKEN".into(),
                value: "never-expose-this-secret".into(),
            }]),
        };
        db.observe_release(
            &data,
            &"a".repeat(40),
            "first",
            &Snapshot {
                files: vec![SourceFile {
                    path: "PKGBUILD".into(),
                    content: Some("<script>alert(1)</script>".into()),
                    hash: "hash".into(),
                    mode: 0o100644,
                    size: 25,
                }],
                complete: true,
            },
        )
        .await
        .unwrap();
        let candidate = db.candidates().await.unwrap().remove(0);
        let html = detail(
            Extension(tera.clone()),
            Extension(db.clone()),
            Extension(identity.clone()),
            Path(candidate.id.clone()),
        )
        .await
        .unwrap()
        .0;
        assert!(html.contains("Approve and queue build"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>alert"));
        assert!(!html.contains("never-expose-this-secret"));
        assert!(!html.contains("repository-token"));
        assert!(!html.contains("query-secret"));
        db.store_ai(&candidate.id,&serde_json::json!({"model":"test","mode":"feedback","approve":true,"summary":"Looks safe","findings":["<img src=x onerror=alert(1)>"]})).await.unwrap();
        let html = detail(
            Extension(tera.clone()),
            Extension(db.clone()),
            Extension(identity.clone()),
            Path(candidate.id),
        )
        .await
        .unwrap()
        .0;
        assert!(html.contains("AI recommends approval"));
        assert!(!html.contains("<img src=x"));
        assert!(
            queue(
                Extension(tera.clone()),
                Extension(db.clone()),
                Extension(identity.clone()),
                Query(Filter { status: None })
            )
            .await
            .unwrap()
            .0
            .contains("example")
        );
        assert!(
            users(Extension(tera), Extension(db), Extension(identity))
                .await
                .unwrap()
                .0
                .contains("OIDC administrator")
        );
    }
}
