//! A small server for a self-hosted build: it signs a user in against an
//! OpenID Connect provider, then serves the drawings that user may see.
//!
//! Why a server at all: the provider does not answer cross-origin calls on
//! its token endpoint, so a page of static files cannot turn an
//! authorization code into a token. Keeping that exchange here also keeps
//! the client secret out of the browser.
//!
//! The browser holds only an opaque session cookie. Every drawing request
//! carries it — the app's fetch asks for same-origin credentials by default —
//! so `?file=/drawings/<id>` opens a private drawing with no change to the
//! wasm client.
//!
//! Layout of the module set:
//!
//! - [`config`] reads and checks the environment;
//! - [`oidc`] is the provider side: discovery, PKCE, the code exchange;
//! - [`session`] is this server's own state: the signed-in user behind a
//!   cookie, kept in memory so a restart simply asks people to sign in again;
//! - [`drawings`] is where drawings come from — local folders, for now;
//! - [`routes`] is the HTTP surface, and [`assets`] serves the web bundle.

pub mod assets;
pub mod config;
pub mod drawings;
pub mod oidc;
pub mod routes;
pub mod session;

use std::sync::Arc;

pub use config::Config;
pub use drawings::Source;
pub use session::Sessions;

/// Everything a request handler needs, built once at start-up.
pub struct App {
    pub config: Arc<Config>,
    pub provider: Arc<oidc::Provider>,
    pub sessions: Arc<Sessions>,
    pub drawings: Arc<Source>,
}

impl App {
    /// The router: the auth endpoints, the drawings, and the web bundle.
    pub fn router(self) -> axum::Router {
        routes::router(self)
    }
}
