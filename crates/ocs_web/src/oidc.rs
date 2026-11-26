//! The provider side: discovery, PKCE, and the code exchange.
//!
//! One authorization per sign-in follows the provider's discovery document:
//! a PKCE verifier and a state are made here, the browser is sent to
//! `authorization_endpoint`, and the code it brings back is exchanged at
//! `token_endpoint` with the verifier and, for a confidential client, the
//! secret. Nothing of the exchange reaches the browser.
//!
//! The parts that can be got wrong are pure functions — verifier and
//! challenge, the `state` check, reading the discovery document and the
//! token response — and are tested here; only the two HTTP calls need a
//! provider.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::time::{Duration, Instant};

/// What the discovery document says where it matters here.
#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub userinfo_endpoint: Option<String>,
    #[serde(default)]
    pub end_session_endpoint: Option<String>,
    /// The provider suggesting `client_secret_post` when it answers this way.
    #[serde(default)]
    pub token_endpoint_auth_methods_supported: Vec<String>,
    #[serde(default)]
    pub code_challenge_methods_supported: Vec<String>,
}

impl Discovery {
    /// The document for `issuer`, fetched once.
    pub async fn fetch(client: &reqwest::Client, issuer: &str) -> Result<Self, String> {
        let url = format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/'));
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|error| format!("cannot reach the provider at {url}: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("{url} answered {}", response.status()));
        }
        let document: Self = response
            .json()
            .await
            .map_err(|error| format!("{url} is not a discovery document: {error}"))?;
        document.check(issuer)?;
        Ok(document)
    }

    /// The rules a document has to satisfy before anything trusts it.
    fn check(&self, issuer: &str) -> Result<(), String> {
        let wanted = issuer.trim_end_matches('/');
        if self.issuer.trim_end_matches('/') != wanted {
            return Err(format!(
                "the discovery document belongs to {} — not to {wanted}",
                self.issuer
            ));
        }
        if !self.authorization_endpoint.starts_with("https://") {
            return Err("the authorization endpoint is not https".to_string());
        }
        if !self.token_endpoint.starts_with("https://") {
            return Err("the token endpoint is not https".to_string());
        }
        if !self.code_challenge_methods_supported.is_empty()
            && !self
                .code_challenge_methods_supported
                .iter()
                .any(|method| method.eq_ignore_ascii_case("S256"))
        {
            return Err(
                "the provider does not offer PKCE with S256, so a browser-held code could be \
                 replayed"
                    .to_string(),
            );
        }
        Ok(())
    }

    /// Whether to authenticate with a secret, and how. `None` for a public
    /// client, which then has to send PKCE alone.
    pub fn auth_method(&self, has_secret: bool) -> AuthMethod {
        if !has_secret {
            return AuthMethod::None;
        }
        // `client_secret_basic` is the safer name for the same thing, but the
        // provider says which it reads; default to the body when it says
        // nothing, since that is what a plain form post does.
        if self
            .token_endpoint_auth_methods_supported
            .iter()
            .any(|method| method == "client_secret_basic")
            && !self
                .token_endpoint_auth_methods_supported
                .iter()
                .any(|method| method == "client_secret_post")
        {
            AuthMethod::Basic
        } else {
            AuthMethod::Post
        }
    }
}

/// How the client proves itself at the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    /// A public client: PKCE only.
    None,
    /// `client_id` and `client_secret` in the form body.
    Post,
    /// `Authorization: Basic base64(client_id:client_secret)`.
    Basic,
}

/// A search parameter value, escaped for a URL query.
pub fn escape(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// A new PKCE verifier: 32 random bytes, base64url without padding, which is
/// inside the 43–128 characters RFC 7636 allows.
pub fn new_verifier() -> String {
    let mut bytes = [0u8; 32];
    fill_random(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// The `code_challenge` for a verifier: `BASE64URL(SHA256(ASCII(verifier)))`.
pub fn challenge_for(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// A new `state`, which binds the callback to the sign-in the browser left.
pub fn new_state() -> String {
    let mut bytes = [0u8; 24];
    fill_random(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn fill_random(bytes: &mut [u8]) {
    use rand::RngCore as _;
    rand::thread_rng().fill_bytes(bytes);
}

/// The address the browser is sent to for one sign-in.
pub fn authorization_url(
    discovery: &Discovery,
    client_id: &str,
    redirect_uri: &str,
    scopes: &[String],
    state: &str,
    code_challenge: &str,
) -> String {
    let scope = scopes.join(" ");
    format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}\
         &code_challenge={}&code_challenge_method=S256",
        discovery.authorization_endpoint,
        escape(client_id),
        escape(redirect_uri),
        escape(&scope),
        escape(state),
        escape(code_challenge),
    )
}

/// Whether the callback's `state` is the one this sign-in was sent with.
/// Compared in constant time: the value is a secret the provider echoes.
pub fn state_matches(expected: &str, returned: Option<&str>) -> bool {
    let Some(returned) = returned else {
        return false;
    };
    if expected.len() != returned.len() {
        return false;
    }
    expected
        .bytes()
        .zip(returned.bytes())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

/// What the token endpoint answered, as far as this server uses it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
}

/// The claims this server trusts from the `id_token`, once it came back over
/// TLS straight from the token endpoint. The signature is not checked again:
/// the document came from the issuer itself, which is the same reason the
/// code exchange needed no nonce — nothing else could have written it.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Claims {
    #[serde(default)]
    pub sub: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub email_verified: Option<bool>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub preferred_username: Option<String>,
    /// The provider's custom claim: one role string or a list of them.
    #[serde(default)]
    pub role: RoleClaim,
    /// This provider also answers with `roles`; the app reads both shapes.
    #[serde(default)]
    pub roles: RoleClaim,
}

/// A claim that is either one string or a list of them.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RoleClaim {
    One(String),
    Many(Vec<String>),
    #[default]
    Absent,
}

impl RoleClaim {
    pub fn list(&self) -> Vec<String> {
        match self {
            Self::One(role) => vec![role.clone()],
            Self::Many(roles) => roles.clone(),
            Self::Absent => Vec::new(),
        }
    }
}

impl Claims {
    /// The roles a user holds, from either claim.
    pub fn roles(&self) -> Vec<String> {
        let mut roles = self.role.list();
        roles.extend(self.roles.list());
        roles.sort();
        roles.dedup();
        roles
    }

    /// The name to show; the address when the provider has no name.
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .or_else(|| self.preferred_username.clone())
            .or_else(|| self.email.clone())
            .unwrap_or_else(|| self.sub.clone())
    }

    /// Whether the user may be signed in: the provider has to say the
    /// address is verified when it says anything at all about it.
    pub fn is_acceptable(&self) -> bool {
        !self.sub.is_empty() && self.email_verified.unwrap_or(true)
    }

    /// The payload of a JWT, decoded without trusting it. Only called on a
    /// token that arrived from the token endpoint over TLS.
    pub fn from_jwt_payload(token: &str) -> Result<Self, String> {
        let mut parts = token.split('.');
        let (_header, payload) = (parts.next(), parts.next());
        let payload = payload.ok_or_else(|| "not a JWT: no payload".to_string())?;
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|error| format!("the token payload is not base64url: {error}"))?;
        serde_json::from_slice(&decoded)
            .map_err(|error| format!("the token payload is not JSON: {error}"))
    }
}

/// Exchanges an authorization code for tokens.
pub struct Provider {
    pub discovery: Discovery,
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    scopes: Vec<String>,
    auth_method: AuthMethod,
    client: reqwest::Client,
}

impl Provider {
    /// Finds the provider's endpoints. `redirect_uri` is only kept to send
    /// with the exchange; it has already been registered by the operator.
    pub async fn discover(
        client: reqwest::Client,
        issuer: &str,
        client_id: &str,
        client_secret: &str,
        redirect_uri: &str,
        scopes: &[String],
    ) -> Result<Self, String> {
        let discovery = Discovery::fetch(&client, issuer).await?;
        let auth_method = discovery.auth_method(!client_secret.is_empty());
        Ok(Self {
            discovery,
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            redirect_uri: redirect_uri.to_string(),
            scopes: scopes.to_vec(),
            auth_method,
            client,
        })
    }

    /// The client this server is, and what it asks for: the routes build the
    /// authorization address from these.
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    /// The provider's addresses, for the routes.
    pub fn discovery(&self) -> &Discovery {
        &self.discovery
    }

    /// Turns a code into tokens. The verifier is the one the sign-in was
    /// started with; the provider checks it against the challenge.
    pub async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
    ) -> Result<TokenResponse, String> {
        let form = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("code_verifier", verifier),
            ("client_id", self.client_id.as_str()),
        ];
        let mut request = self.client.post(&self.discovery.token_endpoint);
        request = match self.auth_method {
            AuthMethod::None => request.form(&form),
            AuthMethod::Post => request.form(
                &form
                    .into_iter()
                    .chain([("client_secret", self.client_secret.as_str())])
                    .collect::<Vec<_>>(),
            ),
            AuthMethod::Basic => request
                .basic_auth(&self.client_id, Some(&self.client_secret))
                .form(&form.into_iter().filter(|(key, _)| *key != "client_id").collect::<Vec<_>>()),
        };
        let response = request
            .send()
            .await
            .map_err(|error| format!("the token endpoint is unreachable: {error}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| format!("the token endpoint's answer could not be read: {error}"))?;
        if !status.is_success() {
            // The provider's message names the cause (bad verifier, wrong
            // redirect, expired code) and carries no secret of ours.
            return Err(format!("the token endpoint answered {status}: {body}"));
        }
        serde_json::from_str(&body)
            .map_err(|error| format!("the token endpoint's answer is not a token: {error}"))
    }

    /// The claims of the signed-in user: the `id_token` when the provider
    /// sent one, otherwise the access token's payload.
    pub fn claims(&self, tokens: &TokenResponse) -> Result<Claims, String> {
        let jwt = tokens
            .id_token
            .as_deref()
            .unwrap_or(&tokens.access_token);
        Claims::from_jwt_payload(jwt)
    }

    /// The address that ends the provider's session too, when it offers one.
    pub fn logout_url(&self) -> Option<String> {
        self.discovery.end_session_endpoint.clone()
    }
}

/// How long a sign-in may sit half-finished before its verifier is dropped.
pub const PENDING_LIFETIME: Duration = Duration::from_secs(10 * 60);

/// A sign-in that has left for the provider and not come back yet.
#[derive(Debug, Clone)]
pub struct Pending {
    pub verifier: String,
    pub started: Instant,
}

#[cfg(test)]
mod tests {
    //! The parts of the exchange that can be got wrong without a provider.
    use super::*;

    fn discovery() -> Discovery {
        Discovery {
            issuer: "https://amecc.spm.io.vn".to_string(),
            authorization_endpoint: "https://amecc.spm.io.vn/connect/authorize".to_string(),
            token_endpoint: "https://amecc.spm.io.vn/connect/token".to_string(),
            userinfo_endpoint: Some("https://amecc.spm.io.vn/connect/userinfo".to_string()),
            end_session_endpoint: Some("https://amecc.spm.io.vn/connect/logout".to_string()),
            token_endpoint_auth_methods_supported: vec![
                "client_secret_post".to_string(),
                "client_secret_basic".to_string(),
                "none".to_string(),
            ],
            code_challenge_methods_supported: vec!["S256".to_string(), "plain".to_string()],
        }
    }

    #[test]
    fn a_verifier_and_its_challenge_match_the_rfc_example() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn a_fresh_verifier_is_long_enough_and_never_repeats() {
        let first = new_verifier();
        let second = new_verifier();
        assert!((43..=128).contains(&first.len()), "{}", first.len());
        assert_ne!(first, second);
        assert!(!first.contains(['=', '+', '/']), "base64url without padding");
    }

    #[test]
    fn the_authorization_address_escapes_what_it_carries() {
        let address = authorization_url(
            &discovery(),
            "ocs web",
            "https://ssdnode.example/auth/callback",
            &["openid".to_string(), "profile".to_string()],
            "a+b/c",
            "challenge",
        );
        assert!(address.starts_with("https://amecc.spm.io.vn/connect/authorize?"));
        assert!(address.contains("client_id=ocs+web"), "{address}");
        assert!(address.contains("scope=openid+profile"), "{address}");
        assert!(address.contains("state=a%2Bb%2Fc"), "{address}");
        assert!(address.contains("code_challenge_method=S256"), "{address}");
        assert!(address.contains("response_type=code"), "{address}");
    }

    #[test]
    fn the_state_has_to_come_back_exactly() {
        let state = new_state();
        assert!(state_matches(&state, Some(&state)));
        assert!(!state_matches(&state, None));
        assert!(!state_matches(&state, Some("")));
        assert!(!state_matches(&state, Some("different")));
        assert!(!state_matches("abc", Some("abd")));
    }

    #[test]
    fn a_discovery_document_has_to_belong_to_the_issuer_we_asked() {
        assert!(discovery().check("https://amecc.spm.io.vn").is_ok());
        assert!(discovery().check("https://amecc.spm.io.vn/").is_ok());
        assert!(discovery().check("https://elsewhere.example").is_err());
    }

    #[test]
    fn a_provider_without_pkce_is_refused() {
        let mut document = discovery();
        document.code_challenge_methods_supported = vec!["plain".to_string()];
        assert!(document.check("https://amecc.spm.io.vn").is_err());
        // Saying nothing is not the same as refusing: older providers omit it.
        document.code_challenge_methods_supported.clear();
        assert!(document.check("https://amecc.spm.io.vn").is_ok());
    }

    #[test]
    fn the_client_proves_itself_the_way_the_provider_reads() {
        assert_eq!(discovery().auth_method(false), AuthMethod::None);
        assert_eq!(discovery().auth_method(true), AuthMethod::Post);
        let mut basic_only = discovery();
        basic_only.token_endpoint_auth_methods_supported = vec!["client_secret_basic".to_string()];
        assert_eq!(basic_only.auth_method(true), AuthMethod::Basic);
    }

    #[test]
    fn the_claims_come_out_of_a_token_payload() {
        // {"sub":"user-1","email":"a@b.vn","email_verified":true,"name":"Nguyen",
        //  "role":"viewer","roles":["editor","viewer"]}
        let payload = "eyJzdWIiOiJ1c2VyLTEiLCJlbWFpbCI6ImFAYi52biIsImVtYWlsX3ZlcmlmaWVkIjp0cnVlLCJuYW1lIjoiTmd1eWVuIiwicm9sZSI6InZpZXdlciIsInJvbGVzIjpbImVkaXRvciIsInZpZXdlciJdfQ";
        let token = format!("header.{payload}.signature");
        let claims = Claims::from_jwt_payload(&token).expect("a payload");
        assert_eq!(claims.sub, "user-1");
        assert_eq!(claims.email.as_deref(), Some("a@b.vn"));
        assert_eq!(claims.display_name(), "Nguyen");
        assert_eq!(claims.roles(), vec!["editor", "viewer"]);
        assert!(claims.is_acceptable());
    }

    #[test]
    fn a_user_without_a_subject_or_with_an_unverified_address_is_refused() {
        let mut claims = Claims {
            sub: String::new(),
            ..Claims::default()
        };
        assert!(!claims.is_acceptable());
        claims.sub = "user-1".to_string();
        claims.email_verified = Some(false);
        assert!(!claims.is_acceptable());
        claims.email_verified = None;
        assert!(claims.is_acceptable(), "silent about the address is fine");
    }

    #[test]
    fn one_role_as_a_string_reads_the_same_as_a_list() {
        let one = Claims {
            role: RoleClaim::One("viewer".to_string()),
            ..Claims::default()
        };
        let many = Claims {
            role: RoleClaim::Many(vec!["viewer".to_string()]),
            ..Claims::default()
        };
        assert_eq!(one.roles(), vec!["viewer"]);
        assert_eq!(many.roles(), vec!["viewer"]);
    }
}
