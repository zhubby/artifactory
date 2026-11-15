use crate::{
    crypto,
    error::{ApiError, Result},
    state::AppState,
};
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Json,
    extract::{FromRequestParts, MatchedPath, RawPathParams, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header, request::Parts},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

/// Global account identity. Business roles are resolved from tenant membership.
#[derive(Clone, Debug)]
pub struct AccountAuth {
    pub user_id: Uuid,
    pub username: String,
}

#[derive(Clone, Debug)]
pub struct Auth {
    pub user_id: Uuid,
    pub username: String,
    pub tenant_id: Uuid,
    /// Effective tenant role, never users.role.
    pub role: String,
    pub project_roles: std::collections::BTreeMap<Uuid, String>,
    pub token_projects: Option<Vec<Uuid>>,
    pub token_scopes: Vec<String>,
    pub(crate) credential_hash: String,
}

impl Auth {
    pub fn scoped(user_id: Uuid, role: &str, projects: Vec<Uuid>, scopes: Vec<String>) -> Self {
        Self {
            user_id,
            username: String::new(),
            tenant_id: Uuid::nil(),
            role: "member".into(),
            project_roles: projects.iter().map(|p| (*p, role.into())).collect(),
            token_projects: Some(projects),
            token_scopes: scopes,
            credential_hash: String::new(),
        }
    }

    pub fn tenant_admin(&self) -> bool {
        matches!(self.role.as_str(), "owner" | "admin")
    }

    pub fn session_role(&self, role: &str) -> Result<()> {
        if self.token_projects.is_some() || (role != "viewer" && !self.tenant_admin()) {
            return Err(ApiError::Forbidden);
        }
        Ok(())
    }

    pub fn project(&self, project: Uuid, operation: &str) -> Result<()> {
        let required = match operation {
            "read" => 0,
            "admin" => 2,
            "build" | "release" => 1,
            _ => return Err(ApiError::Forbidden),
        };
        if self
            .project_roles
            .get(&project)
            .is_none_or(|role| rank(role) < required)
        {
            return Err(ApiError::Forbidden);
        }
        if let Some(projects) = &self.token_projects
            && (!projects.contains(&project) || !self.token_scopes.iter().any(|s| s == operation))
        {
            return Err(ApiError::Forbidden);
        }
        Ok(())
    }

    pub fn visible_projects(&self) -> Option<Vec<Uuid>> {
        Some(
            self.project_roles
                .keys()
                .copied()
                .filter(|id| {
                    self.token_projects
                        .as_ref()
                        .is_none_or(|ids| ids.contains(id))
                })
                .collect(),
        )
    }

    pub async fn refresh(&self, state: &AppState) -> Result<Self> {
        let mut current = if self.token_projects.is_some() {
            state
                .store
                .authenticate_token(&self.credential_hash)
                .await?
                .ok_or(ApiError::Unauthorized)?
        } else {
            let account = state
                .store
                .authenticate_session(&self.credential_hash)
                .await?
                .ok_or(ApiError::Unauthorized)?;
            Self {
                user_id: account.user_id,
                username: account.username,
                ..self.clone()
            }
        };
        state
            .store
            .authorize_tenant(&mut current, self.tenant_id)
            .await?;
        Ok(current)
    }
}

fn rank(role: &str) -> i32 {
    match role {
        "admin" => 2,
        "operator" => 1,
        "viewer" => 0,
        _ => -1,
    }
}

impl FromRequestParts<AppState> for AccountAuth {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        if parts.headers.contains_key(header::AUTHORIZATION) {
            return Err(ApiError::Forbidden);
        }
        if ![Method::GET, Method::HEAD, Method::OPTIONS].contains(&parts.method) {
            csrf(&parts.headers, state)?;
        }
        let token = cookie(&parts.headers).ok_or(ApiError::Unauthorized)?;
        state
            .store
            .authenticate_session(&crypto::hash(token))
            .await?
            .ok_or(ApiError::Unauthorized)
    }
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        if let Some(auth) = parts.extensions.get::<Auth>() {
            return Ok(auth.clone());
        }
        let requested = parts
            .headers
            .get("x-artifactory-tenant")
            .map(|h| {
                h.to_str()
                    .ok()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .ok_or_else(|| ApiError::BadRequest("invalid tenant context".into()))
            })
            .transpose()?;
        let mut auth = if let Some(value) = parts.headers.get(header::AUTHORIZATION) {
            let value = value.to_str().map_err(|_| ApiError::Unauthorized)?;
            let token = value
                .strip_prefix("Bearer ")
                .ok_or(ApiError::Unauthorized)?;
            let mut auth = state
                .store
                .authenticate_token(&crypto::hash(token))
                .await?
                .ok_or(ApiError::Unauthorized)?;
            auth.credential_hash = crypto::hash(token);
            if requested.is_some_and(|id| id != auth.tenant_id) {
                return Err(ApiError::Forbidden);
            }
            auth
        } else {
            let account = AccountAuth::from_request_parts(parts, state).await?;
            Auth {
                user_id: account.user_id,
                username: account.username,
                tenant_id: requested.ok_or(ApiError::TenantRequired)?,
                role: String::new(),
                project_roles: Default::default(),
                token_projects: None,
                token_scopes: vec![],
                credential_hash: crypto::hash(
                    cookie(&parts.headers).ok_or(ApiError::Unauthorized)?,
                ),
            }
        };
        let tenant = auth.tenant_id;
        state.store.authorize_tenant(&mut auth, tenant).await?;
        parts.extensions.insert(auth.clone());
        Ok(auth)
    }
}

/// Request-local state with an explicit, validated tenant and project scope.
pub struct TenantState(pub AppState);
impl FromRequestParts<AppState> for TenantState {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        let auth = Auth::from_request_parts(parts, state).await?;
        let matched = MatchedPath::from_request_parts(parts, state)
            .await
            .map_err(|error| ApiError::Internal(error.into()))?;
        let route = matched
            .as_str()
            .strip_prefix("/api/v1/")
            .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("unexpected tenant route")))?;
        let mut segments = route.split('/');
        let mut resource = None;
        if let (Some(kind), Some("{id}")) = (segments.next(), segments.next()) {
            // Use Axum's decoded parameters, exactly as the handler's Path extractor does.
            let parameters = RawPathParams::from_request_parts(parts, state)
                .await
                .map_err(|_| ApiError::BadRequest("invalid resource ID".into()))?;
            let id = parameters
                .iter()
                .find_map(|(key, value)| (key == "id").then_some(value))
                .and_then(|value| Uuid::parse_str(value).ok())
                .ok_or_else(|| ApiError::BadRequest("invalid resource ID".into()))?;
            state.store.authorize_resource(kind, id, &auth).await?;
            resource = Some((kind, id));
        }
        if ![Method::GET, Method::HEAD, Method::OPTIONS].contains(&parts.method)
            && !["/cancel", "/lifecycle", "/clarifications"]
                .iter()
                .any(|suffix| route.ends_with(suffix))
        {
            state
                .store
                .ensure_writable(auth.tenant_id, resource.map(|r| r.0), resource.map(|r| r.1))
                .await?;
        }
        let mut scoped = state.clone();
        scoped.store = state.store.for_auth(&auth);
        Ok(Self(scoped))
    }
}

pub fn csrf(headers: &HeaderMap, state: &AppState) -> Result<()> {
    if headers
        .get("x-artifactory-csrf")
        .and_then(|value| value.to_str().ok())
        != Some("1")
    {
        return Err(ApiError::Forbidden);
    }
    if let Some(origin) = headers.get(header::ORIGIN)
        && origin.to_str().ok() != Some(state.settings.public_url.as_str())
    {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}

fn cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|entry| entry.trim().strip_prefix("artifactory_session="))
}

pub fn password_hash(password: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        password.len() >= 12 && password.len() <= 256,
        "password must contain 12..256 characters"
    );
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map_err(|_| anyhow::anyhow!("password hashing failed"))?
        .to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Login {
    username: String,
    password: String,
}

pub async fn registration_config(State(state): State<AppState>) -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({"registration_enabled":state.settings.registration_enabled})),
    )
}

pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    input: std::result::Result<Json<Login>, JsonRejection>,
) -> Result<impl IntoResponse> {
    csrf(&headers, &state)?;
    if !state.settings.registration_enabled {
        return Err(ApiError::Forbidden);
    }
    let Json(input) =
        input.map_err(|_| ApiError::BadRequest("invalid registration fields".into()))?;
    let username = input.username.trim();
    if username.contains('\0') {
        return Err(ApiError::BadRequest("invalid registration fields".into()));
    }
    if username.is_empty() || username.len() > 100 {
        return Err(ApiError::BadRequest(
            "username must contain 1..100 bytes".into(),
        ));
    }
    if !(12..=256).contains(&input.password.len()) {
        return Err(ApiError::BadRequest(
            "password must contain 12..256 bytes".into(),
        ));
    }
    state.store.reserve_registration().await?;
    let password = input.password;
    let encoded = tokio::task::spawn_blocking(move || password_hash(&password))
        .await
        .map_err(anyhow::Error::from)??;
    let user_id = Uuid::new_v4();
    let token = crypto::token();
    state
        .store
        .register_viewer(user_id, username, &encoded, &crypto::hash(&token))
        .await?;
    Ok((
        StatusCode::CREATED,
        [(
            header::SET_COOKIE,
            session_cookie(&token, state.settings.insecure_dev),
        )],
        Json(state.store.account_profile(user_id).await?),
    ))
}

fn session_cookie(token: &str, insecure_dev: bool) -> HeaderValue {
    let secure = if insecure_dev { "" } else { "; Secure" };
    HeaderValue::from_str(&format!(
        "artifactory_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=86400{secure}"
    ))
    .unwrap()
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Login>,
) -> Result<impl IntoResponse> {
    csrf(&headers, &state)?;
    if input.username.len() > 100 || input.password.len() > 256 {
        return Err(ApiError::Unauthorized);
    }
    let recent = state.store.recent_login_failures(&input.username).await?;
    if recent >= 10 {
        return Err(ApiError::Unauthorized);
    }
    let row = state.store.login_account(&input.username).await?;
    let valid = if let Some(row) = &row {
        let encoded = row.password_hash.clone();
        let password = input.password.clone();
        tokio::task::spawn_blocking(move || {
            PasswordHash::new(&encoded).is_ok_and(|encoded| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &encoded)
                    .is_ok()
            })
        })
        .await
        .unwrap_or(false)
    } else {
        false
    };
    if !valid {
        state
            .audit("anonymous", "login_failed", &input.username, json!({}))
            .await?;
        return Err(ApiError::Unauthorized);
    }
    let row = row.unwrap();
    let user_id = row.id;
    let token = crypto::token();
    state
        .store
        .create_verified_session(&crypto::hash(&token), user_id, &row.password_hash)
        .await?;
    state.audit(user_id, "login", user_id, json!({})).await?;
    Ok((
        [(
            header::SET_COOKIE,
            session_cookie(&token, state.settings.insecure_dev),
        )],
        Json(state.store.account_profile(user_id).await?),
    ))
}

pub async fn logout(
    State(state): State<AppState>,
    _auth: AccountAuth,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    if let Some(token) = cookie(&headers) {
        state.store.remove_session(&crypto::hash(token)).await?;
    }
    Ok((
        [(
            header::SET_COOKIE,
            "artifactory_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        )],
        Json(json!({"ok":true})),
    ))
}

pub async fn me(State(state): State<AppState>, auth: AccountAuth) -> Result<Json<Value>> {
    Ok(Json(state.store.account_profile(auth.user_id).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileInput {
    display_name: String,
}

pub async fn update_profile(
    State(state): State<AppState>,
    auth: AccountAuth,
    input: std::result::Result<Json<ProfileInput>, JsonRejection>,
) -> Result<Json<Value>> {
    let Json(input) = input.map_err(|_| ApiError::BadRequest("invalid profile fields".into()))?;
    let name = input.display_name.trim();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err(ApiError::BadRequest(
            "display name must contain 1..100 visible characters".into(),
        ));
    }
    Ok(Json(
        state
            .store
            .update_account_profile(auth.user_id, name)
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordInput {
    current_password: String,
    new_password: String,
}

/// Verify account credentials for a password change or destructive lifecycle request.
pub async fn verify_current_password(
    state: &AppState,
    user: Uuid,
    username: &str,
    password: String,
) -> Result<String> {
    state.store.reserve_security_attempt(user).await?;
    if password.len() > 256 {
        return Err(ApiError::BadRequest("current password is incorrect".into()));
    }
    let account = state
        .store
        .login_account(username)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    let encoded = account.password_hash;
    let verified = encoded.clone();
    let valid = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&encoded).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .map_err(anyhow::Error::from)?;
    if !valid {
        return Err(ApiError::BadRequest("current password is incorrect".into()));
    }
    Ok(verified)
}

pub async fn change_password(
    State(state): State<AppState>,
    auth: AccountAuth,
    Json(input): Json<PasswordInput>,
) -> Result<impl IntoResponse> {
    if !(12..=256).contains(&input.new_password.len()) {
        return Err(ApiError::BadRequest(
            "password must contain 12..256 bytes".into(),
        ));
    }
    let verified =
        verify_current_password(&state, auth.user_id, &auth.username, input.current_password)
            .await?;
    let replacement = tokio::task::spawn_blocking(move || password_hash(&input.new_password))
        .await
        .map_err(anyhow::Error::from)??;
    state
        .store
        .replace_password(auth.user_id, &verified, &replacement)
        .await?;
    Ok((
        [(
            header::SET_COOKIE,
            "artifactory_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        )],
        Json(json!({"ok":true})),
    ))
}
