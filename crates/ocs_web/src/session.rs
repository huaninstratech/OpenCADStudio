//! Who is signed in, and how the browser proves it.
//!
//! The browser holds one opaque cookie value. Everything else — the tokens,
//! the name, the roles — stays here, in memory. A restart forgets the
//! sessions and everyone signs in again, which is the honest behaviour for a
//! server that keeps no database: nothing stale survives, and no session
//! outlives the process that could revoke it.
//!
//! Sign-ins that have left for the provider are kept in the same place, so a
//! callback can only finish a sign-in this server started, and only once.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::oidc::{self, Pending};

/// A signed-in user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// The provider's subject: the only identifier that is stable.
    pub subject: String,
    pub email: String,
    pub name: String,
    pub roles: Vec<String>,
    /// The provider's access token, kept to ask it things on the user's
    /// behalf (the drawings source may use it).
    pub access_token: String,
}

impl User {
    pub fn is_anonymous(&self) -> bool {
        self.subject.is_empty()
    }
}

struct Signed {
    user: User,
    /// When the session stops being accepted. A session is dropped when it
    /// would otherwise be older than [`SESSION_LIFETIME`].
    renewed: Instant,
}

/// How long a sign-in lasts. The provider's own session decides whether the
/// browser has to type a password again; this only bounds how long this
/// server keeps answering for a cookie it has not seen renewed.
pub const SESSION_LIFETIME: std::time::Duration = std::time::Duration::from_secs(12 * 60 * 60);

/// The session store: opaque cookie values to users, and `state` to pending
/// sign-ins.
#[derive(Default)]
pub struct Sessions {
    signed: Mutex<HashMap<String, Signed>>,
    pending: Mutex<HashMap<String, Pending>>,
}

impl Sessions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a sign-in that is about to leave for the provider, keyed by
    /// its `state`; the callback spends it. The key is the `state` the
    /// provider will echo, so nothing else can finish this sign-in.
    pub fn start(&self, state: &str, verifier: &str) {
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                state.to_string(),
                Pending {
                    verifier: verifier.to_string(),
                    started: Instant::now(),
                },
            );
    }

    /// Takes the pending sign-in for `state`, refusing one that has waited
    /// longer than [`oidc::PENDING_LIFETIME`] or never existed.
    pub fn take_pending(&self, state: &str) -> Option<String> {
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(state)
            .filter(|pending| pending.started.elapsed() <= oidc::PENDING_LIFETIME)
            .map(|pending| pending.verifier)
    }

    /// Forgets pending sign-ins nobody came back for.
    pub fn sweep_pending(&self) {
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|_, pending| pending.started.elapsed() <= oidc::PENDING_LIFETIME);
    }

    /// Starts a session for `user` and gives the cookie value for it.
    pub fn sign_in(&self, user: User) -> String {
        let id = oidc::new_state();
        self.signed
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(id.clone(), Signed {
                user,
                renewed: Instant::now(),
            });
        id
    }

    /// The user behind a cookie value, when the session is still young
    /// enough. Each read renews the session's lease, so an active user is
    /// not signed out mid-drawing.
    pub fn user(&self, id: Option<&str>) -> Option<User> {
        let id = id?;
        let mut signed = self.signed.lock().unwrap_or_else(|error| error.into_inner());
        let entry = signed.get_mut(id)?;
        if entry.renewed.elapsed() > SESSION_LIFETIME {
            signed.remove(id);
            return None;
        }
        entry.renewed = Instant::now();
        Some(entry.user.clone())
    }

    /// Ends one session.
    pub fn sign_out(&self, id: Option<&str>) {
        if let Some(id) = id {
            self.signed
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(id);
        }
    }

    /// How many sessions are open: for the health endpoint and the tests.
    pub fn open_sessions(&self) -> usize {
        self.signed
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
    }
}

#[cfg(test)]
mod tests {
    //! Sessions and pending sign-ins, without a provider.
    use super::*;

    fn user(subject: &str) -> User {
        User {
            subject: subject.to_string(),
            email: format!("{subject}@example.vn"),
            name: "Nguyen".to_string(),
            roles: vec!["viewer".to_string()],
            access_token: "token".to_string(),
        }
    }

    #[test]
    fn a_cookie_reads_back_its_user_and_a_stranger_reads_nothing() {
        let sessions = Sessions::new();
        let cookie = sessions.sign_in(user("user-1"));
        assert_eq!(sessions.user(Some(&cookie)).map(|u| u.subject), Some("user-1".to_string()));
        assert!(sessions.user(Some("not-a-session")).is_none());
        assert!(sessions.user(None).is_none());
        assert_eq!(sessions.open_sessions(), 1);
    }

    #[test]
    fn signing_out_forgets_the_session() {
        let sessions = Sessions::new();
        let cookie = sessions.sign_in(user("user-1"));
        sessions.sign_out(Some(&cookie));
        assert!(sessions.user(Some(&cookie)).is_none());
        assert_eq!(sessions.open_sessions(), 0);
    }

    #[test]
    fn a_pending_sign_in_is_spent_the_first_time_it_comes_back() {
        let sessions = Sessions::new();
        sessions.start("state-1", "verifier-1");
        assert_eq!(sessions.take_pending("state-1").as_deref(), Some("verifier-1"));
        assert_eq!(sessions.take_pending("state-1"), None, "only once");
        assert_eq!(sessions.take_pending("state-2"), None, "never started");
    }

    #[test]
    fn the_cookie_values_are_not_guessable() {
        let sessions = Sessions::new();
        let first = sessions.sign_in(user("user-1"));
        let second = sessions.sign_in(user("user-2"));
        assert_ne!(first, second);
        assert!(first.len() >= 32, "{}", first.len());
    }
}
