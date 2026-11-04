use crate::*;
use common::types::{BuildTaskTransmissionFormat, PackageSearchResult};
use sea_orm::{ConnectionTrait, TransactionTrait, Value};
use serde::{Deserialize, Serialize};

pub fn now() -> i64 {
    sea_orm::sqlx::types::chrono::Utc::now().timestamp()
}
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn json<T: Serialize>(v: &T) -> Result<String, DbErr> {
    serde_json::to_string(v).map_err(|e| DbErr::Custom(e.to_string()))
}
fn decode<T: serde::de::DeserializeOwned>(v: &str) -> Result<T, DbErr> {
    serde_json::from_str(v).map_err(|e| DbErr::Custom(e.to_string()))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: String,
    pub content: Option<String>,
    pub hash: String,
    pub mode: i32,
    pub size: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub files: Vec<SourceFile>,
    pub complete: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub package_id: i32,
    pub revision: String,
    pub baseline: Option<String>,
    pub payload: PackageSearchResult,
    pub snapshot: Snapshot,
    pub state: String,
    pub created_at: i64,
}
fn candidate(r: sea_orm::QueryResult) -> Result<Candidate, DbErr> {
    Ok(Candidate {
        id: r.try_get("", "id")?,
        package_id: r.try_get("", "package_id")?,
        revision: r.try_get("", "revision")?,
        baseline: r.try_get("", "baseline")?,
        payload: decode(&r.try_get::<String>("", "payload")?)?,
        snapshot: decode(&r.try_get::<String>("", "snapshot")?)?,
        state: r.try_get("", "state")?,
        created_at: r.try_get::<i64>("", "created_at")? / 1_000_000,
    })
}
#[derive(Serialize)]
pub struct ReviewEvent {
    pub actor: String,
    pub decision: String,
    pub comment: String,
    pub created_at: i64,
}
impl Database {
    pub async fn review_rows(
        &self,
        sql: &str,
        values: Vec<Value>,
    ) -> Result<Vec<sea_orm::QueryResult>, DbErr> {
        self.db
            .query_all(self.storage_query(sql, values).await?)
            .await
    }
    pub async fn review_execute(&self, sql: &str, values: Vec<Value>) -> Result<u64, DbErr> {
        Ok(self
            .db
            .execute(self.storage_query(sql, values).await?)
            .await?
            .rows_affected())
    }
    pub async fn candidates(&self) -> Result<Vec<Candidate>, DbErr> {
        self.review_rows(
            "SELECT * FROM release_candidate ORDER BY created_at DESC,id DESC",
            vec![],
        )
        .await?
        .into_iter()
        .map(candidate)
        .collect()
    }
    pub async fn candidate(&self, id: &str) -> Result<Option<Candidate>, DbErr> {
        self.review_rows(
            "SELECT * FROM release_candidate WHERE id=?",
            vec![id.into()],
        )
        .await?
        .into_iter()
        .next()
        .map(candidate)
        .transpose()
    }
    pub async fn observe_release(
        &self,
        data: &PackageSearchResult,
        revision: &str,
        fingerprint: &str,
        snapshot: &Snapshot,
    ) -> Result<(), DbErr> {
        let tx = self.db.begin().await?;
        let existing = PackageMetadata::find()
            .filter(package_metadata::Column::Name.eq(&data.name))
            .one(&tx)
            .await?;
        let package = if let Some(p) = existing {
            p
        } else {
            package_metadata::ActiveModel {
                name: ActiveValue::Set(data.name.clone()),
                version: ActiveValue::Set(data.version.clone()),
                maintainer: ActiveValue::Set(data.maintainer.clone()),
                last_modified: ActiveValue::Set(data.last_modified),
                source: ActiveValue::Set(data.source.clone()),
                subfolder: ActiveValue::Set(data.subfolder.clone()),
                ..Default::default()
            }
            .insert(&tx)
            .await?
        };
        // Lock before checking the last observation, including when source history reverts.
        tx.execute(
            self.storage_query(
                "UPDATE package_metadata SET last_modified=last_modified WHERE id=?",
                vec![package.id.into()],
            )
            .await?,
        )
        .await?;
        let observed = tx
            .query_one(
                self.storage_query(
                    "SELECT fingerprint FROM release_observation WHERE package_id=?",
                    vec![package.id.into()],
                )
                .await?,
            )
            .await?;
        if observed.is_some_and(|r| {
            r.try_get::<String>("", "fingerprint").ok().as_deref() == Some(fingerprint)
        }) {
            return Ok(());
        }
        let baseline=tx.query_one(self.storage_query("SELECT id FROM release_candidate WHERE package_id=? AND state='approved' ORDER BY created_at DESC,id DESC LIMIT 1",vec![package.id.into()]).await?).await?.map(|r|r.try_get::<String>("","id")).transpose()?;
        tx.execute(self.storage_query("UPDATE release_candidate SET state='superseded' WHERE package_id=? AND state='pending'",vec![package.id.into()]).await?).await?;
        tx.execute(self.storage_query("INSERT INTO release_candidate (id,package_id,fingerprint,revision,baseline,payload,snapshot,created_at) VALUES (?,?,?,?,?,?,?,?)",vec![id().into(),package.id.into(),format!("{fingerprint}:{}",id()).into(),revision.into(),baseline.into(),json(data)?.into(),json(snapshot)?.into(),sea_orm::sqlx::types::chrono::Utc::now().timestamp_micros().into()]).await?).await?;
        tx.execute(self.storage_query("INSERT INTO release_observation (package_id,fingerprint) VALUES (?,?) ON CONFLICT(package_id) DO UPDATE SET fingerprint=excluded.fingerprint",vec![package.id.into(),fingerprint.into()]).await?).await?;
        tx.commit().await
    }
    pub async fn decide_release(
        &self,
        cid: &str,
        actor: &str,
        approve: bool,
        comment: &str,
        grant: impl FnOnce(&mut BuildTaskTransmissionFormat) -> Result<(), common::storage::Error>,
    ) -> Result<bool, DbErr> {
        if !approve && comment.trim().is_empty() {
            return Err(DbErr::Custom("rejection requires a comment".into()));
        }
        let tx = self.db.begin().await?;
        let Some(c) = tx
            .query_one(
                self.storage_query(
                    "SELECT * FROM release_candidate WHERE id=?",
                    vec![cid.into()],
                )
                .await?,
            )
            .await?
            .map(candidate)
            .transpose()?
        else {
            return Ok(false);
        };
        tx.execute(
            self.storage_query(
                "UPDATE package_metadata SET last_modified=last_modified WHERE id=?",
                vec![c.package_id.into()],
            )
            .await?,
        )
        .await?;
        let state = if approve { "approved" } else { "rejected" };
        let changed = tx
            .execute(
                self.storage_query(
                    "UPDATE release_candidate SET state=? WHERE id=? AND state='pending'",
                    vec![state.into(), cid.into()],
                )
                .await?,
            )
            .await?
            .rows_affected();
        if changed != 1 {
            return Ok(false);
        }
        if approve {
            let d = &c.payload;
            let mut task = BuildTaskTransmissionFormat {
                candidate_id: Some(c.id.clone()),
                revision: Some(c.revision.clone()),
                id: c.package_id,
                name: d.name.clone(),
                version: d.version.clone(),
                source: d.source.clone(),
                subfolder: d.subfolder.clone(),
                options: d.options.clone(),
                env: d.environment.clone(),
                attempt: String::new(),
                object_key: String::new(),
                upload: None,
            };
            grant(&mut task).map_err(|_| DbErr::Custom("upload grant failed".into()))?;
            tx.execute(self.attempt_insert(&task).await?).await?;
            tx.execute(self.storage_query("UPDATE package_metadata SET version=?,source=?,subfolder=?,last_modified=? WHERE id=?",vec![d.version.clone().into(),d.source.clone().into(),d.subfolder.clone().into(),d.last_modified.into(),c.package_id.into()]).await?).await?;
        }
        tx.execute(self.storage_query("INSERT INTO review_event (id,candidate_id,actor,decision,comment,created_at) VALUES (?,?,?,?,?,?)",vec![id().into(),cid.into(),actor.into(),state.into(),comment.into(),now().into()]).await?).await?;
        tx.commit().await?;
        Ok(true)
    }
    pub async fn approved_task(&self, t: &BuildTaskTransmissionFormat) -> Result<bool, DbErr> {
        let (Some(cid), Some(revision)) = (&t.candidate_id, &t.revision) else {
            return Ok(false);
        };
        let Some(c) = self.candidate(cid).await? else {
            return Ok(false);
        };
        let d = c.payload;
        Ok(c.state == "approved"
            && c.revision == *revision
            && c.package_id == t.id
            && d.name == t.name
            && d.version == t.version
            && d.source == t.source
            && d.subfolder == t.subfolder
            && d.options == t.options
            && json(&d.environment)? == json(&t.env)?)
    }
    pub async fn has_approved_release(&self, pid: i32) -> Result<bool, DbErr> {
        Ok(!self
            .review_rows(
                "SELECT id FROM release_candidate WHERE package_id=? AND state='approved' LIMIT 1",
                vec![pid.into()],
            )
            .await?
            .is_empty())
    }
    pub async fn rebuild_approved(
        &self,
        pid: i32,
        actor: &str,
        grant: impl FnOnce(&mut BuildTaskTransmissionFormat) -> Result<(), common::storage::Error>,
    ) -> Result<bool, DbErr> {
        let tx = self.db.begin().await?;
        tx.execute(
            self.storage_query(
                "UPDATE package_metadata SET last_modified=last_modified WHERE id=?",
                vec![pid.into()],
            )
            .await?,
        )
        .await?;
        let Some(c)=tx.query_one(self.storage_query("SELECT * FROM release_candidate WHERE package_id=? AND state='approved' ORDER BY created_at DESC,id DESC LIMIT 1",vec![pid.into()]).await?).await?.map(candidate).transpose()? else{return Ok(false)};
        let d = c.payload;
        let mut task = BuildTaskTransmissionFormat {
            candidate_id: Some(c.id.clone()),
            revision: Some(c.revision),
            id: pid,
            name: d.name,
            version: d.version,
            source: d.source,
            subfolder: d.subfolder,
            options: d.options,
            env: d.environment,
            attempt: String::new(),
            object_key: String::new(),
            upload: None,
        };
        grant(&mut task).map_err(|_| DbErr::Custom("upload grant failed".into()))?;
        tx.execute(self.attempt_insert(&task).await?).await?;
        tx.execute(self.storage_query("INSERT INTO review_event (id,candidate_id,actor,decision,comment,created_at) VALUES (?,?,?,?,?,?)",vec![id().into(),c.id.into(),actor.into(),"rebuild".into(),"".into(),now().into()]).await?).await?;
        tx.commit().await?;
        Ok(true)
    }
    pub async fn review_events(&self, cid: &str) -> Result<Vec<ReviewEvent>, DbErr> {
        self.review_rows(
            "SELECT * FROM review_event WHERE candidate_id=? ORDER BY created_at",
            vec![cid.into()],
        )
        .await?
        .into_iter()
        .map(|r| {
            Ok(ReviewEvent {
                actor: r.try_get("", "actor")?,
                decision: r.try_get("", "decision")?,
                comment: r.try_get("", "comment")?,
                created_at: r.try_get::<i64>("", "created_at")? / 1_000_000,
            })
        })
        .collect()
    }
    pub async fn ai_result(&self, cid: &str) -> Result<Option<serde_json::Value>, DbErr> {
        self.review_rows(
            "SELECT result FROM ai_review WHERE candidate_id=?",
            vec![cid.into()],
        )
        .await?
        .into_iter()
        .next()
        .map(|r| decode(&r.try_get::<String>("", "result")?))
        .transpose()
    }
    pub async fn store_ai(&self, cid: &str, result: &serde_json::Value) -> Result<(), DbErr> {
        self.review_execute("INSERT INTO ai_review (candidate_id,result) VALUES (?,?) ON CONFLICT(candidate_id) DO UPDATE SET result=excluded.result",vec![cid.into(),json(result)?.into()]).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn data() -> PackageSearchResult {
        PackageSearchResult {
            name: "example".into(),
            version: "1-1".into(),
            maintainer: "test".into(),
            last_modified: 1,
            source: Some("https://example.com/pkg.git".into()),
            subfolder: None,
            options: None,
            environment: None,
        }
    }
    fn snapshot() -> Snapshot {
        Snapshot {
            files: vec![SourceFile {
                path: "PKGBUILD".into(),
                content: Some("pkgver=1".into()),
                hash: "hash".into(),
                mode: 0o100644,
                size: 8,
            }],
            complete: true,
        }
    }
    fn grant(t: &mut BuildTaskTransmissionFormat) -> Result<(), common::storage::Error> {
        t.attempt = id();
        t.object_key = format!("builds/{}/bundle.tar", t.attempt);
        Ok(())
    }
    #[tokio::test]
    async fn approval_gate_is_atomic_pinned_and_idempotent() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        db.observe_release(&data(), &"a".repeat(40), "one", &snapshot())
            .await
            .unwrap();
        db.observe_release(&data(), &"a".repeat(40), "one", &snapshot())
            .await
            .unwrap();
        assert_eq!(db.candidates().await.unwrap().len(), 1);
        assert!(db.pending_attempts().await.unwrap().is_empty());
        let c = db.candidates().await.unwrap().remove(0);
        assert!(c.baseline.is_none());
        assert!(!db.has_approved_release(c.package_id).await.unwrap());
        assert!(
            !db.rebuild_approved(c.package_id, "human", |_| panic!(
                "unapproved release must not receive an upload grant"
            ))
            .await
            .unwrap()
        );
        assert!(
            db.decide_release(&c.id, "human", true, "", |_| Err("failed".into()))
                .await
                .is_err()
        );
        assert_eq!(db.candidate(&c.id).await.unwrap().unwrap().state, "pending");
        assert!(db.pending_attempts().await.unwrap().is_empty());
        assert!(
            db.decide_release(&c.id, "human", true, "checked", grant)
                .await
                .unwrap()
        );
        assert!(db.has_approved_release(c.package_id).await.unwrap());
        assert!(
            !db.decide_release(&c.id, "second", true, "", grant)
                .await
                .unwrap()
        );
        let mut task = db.pending_attempts().await.unwrap().remove(0);
        assert_eq!(task.revision, Some("a".repeat(40)));
        assert!(db.valid_attempt(&task).await.unwrap());
        task.revision = Some("b".repeat(40));
        assert!(!db.valid_attempt(&task).await.unwrap());
        task.revision = Some("a".repeat(40));
        task.options = Some("--skipchecksums".into());
        assert!(!db.valid_attempt(&task).await.unwrap());
        assert_eq!(db.review_events(&c.id).await.unwrap().len(), 1);
        assert!(
            db.rebuild_approved(c.package_id, "human", grant)
                .await
                .unwrap()
        );
        let tasks = db.pending_attempts().await.unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].revision, tasks[1].revision);
    }
    #[tokio::test]
    async fn supersession_rejection_and_baseline() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let d = data();
        db.observe_release(&d, &"a".repeat(40), "one", &snapshot())
            .await
            .unwrap();
        let first = db.candidates().await.unwrap().remove(0);
        db.observe_release(&d, &"b".repeat(40), "two", &snapshot())
            .await
            .unwrap();
        assert_eq!(
            db.candidate(&first.id).await.unwrap().unwrap().state,
            "superseded"
        );
        assert!(
            !db.decide_release(&first.id, "human", true, "", grant)
                .await
                .unwrap()
        );
        let second = db
            .candidates()
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.state == "pending")
            .unwrap();
        assert!(!db.has_approved_release(second.package_id).await.unwrap());
        assert!(
            db.decide_release(&second.id, "human", false, "", grant)
                .await
                .is_err()
        );
        assert!(
            db.decide_release(&second.id, "human", false, "unsafe", grant)
                .await
                .unwrap()
        );
        assert!(db.pending_attempts().await.unwrap().is_empty());
        assert!(!db.has_approved_release(second.package_id).await.unwrap());
        db.observe_release(&d, &"c".repeat(40), "three", &snapshot())
            .await
            .unwrap();
        let third = db
            .candidates()
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.state == "pending")
            .unwrap();
        db.decide_release(&third.id, "human", true, "", grant)
            .await
            .unwrap();
        db.observe_release(&d, &"d".repeat(40), "four", &snapshot())
            .await
            .unwrap();
        let fourth = db
            .candidates()
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.state == "pending")
            .unwrap();
        assert_eq!(fourth.baseline, Some(third.id));
    }
}

#[cfg(test)]
mod observation_regression_tests {
    use super::*;
    #[tokio::test]
    async fn reverting_source_supersedes_pending_and_requires_a_fresh_review() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let data = PackageSearchResult {
            name: "reverted".into(),
            version: "1".into(),
            maintainer: "test".into(),
            last_modified: 1,
            source: None,
            subfolder: None,
            options: None,
            environment: None,
        };
        let snapshot = Snapshot {
            files: vec![],
            complete: true,
        };
        db.observe_release(&data, &"a".repeat(40), "a", &snapshot)
            .await
            .unwrap();
        let original = db.candidates().await.unwrap().remove(0);
        db.decide_release(&original.id, "human", true, "", |t| {
            t.attempt = "first".into();
            t.object_key = "first".into();
            Ok(())
        })
        .await
        .unwrap();
        db.observe_release(&data, &"b".repeat(40), "b", &snapshot)
            .await
            .unwrap();
        let stale = db
            .candidates()
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.state == "pending")
            .unwrap();
        db.observe_release(&data, &"a".repeat(40), "a", &snapshot)
            .await
            .unwrap();
        assert_eq!(
            db.candidate(&stale.id).await.unwrap().unwrap().state,
            "superseded"
        );
        assert!(
            !db.decide_release(&stale.id, "human", true, "", |_| panic!(
                "stale revision must not build"
            ))
            .await
            .unwrap()
        );
        let candidates = db.candidates().await.unwrap();
        assert_eq!(candidates.len(), 3);
        let current = candidates.iter().find(|c| c.state == "pending").unwrap();
        assert_ne!(current.id, original.id);
        assert_eq!(current.revision, original.revision);
        assert_eq!(current.baseline.as_deref(), Some(original.id.as_str()));
        db.observe_release(&data, &"a".repeat(40), "a", &snapshot)
            .await
            .unwrap();
        assert_eq!(db.candidates().await.unwrap().len(), 3);
        db.decide_release(&current.id, "human", false, "rejected", |_| unreachable!())
            .await
            .unwrap();
        db.observe_release(&data, &"b".repeat(40), "b", &snapshot)
            .await
            .unwrap();
        assert_eq!(
            db.candidates()
                .await
                .unwrap()
                .iter()
                .filter(|c| c.state == "pending")
                .count(),
            1
        );
    }
}
