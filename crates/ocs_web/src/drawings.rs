//! Where drawings come from.
//!
//! The server serves drawings; which drawings a user may see, and where they
//! are kept, is a separate question. [`Source`] is that question, and it has
//! two answers:
//!
//! - **a folder** ([`LocalFolder`]) — collections of files on this machine,
//!   with an optional `access` file per collection naming the roles that may
//!   see it;
//! - **INS.FileServices** ([`file_services::FileServices`]) — the suite's file
//!   service, asked with the signed-in user's own token, so the service's
//!   permissions are the answer.
//!
//! Both are asynchronous because one of them talks to a service; the folder
//! source simply never awaits anything slow.
//!
//! Access for the folder source: a user sees a collection when one of their
//! roles is listed in its `access` file, or the collection has no `access`
//! file (the operator's own folder), or one of
//! [`crate::config::Config::admin_claims`] is among their roles.

pub mod file_services;

use std::path::{Component, Path, PathBuf};

use crate::session::User;

/// One drawing, as a list entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drawing {
    /// The identifier the address uses: `collection/name`, or the file
    /// service's id.
    pub id: String,
    pub name: String,
    /// The collection it is in, for grouping in a list.
    pub collection: String,
    pub bytes: u64,
}

/// Where the drawings a user may see are read from.
pub enum Source {
    Folder(LocalFolder),
    FileServices(file_services::FileServices),
    None(NoDrawings),
}

impl Source {
    /// The drawings `user` may see, in a stable order.
    pub async fn list(&self, user: &User, admin: &AdminPolicy) -> Vec<Drawing> {
        match self {
            Self::Folder(folder) => folder.list(user, admin),
            Self::FileServices(service) => match service.list_async(user).await {
                Ok(drawings) => drawings,
                Err(message) => {
                    // A service that is down is reported on the interface as
                    // an empty list; the reason goes to the log so it is not
                    // swallowed.
                    tracing::warn!("the file service could not list drawings: {message}");
                    Vec::new()
                }
            },
            Self::None(none) => none.list(user, admin),
        }
    }

    /// The bytes of drawing `id`, when `user` may see it.
    pub async fn open(
        &self,
        user: &User,
        admin: &AdminPolicy,
        id: &str,
    ) -> Result<Vec<u8>, OpenError> {
        match self {
            Self::Folder(folder) => folder.open(user, admin, id),
            Self::FileServices(service) => service.open_async(user, id).await,
            Self::None(none) => none.open(user, admin, id),
        }
    }
}

/// Whether a user is an administrator, decided by the configured claims.
#[derive(Debug, Clone)]
pub struct AdminPolicy {
    /// Role names that grant access to every collection. A claim written
    /// `roles:ocs-admin` means "the `roles` claim holds `ocs-admin`"; a bare
    /// `ocs-admin` means "any role is `ocs-admin`".
    pub roles: Vec<String>,
}

impl AdminPolicy {
    pub fn new(claims: &[String]) -> Self {
        let roles = claims
            .iter()
            .map(|claim| {
                claim
                    .rsplit_once(':')
                    .map(|(_, role)| role.to_string())
                    .unwrap_or_else(|| claim.clone())
            })
            .filter(|role| !role.is_empty())
            .collect();
        Self { roles }
    }

    pub fn allows(&self, user: &User) -> bool {
        user.roles
            .iter()
            .any(|role| self.roles.iter().any(|allowed| allowed == role))
    }
}

/// Why a drawing could not be opened.
#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    /// No such drawing, or the user may not see it. One error for both: a
    /// server that says which would tell a stranger what exists.
    NotFound,
    /// The drawing is there but could not be read.
    Unreadable(String),
}

/// A source with nothing in it: a server that signs users in but serves no
/// drawings (the bundle only).
#[derive(Debug, Clone, Copy)]
pub struct NoDrawings;

impl NoDrawings {
    fn list(&self, _user: &User, _admin: &AdminPolicy) -> Vec<Drawing> {
        Vec::new()
    }

    fn open(&self, _user: &User, _admin: &AdminPolicy, _id: &str) -> Result<Vec<u8>, OpenError> {
        Err(OpenError::NotFound)
    }
}

/// Drawings in a folder: one subfolder per collection, the drawing files
/// directly inside it.
#[derive(Debug, Clone)]
pub struct LocalFolder {
    root: PathBuf,
    /// A collection made of the root's own files, always visible.
    loose: bool,
}

impl LocalFolder {
    pub fn new(root: PathBuf) -> Self {
        Self { root, loose: true }
    }

    /// The folder a collection lives in, refusing a name that is not one
    /// plain path component: `..` and separators never reach the disk.
    fn collection_dir(&self, collection: &str) -> Option<PathBuf> {
        if collection.is_empty() || collection.starts_with('_') {
            return None;
        }
        let mut path = Path::new(collection).components();
        match (path.next(), path.next()) {
            (Some(Component::Normal(name)), None) if name == collection => {
                Some(self.root.join(name))
            }
            _ => None,
        }
    }

    /// The drawing a path names, if it is a drawing file name.
    fn drawing_at(&self, collection: &str, name: &str) -> Option<Drawing> {
        if !is_drawing_name(name) {
            return None;
        }
        let path = self.collection_dir(collection)?.join(name);
        if !path.is_file() {
            return None;
        }
        Some(Drawing {
            id: format!("{collection}/{name}"),
            name: name.to_string(),
            collection: collection.to_string(),
            bytes: path.metadata().map(|metadata| metadata.len()).unwrap_or(0),
        })
    }

    /// The roles a collection's `access` file names, or `None` when it has
    /// none — an operator's own collection, visible to anyone signed in.
    fn collection_roles(&self, collection: &str) -> Option<Vec<String>> {
        let access = self.collection_dir(collection)?.join("access");
        let text = std::fs::read_to_string(access).ok()?;
        Some(
            text.split(['\n', ',', ' '])
                .map(str::trim)
                .filter(|role| !role.is_empty())
                .map(str::to_string)
                .collect(),
        )
    }

    /// Whether `user` may see the collection: an admin sees every one, a
    /// collection without an `access` file is open to every signed-in user,
    /// and otherwise one of the user's roles has to be named.
    fn may_see(&self, user: &User, admin: &AdminPolicy, collection: &str) -> bool {
        if admin.allows(user) {
            return true;
        }
        match self.collection_roles(collection) {
            None => true,
            Some(roles) => user.roles.iter().any(|role| roles.contains(role)),
        }
    }
}

/// Whether a file name is a drawing this build opens. Mirrors the web
/// client's `?file=` gate, so what a listing offers is what the client opens.
pub fn is_drawing_name(name: &str) -> bool {
    let extension = name.rsplit('.').next().unwrap_or_default();
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "dwg" | "dxf" | "ifcx" | "ifccad"
    )
}

impl LocalFolder {
    fn list(&self, user: &User, admin: &AdminPolicy) -> Vec<Drawing> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut drawings = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if path.is_dir() {
                if !self.may_see(user, admin, name) {
                    continue;
                }
                let Ok(files) = std::fs::read_dir(&path) else {
                    continue;
                };
                for file in files.flatten() {
                    if let Some(file_name) = file.file_name().to_str() {
                        if let Some(drawing) = self.drawing_at(name, file_name) {
                            drawings.push(drawing);
                        }
                    }
                }
            } else if self.loose && is_drawing_name(name) {
                drawings.push(Drawing {
                    id: name.to_string(),
                    name: name.to_string(),
                    collection: String::new(),
                    bytes: path.metadata().map(|metadata| metadata.len()).unwrap_or(0),
                });
            }
        }
        drawings.sort_by(|a, b| (&a.collection, &a.name).cmp(&(&b.collection, &b.name)));
        drawings
    }

    fn open(&self, user: &User, admin: &AdminPolicy, id: &str) -> Result<Vec<u8>, OpenError> {
        let (path, collection) = match id.split_once('/') {
            Some((collection, name)) => {
                let path = self
                    .collection_dir(collection)
                    .ok_or(OpenError::NotFound)?
                    .join(safe_file_name(name).ok_or(OpenError::NotFound)?);
                (path, collection)
            }
            None if self.loose => (
                self.root.join(safe_file_name(id).ok_or(OpenError::NotFound)?),
                "",
            ),
            None => return Err(OpenError::NotFound),
        };
        if !collection.is_empty() && !self.may_see(user, admin, collection) {
            return Err(OpenError::NotFound);
        }
        let bytes = std::fs::read(&path).map_err(|_| OpenError::NotFound)?;
        Ok(bytes)
    }
}

/// A file name that is one plain path component.
fn safe_file_name(name: &str) -> Option<&str> {
    if name.is_empty() || name.starts_with('.') {
        return None;
    }
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Some(name),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! The folder source: which drawings a user sees, and what a hostile
    //! identifier cannot reach.
    use super::*;

    fn user(roles: &[&str]) -> User {
        User {
            subject: "user-1".to_string(),
            email: "a@b.vn".to_string(),
            name: "Nguyen".to_string(),
            roles: roles.iter().map(|role| role.to_string()).collect(),
            access_token: String::new(),
        }
    }

    fn policy(roles: &[&str]) -> AdminPolicy {
        AdminPolicy::new(&roles.iter().map(|role| role.to_string()).collect::<Vec<_>>())
    }

    /// A folder with an open collection, one restricted to `viewer`, one
    /// restricted to someone else, and a loose drawing at the root.
    fn folder() -> (tempfile::TempDir, LocalFolder) {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let root = dir.path();
        std::fs::create_dir(root.join("du-an-a")).unwrap();
        std::fs::write(root.join("du-an-a/ban-ve.dxf"), b"0\nSECTION\n").unwrap();
        std::fs::write(root.join("du-an-a/notes.txt"), b"not a drawing").unwrap();
        std::fs::create_dir(root.join("du-an-b")).unwrap();
        std::fs::write(root.join("du-an-b/chi-tiet.dwg"), b"DWG").unwrap();
        std::fs::write(root.join("du-an-b/access"), b"viewer\n").unwrap();
        std::fs::create_dir(root.join("du-an-c")).unwrap();
        std::fs::write(root.join("du-an-c/rieng.dxf"), b"0\n").unwrap();
        std::fs::write(root.join("du-an-c/access"), b"someone-else\n").unwrap();
        std::fs::write(root.join("loose.dxf"), b"0\n").unwrap();
        let folder = LocalFolder::new(root.to_path_buf());
        (dir, folder)
    }

    #[test]
    fn a_listing_shows_what_the_user_may_open_and_only_drawings() {
        let (_keep, folder) = folder();
        let ids: Vec<String> = folder
            .list(&user(&["viewer"]), &policy(&["ocs-admin"]))
            .into_iter()
            .map(|drawing| drawing.id)
            .collect();
        assert!(ids.contains(&"du-an-a/ban-ve.dxf".to_string()));
        assert!(ids.contains(&"du-an-b/chi-tiet.dwg".to_string()));
        assert!(ids.contains(&"loose.dxf".to_string()));
        assert!(
            !ids.iter().any(|id| id.starts_with("du-an-c/")),
            "a collection for another role stays hidden: {ids:?}"
        );
        assert!(
            !ids.iter().any(|id| id.ends_with(".txt")),
            "a non-drawing is not offered: {ids:?}"
        );
    }

    #[test]
    fn an_administrator_sees_every_collection() {
        let (_keep, folder) = folder();
        let ids: Vec<String> = folder
            .list(&user(&["ocs-admin"]), &policy(&["ocs-admin"]))
            .into_iter()
            .map(|drawing| drawing.id)
            .collect();
        assert!(ids.iter().any(|id| id == "du-an-c/rieng.dxf"), "{ids:?}");
    }

    #[test]
    fn opening_a_restricted_drawing_needs_the_role_and_says_nothing_when_it_fails() {
        let (_keep, folder) = folder();
        let admin = policy(&["ocs-admin"]);
        assert!(folder.open(&user(&["viewer"]), &admin, "du-an-b/chi-tiet.dwg").is_ok());
        assert_eq!(
            folder.open(&user(&["viewer"]), &admin, "du-an-c/rieng.dxf"),
            Err(OpenError::NotFound)
        );
    }

    #[test]
    fn an_identifier_cannot_reach_outside_the_folder() {
        let (_keep, folder) = folder();
        let admin = policy(&["ocs-admin"]);
        for hostile in [
            "../Cargo.toml",
            "du-an-a/../../Cargo.toml",
            "du-an-a/..",
            "/etc/passwd",
            "du-an-a/=evil",
            "du-an-a/.hidden",
            "",
        ] {
            assert_eq!(
                folder.open(&user(&[]), &admin, hostile),
                Err(OpenError::NotFound),
                "{hostile:?} was refused"
            );
        }
    }

    #[test]
    fn a_collection_named_like_a_parent_or_hidden_folder_is_not_reachable() {
        let folder = LocalFolder::new(PathBuf::from("/drawings"));
        assert!(folder.collection_dir("..").is_none());
        assert!(folder.collection_dir("_internal").is_none());
        assert!(folder.collection_dir("a/b").is_none());
        assert!(folder.collection_dir("du-an-a").is_some());
    }

    #[test]
    fn an_admin_claim_may_name_the_claim_it_belongs_to() {
        let policy = AdminPolicy::new(&["roles:ocs-admin".to_string(), "ops".to_string()]);
        assert_eq!(policy.roles, vec!["ocs-admin", "ops"]);
        assert!(policy.allows(&user(&["ocs-admin"])));
        assert!(policy.allows(&user(&["ops"])));
        assert!(!policy.allows(&user(&["viewer"])));
    }

    #[test]
    fn the_drawing_names_are_the_ones_the_client_opens() {
        for name in ["a.dwg", "a.DXF", "a.ifcx", "a.ifccad"] {
            assert!(is_drawing_name(name), "{name}");
        }
        for name in ["a.txt", "a.pdf", "a.dwg.txt"] {
            assert!(!is_drawing_name(name), "{name}");
        }
        // `dwg` alone has no dot; `rsplit` then reads the whole name as the
        // extension, which is the same rule the client's `?file=` gate uses.
        assert!(is_drawing_name("dwg"));
    }
}
