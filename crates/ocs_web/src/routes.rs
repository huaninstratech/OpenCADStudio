//! The HTTP surface.
//!
//! | Address | What it does |
//! |---|---|
//! | `GET /auth/login` | starts a sign-in and sends the browser to the provider |
//! | `GET /auth/callback` | finishes it: the code becomes tokens, tokens become a cookie |
//! | `POST /auth/logout` | ends the session here |
//! | `GET /auth/me` | who the cookie belongs to, for the interface |
//! | `GET /drawings` | the drawings the signed-in user may open |
//! | `GET /drawings/{id}` | one drawing's bytes — the `?file=` address |
//! | `GET /healthz` | whether the server is up |
//! | everything else | the web bundle, `index.html` for a directory |
//!
//! Two rules hold the auth together:
//! - the cookie is `HttpOnly` (no script reads it), `SameSite=Lax` (a foreign
//!   page cannot make the browser bring it to a state-changing request), and
//!   `Secure` unless the operator turned that off for a loopback test;
//! - the one request that changes something is a POST checked against the
//!   `Origin` header, so a page elsewhere cannot sign the user out.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;

use crate::assets::{Bundle, File};
use crate::drawings::{AdminPolicy, OpenError, Source};
use crate::oidc::{self, Provider};
use crate::session::{Sessions, User};
use crate::App;

/// The cookie that carries a session.
pub const SESSION_COOKIE: &str = "ocs_session";

/// The router for one server.
pub fn router(app: App) -> Router {
    let App {
        config,
        provider,
        sessions,
        drawings,
    } = app;
    let shared = Arc::new(Shared {
        provider,
        sessions,
        drawings,
        bundle: Arc::new(Bundle::load(config.static_dir.as_deref())),
        admin: Arc::new(AdminPolicy::new(&config.admin_claims)),
        cookie_secure: config.cookie_secure,
    });
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .route("/drawings", get(list_drawings))
        .route("/drawings/{*id}", get(open_drawing))
        .route("/healthz", get(health))
        .fallback(serve_bundle)
        .with_state(shared)
}

/// Everything a handler reads.
struct Shared {
    provider: Arc<Provider>,
    sessions: Arc<Sessions>,
    drawings: Arc<Source>,
    bundle: Arc<Bundle>,
    admin: Arc<AdminPolicy>,
    cookie_secure: bool,
}

impl Shared {
    /// The user behind the request's cookie, if the cookie is one of ours.
    fn user(&self, headers: &HeaderMap) -> Option<User> {
        self.sessions.user(cookie_value(headers).as_deref())
    }

    /// The `Set-Cookie` value that stores (or with `None`, clears) the session.
    fn session_cookie(&self, value: Option<&str>) -> String {
        let mut cookie = match value {
            Some(value) => format!("{SESSION_COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax"),
            None => format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"),
        };
        if self.cookie_secure {
            cookie.push_str("; Secure");
        }
        cookie
    }
}

/// The `ocs_session` value of a request, when there is one.
pub fn cookie_value(headers: &HeaderMap) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name.trim() == SESSION_COOKIE).then(|| value.trim().to_string())
    })
}

/// The response for a refusal: a status and one plain line. It never says
/// which part of the request was wrong.
fn refuse(status: StatusCode, message: &str) -> Response {
    (status, format!("{message}\n")).into_response()
}

/// `GET /auth/login`: starts a sign-in, then leaves for the provider.
async fn login(State(shared): State<Arc<Shared>>) -> Response {
    let verifier = oidc::new_verifier();
    let state = oidc::new_state();
    shared.sessions.start(&state, &verifier);
    shared.sessions.sweep_pending();
    let address = oidc::authorization_url(
        shared.provider.discovery(),
        shared.provider.client_id(),
        shared.provider.redirect_uri(),
        shared.provider.scopes(),
        &state,
        &oidc::challenge_for(&verifier),
    );
    Redirect::to(&address).into_response()
}

/// What the provider appends to the callback.
#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    /// Set instead of `code` when the provider refused (`access_denied`).
    error: Option<String>,
    error_description: Option<String>,
}

/// `GET /auth/callback`: checks the state, trades the code for tokens, and
/// turns the claims into a session.
async fn callback(
    State(shared): State<Arc<Shared>>,
    Query(query): Query<CallbackQuery>,
    headers: HeaderMap,
) -> Response {
    if let Some(provider_error) = query.error {
        let detail = query.error_description.unwrap_or_default();
        return refuse(
            StatusCode::UNAUTHORIZED,
            format!("the provider refused the sign-in: {provider_error} {detail}").trim_end(),
        );
    }
    let (Some(code), Some(state)) = (query.code, query.state) else {
        return refuse(StatusCode::BAD_REQUEST, "the callback carried no code or no state");
    };
    let Some(verifier) = shared.sessions.take_pending(&state) else {
        return refuse(
            StatusCode::BAD_REQUEST,
            "this sign-in was not started here, has already been spent once, or took longer than ten minutes",
        );
    };
    let tokens = match shared.provider.exchange_code(&code, &verifier).await {
        Ok(tokens) => tokens,
        Err(message) => {
            // The message names what the provider objected to and holds no
            // secret of ours; the operator needs it to fix a deployment.
            return refuse(StatusCode::BAD_GATEWAY, &format!("sign-in failed: {message}"));
        }
    };
    let claims = match shared.provider.claims(&tokens) {
        Ok(claims) => claims,
        Err(message) => return refuse(StatusCode::BAD_GATEWAY, &format!("sign-in failed: {message}")),
    };
    if !claims.is_acceptable() {
        return refuse(
            StatusCode::FORBIDDEN,
            "the provider did not vouch for this account: no subject, or an unverified address",
        );
    }
    let user = User {
        subject: claims.sub.clone(),
        email: claims.email.clone().unwrap_or_default(),
        name: claims.display_name(),
        roles: claims.roles(),
        access_token: tokens.access_token,
    };
    let cookie = shared.sessions.sign_in(user);
    // A new sign-in replaces whatever session the browser had.
    let previous = cookie_value(&headers);
    shared.sessions.sign_out(previous.as_deref());
    let mut response = Redirect::to("/").into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&shared.session_cookie(Some(&cookie)))
            .unwrap_or(HeaderValue::from_static("")),
    );
    response
}

/// `POST /auth/logout`: forgets the session here. The provider's own session
/// is left alone on purpose: signing out of one drawing viewer should not
/// sign the user out of everything else in the suite.
async fn logout(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    if !same_origin(&headers) {
        return refuse(StatusCode::FORBIDDEN, "a cross-site request cannot sign anyone out");
    }
    let cookie = cookie_value(&headers);
    shared.sessions.sign_out(cookie.as_deref());
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&shared.session_cookie(None)).unwrap_or(HeaderValue::from_static("")),
    );
    response
}

/// Who the cookie belongs to, as the interface shows it.
async fn me(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    let Some(user) = shared.user(&headers) else {
        return (StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({ "signed_in": false })))
            .into_response();
    };
    axum::Json(serde_json::json!({
        "signed_in": true,
        "subject": user.subject,
        "email": user.email,
        "name": user.name,
        "roles": user.roles,
        "admin": shared.admin.allows(&user),
    }))
    .into_response()
}

/// `GET /drawings`: what the signed-in user may open.
async fn list_drawings(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    let Some(user) = shared.user(&headers) else {
        return refuse(StatusCode::UNAUTHORIZED, "sign in first");
    };
    let drawings: Vec<_> = shared
        .drawings
        .list(&user, &shared.admin).await
        .into_iter()
        .map(|drawing| {
            serde_json::json!({
                "id": drawing.id,
                "name": drawing.name,
                "collection": drawing.collection,
                "bytes": drawing.bytes,
            })
        })
        .collect();
    axum::Json(serde_json::json!({ "drawings": drawings })).into_response()
}

/// `GET /drawings/{id}`: one drawing's bytes. This is the address a `?file=`
/// link names, and the session cookie travels with it because the address is
/// on this server.
async fn open_drawing(
    State(shared): State<Arc<Shared>>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    let Some(user) = shared.user(&headers) else {
        return refuse(StatusCode::UNAUTHORIZED, "sign in first");
    };
    match shared
        .drawings
        .open(&user, &shared.admin, id.trim_start_matches('/'))
        .await
    {
        Ok(bytes) => {
            let mut response = Response::new(Body::from(bytes));
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            );
            // Not cached: a drawing changes, and a cached copy would outlive
            // the session that was allowed to see it.
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            );
            response
        }
        Err(OpenError::NotFound) => refuse(StatusCode::NOT_FOUND, "no such drawing"),
        Err(OpenError::Unreadable(message)) => {
            refuse(StatusCode::INTERNAL_SERVER_ERROR, &message)
        }
    }
}

/// `GET /healthz`: the one address that says nothing about anyone.
async fn health(State(shared): State<Arc<Shared>>) -> Response {
    axum::Json(serde_json::json!({
        "status": "ok",
        "sessions": shared.sessions.open_sessions(),
        "bundle_files": shared.bundle.len(),
    }))
    .into_response()
}

/// Everything else: the web bundle. A path that names no file falls back to
/// `index.html`, since the interface loads from one page.
async fn serve_bundle(State(shared): State<Arc<Shared>>, uri: Uri, method: Method) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return refuse(StatusCode::METHOD_NOT_ALLOWED, "only GET is served here");
    }
    let path = uri.path().trim_start_matches('/');
    let file = shared.bundle.get(path).or_else(|| {
        (path.is_empty() || path.ends_with('/'))
            .then(|| shared.bundle.index())
            .flatten()
    });
    let Some(file) = file else {
        return refuse(StatusCode::NOT_FOUND, "not found");
    };
    bundle_response(file)
}

/// A bundle file with the caching its name earned.
fn bundle_response(file: &File) -> Response {
    let mut response = Response::new(Body::from(file.bytes.clone()));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(file.content_type),
    );
    let cache = if file.immutable {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    response
}

/// Whether the request came from this site. Without a browser-supplied
/// `Origin` — a same-origin GET, a command-line client — there is nothing to
/// compare, and the cookie's `SameSite` rule already held.
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|value| value.to_str().ok()) else {
        return true;
    };
    let Some(host) = headers.get(header::HOST).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let Some((scheme, rest)) = origin.split_once("://") else {
        return false;
    };
    if scheme != "http" && scheme != "https" {
        return false;
    }
    rest.split('/').next().unwrap_or_default() == host
}

#[cfg(test)]
mod tests {
    //! The cookie and origin rules, without a provider.
    use super::{cookie_value, same_origin, SESSION_COOKIE};
    use axum::http::{HeaderMap, HeaderValue};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(
                axum::http::HeaderName::from_bytes(name.as_bytes()).expect("a header name"),
                HeaderValue::from_str(value).expect("a header value"),
            );
        }
        map
    }

    #[test]
    fn the_session_cookie_is_read_out_of_a_cookie_header() {
        let map = headers(&[("cookie", &format!("other=1; {SESSION_COOKIE}=abc123; last=2"))]);
        assert_eq!(cookie_value(&map).as_deref(), Some("abc123"));
        assert_eq!(cookie_value(&headers(&[("cookie", "other=1")])), None);
        assert_eq!(cookie_value(&headers(&[])), None);
    }

    #[test]
    fn a_request_from_this_site_passes_and_one_from_elsewhere_does_not() {
        assert!(same_origin(&headers(&[("host", "ssdnode.example")])));
        assert!(same_origin(&headers(&[
            ("host", "ssdnode.example"),
            ("origin", "https://ssdnode.example"),
        ])));
        assert!(same_origin(&headers(&[
            ("host", "ssdnode.example:8443"),
            ("origin", "https://ssdnode.example:8443"),
        ])));
        assert!(!same_origin(&headers(&[
            ("host", "ssdnode.example"),
            ("origin", "https://evil.example"),
        ])));
        assert!(!same_origin(&headers(&[("host", "a.example"), ("origin", "file://x")])));
    }
}
