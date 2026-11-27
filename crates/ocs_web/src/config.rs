//! Configuration, read once from the environment at start-up.
//!
//! Every setting has an environment name, and a default where a default is
//! safe. There is no config file: a self-hosted install is one binary, a web
//! bundle folder and a drawings folder, and the rest is an environment file
//! the service manager reads.

use std::path::PathBuf;

/// Where the provider sends the browser back; the last part of
/// `OCS_OIDC_REDIRECT_URI`.
pub const CALLBACK_PATH: &str = "/auth/callback";
/// The scopes asked for when `OCS_OIDC_SCOPES` is unset. `email` and `profile`
/// give the name and address the interface shows.
pub const DEFAULT_SCOPES: &str = "openid profile email";
/// Claims that let a user see every collection when `OCS_ADMIN_CLAIMS` is
/// unset.
pub const DEFAULT_ADMIN_CLAIMS: &str = "roles:ocs-admin role:ocs-admin";
/// The address listened on when `OCS_LISTEN` is unset.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:8080";

/// Everything the server needs to know about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The provider's `issuer`. Its endpoints are found through
    /// `/.well-known/openid-configuration`.
    pub issuer: String,
    /// The client registered at the provider.
    pub client_id: String,
    /// Its secret, when the registered client has one.
    pub client_secret: String,
    /// Where the provider sends the browser back. It must be registered at
    /// the provider and must end in [`CALLBACK_PATH`].
    pub redirect_uri: String,
    /// What to ask for besides `openid`.
    pub scopes: Vec<String>,
    /// The web build to serve: the folder that holds `index.html`. Empty
    /// serves the API only, for a reverse proxy that serves the bundle itself.
    pub static_dir: Option<PathBuf>,
    /// The folder drawings are read from. Each subfolder is one collection.
    pub drawings_dir: Option<PathBuf>,
    /// INS.FileServices' base address (`http://127.0.0.1:10319`), when the
    /// drawings live there instead of in a folder.
    pub file_services_url: Option<String>,
    /// The `ownerModule` the drawings are filed under in that service.
    pub file_services_module: String,
    /// Claims that let a user see every collection.
    pub admin_claims: Vec<String>,
    /// The address to listen on.
    pub listen: String,
    /// Whether the session cookie carries `Secure`. On unless turned off: a
    /// deployment is HTTPS, since the provider requires an HTTPS redirect.
    pub cookie_secure: bool,
}

impl Config {
    /// Reads the configuration, refusing combinations that cannot work.
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// [`Config::from_env`] over any lookup, so the rules are testable
    /// without touching the process environment.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let get = |name: &str| lookup(name).filter(|value| !value.trim().is_empty());
        let required = |name: &str| get(name).ok_or_else(|| format!("{name} is not set"));

        let issuer = required("OCS_OIDC_ISSUER")?;
        require_web_address("OCS_OIDC_ISSUER", &issuer)?;
        let client_id = required("OCS_OIDC_CLIENT_ID")?;
        let redirect_uri = required("OCS_OIDC_REDIRECT_URI")?;
        require_web_address("OCS_OIDC_REDIRECT_URI", &redirect_uri)?;
        if !redirect_uri.ends_with(CALLBACK_PATH) {
            return Err(format!(
                "OCS_OIDC_REDIRECT_URI must end in {CALLBACK_PATH}: {redirect_uri}"
            ));
        }
        let scopes: Vec<String> = get("OCS_OIDC_SCOPES")
            .unwrap_or_else(|| DEFAULT_SCOPES.to_string())
            .split_whitespace()
            .map(str::to_string)
            .collect();
        if !scopes.iter().any(|scope| scope == "openid") {
            return Err("OCS_OIDC_SCOPES must include openid".to_string());
        }
        let static_dir = optional_folder("OCS_STATIC_DIR", get("OCS_STATIC_DIR"), |path| {
            path.join("index.html").is_file()
        })?;
        let drawings_dir = optional_folder("OCS_DRAWINGS_DIR", get("OCS_DRAWINGS_DIR"), |path| {
            path.is_dir()
        })?;
        let file_services_url = get("OCS_FILE_SERVICES_URL");
        if static_dir.is_none() && drawings_dir.is_none() && file_services_url.is_none() {
            return Err(
                "nothing to serve: set OCS_STATIC_DIR (the web build), OCS_DRAWINGS_DIR or                  OCS_FILE_SERVICES_URL"
                    .to_string(),
            );
        }
        Ok(Self {
            issuer: issuer.trim_end_matches('/').to_string(),
            client_id,
            client_secret: get("OCS_OIDC_CLIENT_SECRET").unwrap_or_default(),
            redirect_uri,
            scopes,
            static_dir,
            drawings_dir,
            file_services_url: file_services_url.map(|url| url.trim_end_matches('/').to_string()),
            file_services_module: get("OCS_FILE_SERVICES_MODULE").unwrap_or_else(|| "OCS".to_string()),
            admin_claims: get("OCS_ADMIN_CLAIMS")
                .unwrap_or_else(|| DEFAULT_ADMIN_CLAIMS.to_string())
                .split([' ', ','])
                .filter(|claim| !claim.is_empty())
                .map(str::to_string)
                .collect(),
            listen: get("OCS_LISTEN").unwrap_or_else(|| DEFAULT_LISTEN.to_string()),
            cookie_secure: !matches!(get("OCS_COOKIE_SECURE").as_deref(), Some("0" | "false")),
        })
    }
}

/// A folder setting: absent is fine, present must be a folder that `usable`
/// accepts.
fn optional_folder(
    name: &str,
    value: Option<String>,
    usable: impl Fn(&std::path::Path) -> bool,
) -> Result<Option<PathBuf>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let path = PathBuf::from(value);
    if usable(&path) {
        Ok(Some(path))
    } else {
        Err(format!("{name} does not point at a usable folder: {}", path.display()))
    }
}

/// The address a browser will be sent to: HTTPS, or plain HTTP on loopback
/// for a test setup.
fn require_web_address(name: &str, address: &str) -> Result<(), String> {
    if address.starts_with("https://") || is_loopback(address) {
        Ok(())
    } else {
        Err(format!(
            "{name} must be an https address (or a loopback one for a test): {address}"
        ))
    }
}

/// Whether the address is plain HTTP on this machine.
pub fn is_loopback(address: &str) -> bool {
    let Some(rest) = address.strip_prefix("http://") else {
        return false;
    };
    // A bracketed IPv6 host carries colons of its own; read to the closing
    // bracket before looking for the port.
    let host = if let Some(bracketed) = rest.strip_prefix('[') {
        bracketed.split(']').next().map(|host| format!("[{host}]")).unwrap_or_default()
    } else {
        rest.split(['/', ':']).next().unwrap_or_default().to_string()
    };
    matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]")
}

#[cfg(test)]
mod tests {
    //! The rules that decide whether a deployment can start.
    use super::{is_loopback, Config, CALLBACK_PATH};
    use std::collections::HashMap;

    fn config(overrides: &[(&str, &str)]) -> Result<Config, String> {
        let mut values: HashMap<String, String> = HashMap::from([
            (
                "OCS_OIDC_ISSUER".to_string(),
                "https://amecc.spm.io.vn".to_string(),
            ),
            ("OCS_OIDC_CLIENT_ID".to_string(), "ocs-web".to_string()),
            (
                "OCS_OIDC_REDIRECT_URI".to_string(),
                "https://ssdnode.example/auth/callback".to_string(),
            ),
            // A folder that exists on every platform the tests run on. The
            // literal `/tmp` was read as `C:\tmp` on a Windows runner, which has
            // no such folder, so the folder check refused a configuration these
            // tests mean to be accepted.
            (
                "OCS_DRAWINGS_DIR".to_string(),
                std::env::temp_dir().to_string_lossy().into_owned(),
            ),
        ]);
        for (name, value) in overrides {
            values.insert((*name).to_string(), (*value).to_string());
        }
        Config::from_lookup(|name| values.get(name).cloned())
    }

    #[test]
    fn a_complete_configuration_reads_back_its_values() {
        let read = config(&[]).expect("a complete configuration");
        assert_eq!(read.issuer, "https://amecc.spm.io.vn");
        assert_eq!(read.scopes, vec!["openid", "profile", "email"]);
        assert!(read.cookie_secure);
        assert_eq!(read.listen, "127.0.0.1:8080");
        assert!(read.redirect_uri.ends_with(CALLBACK_PATH));
    }

    #[test]
    fn the_issuer_and_the_redirect_have_to_be_https_or_loopback() {
        assert!(config(&[("OCS_OIDC_ISSUER", "http://amecc.spm.io.vn")]).is_err());
        assert!(config(&[("OCS_OIDC_REDIRECT_URI", "http://ssdnode.example/x/callback")]).is_err());
        assert!(config(&[("OCS_OIDC_ISSUER", "http://127.0.0.1:9999")]).is_ok());
        assert!(is_loopback("http://localhost:8080/auth/callback"));
        assert!(is_loopback("http://[::1]:8080/"));
        assert!(!is_loopback("http://ssdnode.local"));
    }

    #[test]
    fn the_redirect_has_to_be_the_callback_path() {
        let refused = config(&[("OCS_OIDC_REDIRECT_URI", "https://ssdnode.example/signin")])
            .expect_err("a redirect elsewhere");
        assert!(refused.contains(CALLBACK_PATH), "{refused}");
    }

    #[test]
    fn openid_is_always_among_the_scopes() {
        assert!(config(&[("OCS_OIDC_SCOPES", "profile email")]).is_err());
        let read = config(&[("OCS_OIDC_SCOPES", "openid str.api")]).expect("scopes");
        assert_eq!(read.scopes, vec!["openid", "str.api"]);
    }

    #[test]
    fn a_missing_secret_means_a_public_client_and_nothing_to_serve_is_refused() {
        let read = config(&[("OCS_OIDC_CLIENT_SECRET", "")]).expect("no secret");
        assert!(read.client_secret.is_empty());
        let refused = config(&[("OCS_DRAWINGS_DIR", "")]).expect_err("nothing to serve");
        assert!(refused.contains("OCS_STATIC_DIR"), "{refused}");
    }

    #[test]
    fn turning_the_secure_cookie_off_is_explicit() {
        assert!(!config(&[("OCS_COOKIE_SECURE", "0")])
            .expect("plain cookie")
            .cookie_secure);
        assert!(!config(&[("OCS_COOKIE_SECURE", "false")])
            .expect("plain cookie")
            .cookie_secure);
    }
}
