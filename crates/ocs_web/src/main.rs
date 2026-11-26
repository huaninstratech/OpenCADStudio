//! The `ocs-web` server: a self-hosted build of the web interface, with
//! sign-in against an OpenID Connect provider and drawings behind it.
//!
//! Run it where the web bundle and the drawings live:
//!
//! ```text
//! ocs-web
//! ```
//!
//! with the settings in the environment (see `crates/ocs_web/README.md`).
//! Everything it needs is read at start-up, so a bad configuration fails
//! before the first request rather than inside it.

use std::sync::Arc;

use ocs_web::{config::Config, drawings::LocalFolder, oidc::Provider, App, Sessions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ocs_web=info,tower_http=warn".into()),
        )
        .init();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("ocs-web: {message}");
            eprintln!();
            eprintln!("{}", usage());
            std::process::exit(2);
        }
    };

    // One client for the provider: TLS roots from the system, a timeout that
    // is shorter than a browser's patience, and no cookie jar — the only
    // credential held here is the client secret.
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(concat!("ocs-web/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let provider = Provider::discover(
        http,
        &config.issuer,
        &config.client_id,
        &config.client_secret,
        &config.redirect_uri,
        &config.scopes,
    )
    .await
    .map_err(|message| format!("the provider cannot be used: {message}"))?;
    tracing::info!(
        issuer = %config.issuer,
        token_endpoint = %provider.discovery().token_endpoint,
        static_dir = ?config.static_dir,
        drawings_dir = ?config.drawings_dir,
        "provider ready"
    );

    // Where the drawings come from: the suite's file service when its address
    // is configured, otherwise a folder on this machine.
    let drawings: Arc<ocs_web::Source> = match (&config.file_services_url, &config.drawings_dir) {
        (Some(base), _) => {
            tracing::info!(file_service = %base, owner_module = %config.file_services_module, "drawings from INS.FileServices");
            Arc::new(ocs_web::Source::FileServices(
                ocs_web::drawings::file_services::FileServices::new(
                    base.clone(),
                    config.file_services_module.clone(),
                ),
            ))
        }
        (None, Some(root)) => Arc::new(ocs_web::Source::Folder(LocalFolder::new(root.clone()))),
        (None, None) => Arc::new(ocs_web::Source::None(ocs_web::drawings::NoDrawings)),
    };
    let app = App {
        config: Arc::new(config.clone()),
        provider: Arc::new(provider),
        sessions: Arc::new(Sessions::new()),
        drawings,
    };
    let router = app.router();

    let listener = tokio::net::TcpListener::bind(&config.listen).await?;
    tracing::info!("listening on http://{}", config.listen);
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Resolve when the process is asked to stop, so connections drain instead
/// of being cut.
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

/// Printed when the configuration does not work, so an operator does not
/// have to find the README first.
fn usage() -> &'static str {
    "Set up an OpenID client at the provider, then run with:
  OCS_OIDC_ISSUER=https://amecc.spm.io.vn
  OCS_OIDC_CLIENT_ID=ocs-web
  OCS_OIDC_CLIENT_SECRET=<the client's secret>
  OCS_OIDC_REDIRECT_URI=https://<your host>/auth/callback
  OCS_STATIC_DIR=/srv/ocs/app
  OCS_DRAWINGS_DIR=/srv/ocs/drawings
  OCS_LISTEN=127.0.0.1:8080
See crates/ocs_web/README.md for the rest."
}
