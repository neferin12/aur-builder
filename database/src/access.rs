use crate::Database;
use crate::reviews::{id, now};
use sea_orm::DbErr;
use serde::Serialize;
use sha2::{Digest, Sha256};
pub fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}
#[derive(Clone, Debug, Serialize)]
pub struct User {
    pub id: String,
    pub display_name: String,
    pub issuer: String,
    pub subject: String,
    pub admin: bool,
    pub disabled: bool,
    pub can_view: bool,
    pub can_rebuild: bool,
    pub can_approve: bool,
}
impl User {
    pub fn allows(&self, p: &str) -> bool {
        !self.disabled
            && (self.admin
                || (self.can_view
                    && match p {
                        "view" => true,
                        "rebuild" => self.can_rebuild,
                        "approve" => self.can_approve,
                        _ => false,
                    }))
    }
}
fn user(r: sea_orm::QueryResult) -> Result<User, DbErr> {
    Ok(User {
        id: r.try_get("", "id")?,
        display_name: r.try_get("", "display_name")?,
        issuer: r.try_get("", "issuer")?,
        subject: r.try_get("", "subject")?,
        admin: r.try_get("", "admin")?,
        disabled: r.try_get("", "disabled")?,
        can_view: r.try_get("", "can_view")?,
        can_rebuild: r.try_get("", "can_rebuild")?,
        can_approve: r.try_get("", "can_approve")?,
    })
}
impl Database {
    pub async fn login_user(
        &self,
        issuer: &str,
        subject: &str,
        name: &str,
        admin: bool,
    ) -> Result<User, DbErr> {
        use sea_orm::{ConnectionTrait, TransactionTrait};
        let tx = self.db.begin().await?;
        tx.execute(
            self.storage_query(
                "UPDATE web_user SET id=id WHERE issuer=? AND subject=?",
                vec![issuer.into(), subject.into()],
            )
            .await?,
        )
        .await?;
        tx.execute(self.storage_query("DELETE FROM web_session WHERE user_id IN (SELECT id FROM web_user WHERE issuer=? AND subject=? AND admin<>?)",vec![issuer.into(),subject.into(),admin.into()]).await?).await?;
        tx.execute(self.storage_query("INSERT INTO web_user (id,issuer,subject,display_name,admin) VALUES (?,?,?,?,?) ON CONFLICT(issuer,subject) DO UPDATE SET display_name=excluded.display_name,admin=excluded.admin",vec![id().into(),issuer.into(),subject.into(),name.into(),admin.into()]).await?).await?;
        let result = user(
            tx.query_one(
                self.storage_query(
                    "SELECT * FROM web_user WHERE issuer=? AND subject=?",
                    vec![issuer.into(), subject.into()],
                )
                .await?,
            )
            .await?
            .ok_or(DbErr::RecordNotFound("user".into()))?,
        )?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn users(&self) -> Result<Vec<User>, DbErr> {
        self.review_rows("SELECT * FROM web_user ORDER BY display_name", vec![])
            .await?
            .into_iter()
            .map(user)
            .collect()
    }
    pub async fn session(&self, sid: &str) -> Result<Option<(User, String)>, DbErr> {
        let mut rows=self.review_rows("SELECT web_user.*,web_session.csrf FROM web_session JOIN web_user ON web_user.id=web_session.user_id WHERE web_session.id=? AND expires_at>?",vec![token_hash(sid).into(),now().into()]).await?;
        if let Some(r) = rows.pop() {
            let csrf = r.try_get("", "csrf")?;
            Ok(Some((user(r)?, csrf)))
        } else {
            Ok(None)
        }
    }
    pub async fn create_session(&self, uid: &str, ttl: i64) -> Result<(String, String), DbErr> {
        self.create_bound_session(uid, ttl, "").await
    }
    pub async fn bound_session(
        &self,
        sid: &str,
        context: &str,
    ) -> Result<Option<(User, String)>, DbErr> {
        let rows=self.review_rows("SELECT web_user.*,web_session.csrf FROM web_session JOIN web_user ON web_user.id=web_session.user_id WHERE web_session.id=? AND expires_at>? AND auth_context=?",vec![token_hash(sid).into(),now().into(),context.into()]).await?;
        rows.into_iter()
            .next()
            .map(|r| {
                let csrf = r.try_get("", "csrf")?;
                Ok((user(r)?, csrf))
            })
            .transpose()
    }
    pub async fn delete_session(&self, sid: &str) -> Result<(), DbErr> {
        self.review_execute(
            "DELETE FROM web_session WHERE id=?",
            vec![token_hash(sid).into()],
        )
        .await?;
        Ok(())
    }
    pub async fn create_bound_session(
        &self,
        uid: &str,
        ttl: i64,
        context: &str,
    ) -> Result<(String, String), DbErr> {
        if ttl <= 0 || ttl > 86400 {
            return Err(DbErr::Custom("invalid session lifetime".into()));
        }
        let sid = id();
        let csrf = id();
        self.review_execute(
            "DELETE FROM web_session WHERE expires_at<=?",
            vec![now().into()],
        )
        .await?;
        let changed=self.review_execute("INSERT INTO web_session (id,user_id,csrf,expires_at,auth_context) SELECT ?,id,?,?,? FROM web_user WHERE id=? AND disabled=FALSE",vec![token_hash(&sid).into(),csrf.clone().into(),(now()+ttl).into(),context.into(),uid.into()]).await?;
        if changed != 1 {
            return Err(DbErr::RecordNotFound("active user".into()));
        }
        Ok((sid, csrf))
    }
    pub async fn update_user_access(
        &self,
        actor: &str,
        target: &str,
        view: bool,
        rebuild: bool,
        approve: bool,
        disabled: bool,
    ) -> Result<(), DbErr> {
        use sea_orm::{ConnectionTrait, TransactionTrait};
        let tx = self.db.begin().await?;
        tx.execute(
            self.storage_query(
                "UPDATE web_user SET can_view=?,can_rebuild=?,can_approve=?,disabled=? WHERE id=?",
                vec![
                    view.into(),
                    rebuild.into(),
                    approve.into(),
                    disabled.into(),
                    target.into(),
                ],
            )
            .await?,
        )
        .await?;
        tx.execute(
            self.storage_query(
                "DELETE FROM web_session WHERE user_id=?",
                vec![target.into()],
            )
            .await?,
        )
        .await?;
        tx.execute(
            self.storage_query(
                "INSERT INTO access_audit (id,actor,target,detail,created_at) VALUES (?,?,?,?,?)",
                vec![
                    id().into(),
                    actor.into(),
                    target.into(),
                    format!("view={view},rebuild={rebuild},approve={approve},disabled={disabled}")
                        .into(),
                    now().into(),
                ],
            )
            .await?,
        )
        .await?;
        tx.commit().await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn default_denied_grants_revocation_and_group_admin() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let user = db
            .login_user("https://issuer", "subject", "Name", false)
            .await
            .unwrap();
        assert!(!user.allows("view"));
        assert!(!user.allows("approve"));
        let (sid, _) = db.create_session(&user.id, 3600).await.unwrap();
        assert!(db.session(&sid).await.unwrap().is_some());
        db.update_user_access("admin", &user.id, true, false, true, false)
            .await
            .unwrap();
        assert!(db.session(&sid).await.unwrap().is_none());
        let user = db
            .login_user("https://issuer", "subject", "Name", false)
            .await
            .unwrap();
        assert!(user.allows("view"));
        assert!(user.allows("approve"));
        assert!(!user.allows("rebuild"));
        let other = db
            .login_user("https://other", "subject", "Other", false)
            .await
            .unwrap();
        assert_ne!(other.id, user.id);
        assert!(!other.allows("view"));
        let admin = db
            .login_user("https://issuer", "subject", "Name", true)
            .await
            .unwrap();
        assert!(admin.allows("rebuild"));
        let removed = db
            .login_user("https://issuer", "subject", "Name", false)
            .await
            .unwrap();
        assert!(!removed.admin);
        assert!(!removed.allows("rebuild"));
        db.update_user_access("admin", &user.id, true, true, true, true)
            .await
            .unwrap();
        let disabled = db
            .login_user("https://issuer", "subject", "Name", true)
            .await
            .unwrap();
        assert!(!disabled.allows("view"));
    }
}

#[cfg(test)]
mod session_security_tests {
    use super::*;
    #[tokio::test]
    async fn sessions_are_hashed_bound_and_revoked_on_group_changes() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let user = db
            .login_user("https://issuer", "subject", "Name", false)
            .await
            .unwrap();
        let (sid, _) = db
            .create_bound_session(&user.id, 3600, "oidc-config-a")
            .await
            .unwrap();
        assert!(
            db.bound_session(&sid, "oidc-config-a")
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            db.bound_session(&sid, "oidc-config-b")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            db.review_rows(
                "SELECT id FROM web_session WHERE id=?",
                vec![sid.clone().into()]
            )
            .await
            .unwrap()
            .is_empty()
        );
        assert!(db.session(&token_hash(&sid)).await.unwrap().is_none());
        db.login_user("https://issuer", "subject", "Name", true)
            .await
            .unwrap();
        assert!(
            db.session(&sid).await.unwrap().is_none(),
            "an old unprivileged cookie must not become an admin cookie"
        );
        let (admin_sid, _) = db
            .create_bound_session(&user.id, 3600, "oidc-config-a")
            .await
            .unwrap();
        db.login_user("https://issuer", "subject", "Name", false)
            .await
            .unwrap();
        assert!(db.session(&admin_sid).await.unwrap().is_none());
        let (sid, _) = db
            .create_bound_session(&user.id, 3600, "oidc-config-a")
            .await
            .unwrap();
        db.delete_session(&sid).await.unwrap();
        assert!(db.session(&sid).await.unwrap().is_none());
        assert!(
            db.create_bound_session(&user.id, 0, "context")
                .await
                .is_err()
        );
        db.update_user_access("admin", &user.id, false, false, false, true)
            .await
            .unwrap();
        assert!(
            db.create_bound_session(&user.id, 3600, "context")
                .await
                .is_err()
        );
    }
}
