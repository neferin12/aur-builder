use axum::{
    extract::{Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{Html, IntoResponse, Redirect, Response},
};
use database::{
    Database,
    access::User,
    reviews::{id, now},
};
use openidconnect::{core::*, reqwest::async_http_client, *};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Groups {
    #[serde(flatten)]
    claims: HashMap<String, serde_json::Value>,
}
impl AdditionalClaims for Groups {}
type Token = StandardTokenResponse<
    IdTokenFields<
        Groups,
        EmptyExtraTokenFields,
        CoreGenderClaim,
        CoreJweContentEncryptionAlgorithm,
        CoreJwsSigningAlgorithm,
        CoreJsonWebKeyType,
    >,
    CoreTokenType,
>;
type OidcClient = Client<
    Groups,
    CoreAuthDisplay,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJwsSigningAlgorithm,
    CoreJsonWebKeyType,
    CoreJsonWebKeyUse,
    CoreJsonWebKey,
    CoreAuthPrompt,
    StandardErrorResponse<CoreErrorResponseType>,
    Token,
    CoreTokenType,
    CoreTokenIntrospectionResponse,
    CoreRevocableToken,
    CoreRevocationErrorResponse,
>;
#[derive(Default)]
struct KeyCache {
    client: Option<OidcClient>,
    attempted_at: Option<std::time::Instant>,
    loaded_at: Option<std::time::Instant>,
}
#[derive(Clone)]
pub struct Auth {
    client: OidcClient,
    client_id: String,
    client_secret: ClientSecret,
    key_cache: Arc<tokio::sync::Mutex<KeyCache>>,
    issuer: String,
    group_claim: String,
    admin_group: String,
    pub origin: String,
    pub ttl: i64,
    scopes: Vec<String>,
}
#[derive(Clone)]
pub struct Identity {
    pub user: User,
    pub csrf: String,
    pub session_id: String,
}
#[derive(Serialize, Deserialize)]
struct Login {
    nonce: String,
    verifier: String,
    browser: String,
    #[serde(default)]
    context: String,
}
pub type AuthState = (Arc<Auth>, Database);
fn required(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| panic!("{name} is required for web authentication"))
}
impl Auth {
    pub async fn discover() -> Self {
        let issuer = required("AB_OIDC_ISSUER");
        let origin = required("AB_WEB_URL").trim_end_matches('/').to_owned();
        let url = url::Url::parse(&origin).expect("invalid AB_WEB_URL");
        assert!(
            url.scheme() == "https"
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none()
                && url.username().is_empty()
                && url.password().is_none(),
            "AB_WEB_URL must be an HTTPS origin"
        );
        assert!(
            issuer.starts_with("https://"),
            "AB_OIDC_ISSUER must use HTTPS"
        );
        let metadata = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            CoreProviderMetadata::discover_async(
                IssuerUrl::new(issuer.clone()).expect("invalid OIDC issuer"),
                oidc_http_client,
            ),
        )
        .await
        .expect("OIDC discovery timed out")
        .expect("OIDC discovery failed");
        validate_metadata(&metadata).expect("OIDC endpoints must use HTTPS");
        let origin = url.origin().ascii_serialization();
        let client_id = required("AB_OIDC_CLIENT_ID");
        let client_secret = ClientSecret::new(required("AB_OIDC_CLIENT_SECRET"));
        let client = OidcClient::from_provider_metadata(
            metadata,
            ClientId::new(client_id.clone()),
            Some(client_secret.clone()),
        )
        .set_redirect_uri(RedirectUrl::new(format!("{origin}/auth/callback")).unwrap());
        let ttl = std::env::var("AB_SESSION_SECONDS")
            .map(|s| {
                s.parse::<i64>()
                    .expect("AB_SESSION_SECONDS must be an integer")
            })
            .unwrap_or(3600);
        assert!(
            (60..=86400).contains(&ttl),
            "AB_SESSION_SECONDS must be between 60 and 86400"
        );
        Self {
            client,
            client_id,
            client_secret,
            key_cache: Arc::new(tokio::sync::Mutex::new(KeyCache {
                loaded_at: Some(std::time::Instant::now()),
                ..Default::default()
            })),
            issuer,
            origin,
            ttl,
            group_claim: std::env::var("AB_OIDC_GROUPS_CLAIM").unwrap_or("groups".into()),
            admin_group: required("AB_OIDC_ADMIN_GROUP"),
            scopes: std::env::var("AB_OIDC_SCOPES")
                .unwrap_or("profile".into())
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        }
    }
}
type HttpError = std::io::Error;
fn secure_oidc_url(url: &url::Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && (url.scheme() == "https"
            || (cfg!(test) && url.scheme() == "http" && url.host_str() == Some("127.0.0.1")))
}
async fn oidc_http_client(request: HttpRequest) -> Result<HttpResponse, HttpError> {
    // Unit-test binaries allow only loopback mock endpoints over HTTP.
    if !secure_oidc_url(&request.url) {
        return Err(std::io::Error::other("insecure OIDC endpoint"));
    }
    async_http_client(request)
        .await
        .map_err(std::io::Error::other)
}
fn validate_metadata(metadata: &CoreProviderMetadata) -> Result<(), StatusCode> {
    let secure = |url: &str| url::Url::parse(url).is_ok_and(|u| secure_oidc_url(&u));
    if !secure(metadata.authorization_endpoint().as_str())
        || !secure(metadata.jwks_uri().as_str())
        || !metadata
            .token_endpoint()
            .is_some_and(|u| secure(u.as_str()))
    {
        return Err(StatusCode::BAD_GATEWAY);
    }
    Ok(())
}
impl Auth {
    fn context(&self) -> String {
        database::access::token_hash(
            &serde_json::to_string(&(
                &self.issuer,
                &self.origin,
                &self.client_id,
                self.client_secret.secret(),
                &self.group_claim,
                &self.admin_group,
                &self.scopes,
                self.ttl,
            ))
            .unwrap(),
        )
    }
    async fn current_client(&self) -> Result<OidcClient, StatusCode> {
        {
            let mut cache = self.key_cache.lock().await;
            let loaded = cache.loaded_at.get_or_insert_with(std::time::Instant::now);
            if loaded.elapsed() < std::time::Duration::from_secs(300) {
                return Ok(cache.client.clone().unwrap_or_else(|| self.client.clone()));
            }
        }
        self.refresh_client().await
    }
    async fn refresh_client(&self) -> Result<OidcClient, StatusCode> {
        let mut cache = self.key_cache.lock().await;
        if cache
            .attempted_at
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(60))
        {
            return cache
                .client
                .clone()
                .filter(|_| {
                    cache
                        .loaded_at
                        .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(300))
                })
                .ok_or(StatusCode::SERVICE_UNAVAILABLE);
        }
        cache.attempted_at = Some(std::time::Instant::now());
        let metadata = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            CoreProviderMetadata::discover_async(
                IssuerUrl::new(self.issuer.clone())
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                oidc_http_client,
            ),
        )
        .await
        .map_err(|_| StatusCode::GATEWAY_TIMEOUT)?
        .map_err(|_| StatusCode::BAD_GATEWAY)?;
        validate_metadata(&metadata)?;
        let client = OidcClient::from_provider_metadata(
            metadata,
            ClientId::new(self.client_id.clone()),
            Some(self.client_secret.clone()),
        )
        .set_redirect_uri(RedirectUrl::new(format!("{}/auth/callback", self.origin)).unwrap());
        cache.client = Some(client.clone());
        cache.loaded_at = Some(std::time::Instant::now());
        Ok(client)
    }
}
#[derive(Clone, Copy)]
enum Permission {
    View,
    Rebuild,
    Approve,
    Admin,
    Session,
}
fn permission(method: &axum::http::Method, path: &str) -> Option<Permission> {
    use axum::http::Method;
    match (method, path) {
        (
            &Method::GET | &Method::HEAD,
            "/" | "/build-results/{pid}" | "/build-log/{pid}" | "/reviews" | "/reviews/{cid}",
        ) => Some(Permission::View),
        (&Method::POST, "/force-rebuild/{pid}") => Some(Permission::Rebuild),
        (&Method::POST, "/reviews/{cid}") => Some(Permission::Approve),
        (&Method::GET | &Method::HEAD, "/admin/users") | (&Method::POST, "/admin/users/{uid}") => {
            Some(Permission::Admin)
        }
        (&Method::POST, "/logout") => Some(Permission::Session),
        _ => None,
    }
}
pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    for (name, value) in [
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        (
            "content-security-policy",
            "frame-ancestors 'none'; base-uri 'self'; form-action 'self'",
        ),
    ] {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(name),
            value.parse().unwrap(),
        );
    }
    response
}
fn validate_extra_claims(
    claims: &IdTokenClaims<Groups, CoreGenderClaim>,
    client_id: &str,
) -> Result<(), StatusCode> {
    if claims
        .authorized_party()
        .is_some_and(|party| party.as_str() != client_id)
        || (claims.audiences().len() > 1 && claims.authorized_party().is_none())
        || claims.issue_time().timestamp() > now() + 60
        || claims.subject().as_str().is_empty()
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}
pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|c| {
            let (k, v) = c.trim().split_once('=')?;
            (k == name).then(|| v.to_owned())
        })
}
fn set_cookie(name: &str, value: &str, ttl: i64) -> String {
    format!("{name}={value}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={ttl}")
}
fn internal<E: std::fmt::Display>(e: E) -> StatusCode {
    log::error!("Authentication database error: {e}");
    StatusCode::INTERNAL_SERVER_ERROR
}
pub async fn login(State((auth, db)): State<AuthState>) -> Result<Response, StatusCode> {
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let client = auth.current_client().await?;
    let mut authorization = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .set_pkce_challenge(challenge);
    for scope in &auth.scopes {
        authorization = authorization.add_scope(Scope::new(scope.clone()));
    }
    let (url, state, nonce) = authorization.url();
    let browser = id();
    let payload = serde_json::to_string(&Login {
        nonce: nonce.secret().clone(),
        verifier: verifier.secret().clone(),
        browser: browser.clone(),
        context: auth.context(),
    })
    .unwrap();
    db.review_execute(
        "DELETE FROM oidc_login WHERE expires_at<=?",
        vec![now().into()],
    )
    .await
    .map_err(internal)?;
    db.review_execute(
        "INSERT INTO oidc_login (id,payload,expires_at) VALUES (?,?,?)",
        vec![
            state.secret().clone().into(),
            payload.into(),
            (now() + 600).into(),
        ],
    )
    .await
    .map_err(internal)?;
    Ok((
        [(
            header::SET_COOKIE,
            set_cookie("__Host-aur-login", &browser, 600),
        )],
        Redirect::to(url.as_str()),
    )
        .into_response())
}
#[derive(Deserialize)]
pub struct Callback {
    code: String,
    state: String,
}
pub async fn callback(
    State((auth, db)): State<AuthState>,
    headers: HeaderMap,
    Query(q): Query<Callback>,
) -> Result<Response, StatusCode> {
    let mut rows = db
        .review_rows(
            "SELECT payload FROM oidc_login WHERE id=? AND expires_at>?",
            vec![q.state.clone().into(), now().into()],
        )
        .await
        .map_err(internal)?;
    let row = rows.pop().ok_or(StatusCode::BAD_REQUEST)?;
    let login: Login =
        serde_json::from_str(&row.try_get::<String>("", "payload").map_err(internal)?)
            .map_err(internal)?;
    if cookie(&headers, "__Host-aur-login").as_deref() != Some(&login.browser)
        || login.context != auth.context()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if db
        .review_execute(
            "DELETE FROM oidc_login WHERE id=? AND expires_at>?",
            vec![q.state.into(), now().into()],
        )
        .await
        .map_err(internal)?
        != 1
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut client = auth.current_client().await?;
    let token = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        client
            .exchange_code(AuthorizationCode::new(q.code))
            .set_pkce_verifier(PkceCodeVerifier::new(login.verifier))
            .request_async(oidc_http_client),
    )
    .await
    .map_err(|_| StatusCode::GATEWAY_TIMEOUT)?
    .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let id_token = token.id_token().ok_or(StatusCode::UNAUTHORIZED)?;
    let nonce = Nonce::new(login.nonce);
    let needs_refresh = matches!(
        id_token.claims(&client.id_token_verifier(), &nonce),
        Err(ClaimsVerificationError::SignatureVerification(
            SignatureVerificationError::NoMatchingKey | SignatureVerificationError::CryptoError(_)
        ))
    );
    if needs_refresh {
        client = auth.refresh_client().await?;
    }
    let verifier = client.id_token_verifier();
    let claims = id_token
        .claims(&verifier, &nonce)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    // The SDK does not validate azp itself. Reject tokens authorized to another client.
    validate_extra_claims(claims, &auth.client_id)?;
    if let Some(expected) = claims.access_token_hash() {
        let actual = AccessTokenHash::from_token(
            token.access_token(),
            &id_token
                .signing_alg()
                .map_err(|_| StatusCode::UNAUTHORIZED)?,
        )
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
        if actual != *expected {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }
    let admin = claims
        .additional_claims()
        .claims
        .get(&auth.group_claim)
        .and_then(|v| v.as_array())
        .is_some_and(|groups| groups.iter().any(|g| g.as_str() == Some(&auth.admin_group)));
    let name = claims
        .name()
        .and_then(|n| n.get(None))
        .map(|s| s.as_str())
        .unwrap_or(claims.subject().as_str());
    let user = db
        .login_user(&auth.issuer, claims.subject().as_str(), name, admin)
        .await
        .map_err(internal)?;
    if user.disabled {
        return Err(StatusCode::FORBIDDEN);
    }
    if let Some(old) = cookie(&headers, "__Host-aur-session") {
        db.delete_session(&old).await.map_err(internal)?;
    }
    let (sid, _) = db
        .create_bound_session(&user.id, auth.ttl, &auth.context())
        .await
        .map_err(internal)?;
    let mut response = Redirect::to("/").into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        set_cookie("__Host-aur-session", &sid, auth.ttl)
            .parse()
            .unwrap(),
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        set_cookie("__Host-aur-login", "", 0).parse().unwrap(),
    );
    Ok(response)
}
pub async fn protect(
    State((auth, db)): State<AuthState>,
    mut req: Request,
    next: Next,
) -> Response {
    let Some(sid) = cookie(req.headers(), "__Host-aur-session") else {
        return Redirect::to("/auth/login").into_response();
    };
    let (user, csrf) = match db.bound_session(&sid, &auth.context()).await {
        Ok(Some(v)) => v,
        Ok(None) => return Redirect::to("/auth/login").into_response(),
        Err(e) => return internal(e).into_response(),
    };
    if user.disabled {
        return StatusCode::FORBIDDEN.into_response();
    }
    let requirement = req
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .and_then(|p| permission(req.method(), p.as_str()));
    let allowed = match requirement {
        Some(Permission::Session) => true,
        Some(Permission::Admin) => user.admin,
        Some(Permission::View) => user.allows("view"),
        Some(Permission::Rebuild) => user.allows("rebuild"),
        Some(Permission::Approve) => user.allows("approve"),
        None => false,
    };
    if !allowed {
        return (StatusCode::FORBIDDEN,Html(format!("<h1>Access pending</h1><p>An administrator must grant access to this account.</p><form method=post action=/logout><input type=hidden name=csrf value=\"{csrf}\"><button>Sign out</button></form>"))).into_response();
    }
    if req.method() != axum::http::Method::GET && req.method() != axum::http::Method::HEAD {
        if req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            != Some(&auth.origin)
        {
            return StatusCode::FORBIDDEN.into_response();
        }
        // CSRF token lives in forms; also require an exact Origin for every mutation.
    }
    req.extensions_mut().insert(Identity {
        user,
        csrf,
        session_id: sid,
    });
    let mut response = next.run(req).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    response
}
pub fn check_csrf(identity: &Identity, csrf: &str) -> Result<(), StatusCode> {
    if identity.csrf == csrf {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}
pub async fn logout(
    State((_, db)): State<AuthState>,
    axum::Extension(identity): axum::Extension<Identity>,
    axum::Form(form): axum::Form<HashMap<String, String>>,
) -> Result<Response, StatusCode> {
    check_csrf(
        &identity,
        form.get("csrf").map(String::as_str).unwrap_or(""),
    )?;
    db.delete_session(&identity.session_id)
        .await
        .map_err(internal)?;
    Ok((
        [(header::SET_COOKIE, set_cookie("__Host-aur-session", "", 0))],
        Redirect::to("/auth/signed-out"),
    )
        .into_response())
}

pub async fn signed_out() -> Html<&'static str> {
    Html(
        "<!doctype html><html lang=en><meta charset=utf-8><title>Signed out · AUR Builder</title><h1>Signed out</h1><p>Your AUR Builder session has ended. Your identity provider may still be signed in.</p><p><a href=/auth/login>Sign in again</a></p></html>",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::Request,
        routing::{get, post},
    };
    use tower::ServiceExt;
    pub(super) fn auth() -> Arc<Auth> {
        Arc::new(Auth {
            client: OidcClient::new(
                ClientId::new("test".into()),
                Some(ClientSecret::new("secret".into())),
                IssuerUrl::new("https://issuer.example.com".into()).unwrap(),
                AuthUrl::new("https://issuer.example.com/authorize".into()).unwrap(),
                Some(TokenUrl::new("https://issuer.example.com/token".into()).unwrap()),
                None,
                JsonWebKeySet::new(vec![]),
            ),
            client_id: "test".into(),
            client_secret: ClientSecret::new("secret".into()),
            key_cache: Arc::default(),
            issuer: "https://issuer.example.com".into(),
            group_claim: "groups".into(),
            admin_group: "admins".into(),
            origin: "https://builder.example.com".into(),
            ttl: 3600,
            scopes: vec!["profile".into()],
        })
    }
    #[tokio::test]
    async fn routes_require_login_permissions_and_csrf() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let user = db
            .login_user("https://issuer", "user", "User", false)
            .await
            .unwrap();
        let (sid, csrf) = db
            .create_bound_session(&user.id, 3600, &auth().context())
            .await
            .unwrap();
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .route("/reviews/{cid}", post(|| async { "ok" }))
            .route("/admin/users", get(|| async { "ok" }))
            .route_layer(axum::middleware::from_fn_with_state(
                (auth(), db.clone()),
                protect,
            ));
        let response = app
            .clone()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let request = |path: &str, method: &str, origin: &str| {
            let mut req = Request::builder()
                .uri(path)
                .method(method)
                .header(header::COOKIE, format!("__Host-aur-session={sid}"));
            if !origin.is_empty() {
                req = req.header(header::ORIGIN, origin);
            }
            req.body(Body::empty()).unwrap()
        };
        assert_eq!(
            app.clone()
                .oneshot(request("/", "GET", ""))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        // Change grants directly so this test keeps its session; the production management path revokes sessions.
        db.review_execute(
            "UPDATE web_user SET can_view=TRUE,can_approve=TRUE WHERE id=?",
            vec![user.id.clone().into()],
        )
        .await
        .unwrap();
        assert_eq!(
            app.clone()
                .oneshot(request("/", "GET", ""))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            app.clone()
                .oneshot(request("/admin/users", "GET", ""))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            app.clone()
                .oneshot(request("/reviews/test", "POST", "https://evil.example.com"))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            app.clone()
                .oneshot(request(
                    "/reviews/test",
                    "POST",
                    "https://builder.example.com"
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let identity = Identity {
            user,
            csrf: csrf.clone(),
            session_id: sid,
        };
        assert!(check_csrf(&identity, "wrong").is_err());
        assert!(check_csrf(&identity, &csrf).is_ok());
    }
    #[tokio::test]
    async fn login_uses_pkce_nonce_and_browser_bound_state() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let response = login(State((auth(), db.clone()))).await.unwrap();
        let location = response
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        let url = url::Url::parse(location).unwrap();
        let params = url.query_pairs().collect::<HashMap<_, _>>();
        assert_eq!(params["code_challenge_method"], "S256");
        assert!(params.contains_key("nonce"));
        let state = params["state"].to_string();
        let error = callback(
            State((auth(), db)),
            HeaderMap::new(),
            Query(Callback {
                code: "test".into(),
                state,
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(error, StatusCode::BAD_REQUEST);
    }
}

#[cfg(test)]
mod token_tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use serde_json::json;
    pub(super) fn jwt(claims: serde_json::Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let input = format!("{header}.{payload}");
        let key = test_key();
        format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(
                key.sign(
                    &CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
                    input.as_bytes()
                )
                .unwrap()
            )
        )
    }
    pub(super) fn test_key() -> CoreRsaPrivateSigningKey {
        CoreRsaPrivateSigningKey::from_pem(
            include_str!("../tests/fixtures/oidc-test-key.pem"),
            None,
        )
        .unwrap()
    }
    pub(super) fn claims() -> serde_json::Value {
        json!({"iss":"https://issuer.example.com","sub":"user","aud":"test","exp":now()+3600,"iat":now(),"nonce":"nonce-test","groups":["admins"]})
    }
    pub(super) fn auth(token_url: String) -> Auth {
        Auth {
            client: OidcClient::new(
                ClientId::new("test".into()),
                Some(ClientSecret::new("secret".into())),
                IssuerUrl::new("https://issuer.example.com".into()).unwrap(),
                AuthUrl::new("https://issuer.example.com/authorize".into()).unwrap(),
                Some(TokenUrl::new(token_url).unwrap()),
                None,
                JsonWebKeySet::new(vec![test_key().as_verification_key()]),
            ),
            client_id: "test".into(),
            client_secret: ClientSecret::new("secret".into()),
            key_cache: Arc::default(),
            issuer: "https://issuer.example.com".into(),
            group_claim: "groups".into(),
            admin_group: "admins".into(),
            origin: "https://builder.example.com".into(),
            ttl: 3600,
            scopes: vec!["profile".into()],
        }
    }
    #[test]
    fn validates_signed_issuer_audience_nonce_and_expiry() {
        let auth = auth("https://issuer.example.com/token".into());
        type Id = IdToken<
            Groups,
            CoreGenderClaim,
            CoreJweContentEncryptionAlgorithm,
            CoreJwsSigningAlgorithm,
            CoreJsonWebKeyType,
        >;
        let token: Id = serde_json::from_value(json!(jwt(claims()))).unwrap();
        assert!(
            token
                .claims(
                    &auth.client.id_token_verifier(),
                    &Nonce::new("nonce-test".into())
                )
                .is_ok()
        );
        for (key, value) in [
            ("iss", json!("https://evil.example.com")),
            ("aud", json!("another-client")),
            ("nonce", json!("wrong")),
            ("exp", json!(now() - 3600)),
        ] {
            let mut c = claims();
            c[key] = value;
            let token: Id = serde_json::from_value(json!(jwt(c))).unwrap();
            assert!(
                token
                    .claims(
                        &auth.client.id_token_verifier(),
                        &Nonce::new("nonce-test".into())
                    )
                    .is_err(),
                "{key} must be validated"
            );
        }
    }
    #[tokio::test]
    async fn callback_creates_group_admin_and_consumes_state() {
        let token = jwt(claims());
        let app = axum::Router::new().route(
            "/token",
            axum::routing::post(move || {
                let token = token.clone();
                async move {
                    axum::Json(
                        json!({"access_token":"access","token_type":"Bearer","id_token":token}),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let auth = Arc::new(auth(format!("http://{address}/token")));
        let login = Login {
            nonce: "nonce-test".into(),
            verifier: "test-pkce-verifier".into(),
            browser: "test-browser-cookie".into(),
            context: auth.context(),
        };
        db.review_execute(
            "INSERT INTO oidc_login (id,payload,expires_at) VALUES (?,?,?)",
            vec![
                "state".into(),
                serde_json::to_string(&login).unwrap().into(),
                (now() + 600).into(),
            ],
        )
        .await
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "__Host-aur-login=test-browser-cookie".parse().unwrap(),
        );
        let response = callback(
            State((auth.clone(), db.clone())),
            headers.clone(),
            Query(Callback {
                code: "code".into(),
                state: "state".into(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|h| {
                let s = h.to_str().unwrap();
                s.starts_with("__Host-aur-session=").then_some(s)
            })
            .unwrap();
        let session_id = cookie.split(';').next().unwrap().split_once('=').unwrap().1;
        let (user, _) = db.session(session_id).await.unwrap().unwrap();
        assert!(user.admin);
        assert!(user.allows("approve"));
        assert_eq!(
            callback(
                State((auth, db)),
                headers,
                Query(Callback {
                    code: "code".into(),
                    state: "state".into()
                })
            )
            .await
            .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
        task.abort();
    }
}

#[cfg(test)]
mod security_regressions {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::Request,
        routing::{get, post},
    };
    use serde_json::json;
    use tower::ServiceExt;

    #[test]
    fn extra_claims_reject_other_authorized_party_and_future_issue_time() {
        type Id = IdToken<
            Groups,
            CoreGenderClaim,
            CoreJweContentEncryptionAlgorithm,
            CoreJwsSigningAlgorithm,
            CoreJsonWebKeyType,
        >;
        let auth = super::token_tests::auth("https://issuer.example.com/token".into());
        for (field, value) in [
            ("azp", json!("another-client")),
            ("iat", json!(now() + 3600)),
        ] {
            let mut claims = super::token_tests::claims();
            claims[field] = value;
            let token: Id = serde_json::from_value(json!(super::token_tests::jwt(claims))).unwrap();
            let verifier = auth.client.id_token_verifier();
            let claims = token
                .claims(&verifier, &Nonce::new("nonce-test".into()))
                .unwrap();
            assert!(
                validate_extra_claims(claims, "test").is_err(),
                "{field} must be checked by application"
            );
        }
    }
    #[tokio::test]
    async fn configuration_changes_invalidate_sessions_and_login_states() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let original = super::tests::auth();
        let mut changed = (*original).clone();
        changed.admin_group = "different-admins".into();
        let user = db
            .login_user(&original.issuer, "user", "User", true)
            .await
            .unwrap();
        let (sid, _) = db
            .create_bound_session(&user.id, 3600, &original.context())
            .await
            .unwrap();
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .route_layer(axum::middleware::from_fn_with_state(
                (Arc::new(changed.clone()), db.clone()),
                protect,
            ));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::COOKIE, format!("__Host-aur-session={sid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let response = login(State((original.clone(), db.clone()))).await.unwrap();
        let url = url::Url::parse(response.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        let browser_cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, browser_cookie.parse().unwrap());
        assert_eq!(
            callback(
                State((Arc::new(changed), db)),
                headers,
                Query(Callback {
                    code: "code".into(),
                    state
                })
            )
            .await
            .unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }
    #[tokio::test]
    async fn all_mutations_require_csrf_and_unlisted_routes_are_denied() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let auth = super::tests::auth();
        let user = db
            .login_user(&auth.issuer, "admin", "Admin", true)
            .await
            .unwrap();
        let (sid, _) = db
            .create_bound_session(&user.id, 3600, &auth.context())
            .await
            .unwrap();
        let app = Router::new()
            .route("/reviews/{cid}", post(crate::reviews::decide))
            .route("/force-rebuild/{pid}", post(crate::init_force_rebuild))
            .route("/admin/users/{uid}", post(crate::reviews::update_user))
            .route("/unlisted", get(|| async { "should not be accessible" }))
            .route_layer(axum::middleware::from_fn_with_state(
                (auth, db.clone()),
                protect,
            ))
            .layer(axum::Extension(db))
            .layer(axum::Extension(tera::Tera::default()))
            .layer(axum::middleware::from_fn(security_headers));
        for (path, body) in [
            ("/reviews/test", "csrf=wrong&decision=approve"),
            ("/force-rebuild/1", "csrf=wrong"),
            ("/admin/users/user", "csrf=wrong&can_approve=on"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(path)
                        .header(header::COOKIE, format!("__Host-aur-session={sid}"))
                        .header(header::ORIGIN, "https://builder.example.com")
                        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            assert_eq!(response.headers()["x-frame-options"], "DENY");
            assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        }
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/unlisted")
                    .header(header::COOKIE, format!("__Host-aur-session={sid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    #[tokio::test]
    async fn rotated_signing_keys_are_discovered_during_callback() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let mut claims = super::token_tests::claims();
        claims["iss"] = json!(base);
        let token = super::token_tests::jwt(claims);
        let metadata = json!({"issuer":base,"authorization_endpoint":format!("{base}/authorize"),"token_endpoint":format!("{base}/token"),"jwks_uri":format!("{base}/keys"),"response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"]});
        let keys = json!({"keys":[super::token_tests::test_key().as_verification_key()]});
        let app = Router::new()
            .route(
                "/token",
                post(move || {
                    let token = token.clone();
                    async move {
                        axum::Json(
                            json!({"access_token":"access","token_type":"Bearer","id_token":token}),
                        )
                    }
                }),
            )
            .route(
                "/.well-known/openid-configuration",
                get(move || {
                    let metadata = metadata.clone();
                    async move { axum::Json(metadata) }
                }),
            )
            .route(
                "/keys",
                get(move || {
                    let keys = keys.clone();
                    async move { axum::Json(keys) }
                }),
            );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut auth = super::token_tests::auth(format!("{base}/token"));
        auth.issuer = base.clone();
        auth.client = OidcClient::new(
            ClientId::new("test".into()),
            Some(ClientSecret::new("secret".into())),
            IssuerUrl::new(base.clone()).unwrap(),
            AuthUrl::new(format!("{base}/authorize")).unwrap(),
            Some(TokenUrl::new(format!("{base}/token")).unwrap()),
            None,
            JsonWebKeySet::new(vec![]),
        );
        let auth = Arc::new(auth);
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let record = Login {
            nonce: "nonce-test".into(),
            verifier: "verifier".into(),
            browser: "browser".into(),
            context: auth.context(),
        };
        db.review_execute(
            "INSERT INTO oidc_login (id,payload,expires_at) VALUES (?,?,?)",
            vec![
                "state".into(),
                serde_json::to_string(&record).unwrap().into(),
                (now() + 600).into(),
            ],
        )
        .await
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, "__Host-aur-login=browser".parse().unwrap());
        let response = callback(
            State((auth.clone(), db)),
            headers,
            Query(Callback {
                code: "code".into(),
                state: "state".into(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert!(auth.key_cache.lock().await.client.is_some());
        server.abort();
    }
    #[tokio::test]
    async fn logout_revokes_cookie_without_silently_signing_in_again() {
        let db = Database::new("sqlite::memory:".into()).await.unwrap();
        db.migrate().await;
        let auth = super::tests::auth();
        let user = db
            .login_user(&auth.issuer, "user", "User", false)
            .await
            .unwrap();
        let (sid, csrf) = db
            .create_bound_session(&user.id, 3600, &auth.context())
            .await
            .unwrap();
        let identity = Identity {
            user,
            csrf: csrf.clone(),
            session_id: sid.clone(),
        };
        let response = logout(
            State((auth, db.clone())),
            axum::Extension(identity),
            axum::Form(HashMap::from([("csrf".into(), csrf)])),
        )
        .await
        .unwrap();
        assert_eq!(response.headers()[header::LOCATION], "/auth/signed-out");
        assert!(
            response.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
        assert!(db.session(&sid).await.unwrap().is_none());
    }
    #[tokio::test]
    async fn stale_cached_keys_are_not_used_after_refresh_failure() {
        let auth = super::tests::auth();
        {
            let mut cache = auth.key_cache.lock().await;
            cache.client = Some(auth.client.clone());
            cache.loaded_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(301));
            cache.attempted_at = Some(std::time::Instant::now());
        }
        assert!(auth.current_client().await.is_err());
        assert!(!secure_oidc_url(
            &url::Url::parse("http://identity.example.com/keys").unwrap()
        ));
        assert!(!secure_oidc_url(
            &url::Url::parse("https://user:password@identity.example.com/token").unwrap()
        ));
    }
}
