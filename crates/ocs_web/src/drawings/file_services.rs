//! Drawings kept in INS.FileServices, read on behalf of the signed-in user.
//!
//! INS.FileServices stores files under a scope and serves them at
//! `/api/files/{id}/content`, guarded by its own permissions
//! (`files:read`) and by the caller's identity. This source therefore does
//! not decide who may see a drawing itself: it asks the service to list
//! what the caller may read, and it forwards the user's own token, so the
//! service's answer is the answer.
//!
//! Two things make that work:
//! - the list call names the module the drawings belong to
//!   (`ownerModule`), because the service requires one;
//! - the token forwarded is the OIDC access token the user signed in with.
//!   The file service has to trust the same provider's tokens — that is a
//!   deployment fact, not something this code can arrange.

use std::time::Duration;

use serde::Deserialize;

use super::{Drawing, OpenError};
use crate::session::User;

/// What a listing entry of INS.FileServices looks like. Only the fields this
/// server needs are read; the rest of the answer is ignored.
#[derive(Debug, Clone, Deserialize)]
struct ServiceFile {
    id: String,
    #[serde(rename = "fileName", default)]
    file_name: String,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(rename = "sizeBytes", default)]
    size_bytes_camel: Option<u64>,
}

impl ServiceFile {
    fn name(&self) -> &str {
        if self.file_name.is_empty() {
            &self.id
        } else {
            &self.file_name
        }
    }

    fn size(&self) -> u64 {
        self.size_bytes.or(self.size_bytes_camel).unwrap_or(0)
    }
}

/// The page of a listing the service answers with.
#[derive(Debug, Clone, Deserialize)]
struct Listing {
    #[serde(default, rename = "items")]
    items: Vec<ServiceFile>,
    #[serde(default, rename = "total")]
    _total: Option<u64>,
}

/// Drawings in one module of INS.FileServices.
#[derive(Debug, Clone)]
pub struct FileServices {
    /// The service's base address, e.g. `http://127.0.0.1:10319`.
    base: String,
    /// The `ownerModule` the drawings are filed under, e.g. `OCS`.
    owner_module: String,
    /// How many entries to ask for at once.
    page: u32,
    client: reqwest::Client,
}

impl FileServices {
    pub fn new(base: impl Into<String>, owner_module: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            owner_module: owner_module.into(),
            page: 200,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent("ocs-web/0.1".to_string())
                .build()
                .unwrap_or_default(),
        }
    }

    /// The listing request, with the user's token.
    async fn list_page(&self, user: &User, skip: u32) -> Result<Listing, String> {
        let address = format!(
            "{}/api/files?ownerModule={}&skip={}&take={}",
            self.base,
            url::form_urlencoded::byte_serialize(self.owner_module.as_bytes()).collect::<String>(),
            skip,
            self.page
        );
        let response = self
            .client
            .get(&address)
            .bearer_auth(&user.access_token)
            .send()
            .await
            .map_err(|error| format!("the file service is unreachable: {error}"))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("the file service answered {status}: {body}"));
        }
        response
            .json::<Listing>()
            .await
            .map_err(|error| format!("the file service's listing is not what was expected: {error}"))
    }

    /// One drawing's bytes, as the file service serves them.
    async fn fetch(&self, user: &User, id: &str) -> Result<Vec<u8>, OpenError> {
        // An id is the service's GUID; anything else cannot name a file.
        if !id.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-') || id.len() < 32 {
            return Err(OpenError::NotFound);
        }
        let address = format!("{}/api/files/{}/content", self.base, id);
        let response = self
            .client
            .get(&address)
            .bearer_auth(&user.access_token)
            .send()
            .await
            .map_err(|error| OpenError::Unreadable(format!("the file service is unreachable: {error}")))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND
            || response.status() == reqwest::StatusCode::FORBIDDEN
            || response.status() == reqwest::StatusCode::UNAUTHORIZED
        {
            return Err(OpenError::NotFound);
        }
        if !response.status().is_success() {
            return Err(OpenError::Unreadable(format!(
                "the file service answered {}",
                response.status()
            )));
        }
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|error| OpenError::Unreadable(format!("the drawing could not be read: {error}")))
    }
}

impl FileServices {
    /// The drawings the service says this user may read.
    pub async fn list_async(&self, user: &User) -> Result<Vec<Drawing>, String> {
        let mut drawings = Vec::new();
        loop {
            let skip = drawings.len() as u32;
            let page = self.list_page(user, skip).await?;
            let received = page.items.len() as u32;
            for file in page.items {
                if !super::is_drawing_name(file.name()) {
                    continue;
                }
                drawings.push(Drawing {
                    id: file.id.clone(),
                    name: file.name().to_string(),
                    collection: self.owner_module.clone(),
                    bytes: file.size(),
                });
            }
            if received < self.page {
                break;
            }
            // The service counts every file, drawings or not, so a page that
            // was full but added no drawing must not loop forever.
            if drawings.len() as u32 == skip {
                break;
            }
        }
        drawings.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(drawings)
    }

    /// One drawing's bytes.
    pub async fn open_async(&self, user: &User, id: &str) -> Result<Vec<u8>, OpenError> {
        self.fetch(user, id).await
    }

    /// Whether a name looks like something a drawing service files.
    #[cfg(test)]
    pub(crate) fn is_drawing(file_name: &str) -> bool {
        super::is_drawing_name(file_name)
    }
}

#[cfg(test)]
mod tests {
    //! Parsing what the file service answers, without a service.
    use super::{FileServices, ServiceFile};

    #[test]
    fn a_listing_entry_reads_its_name_and_size_in_either_spelling() {
        let camel: ServiceFile =
            serde_json::from_str(r#"{"id":"6f1c1f5c-0000-4000-8000-000000000001","fileName":"a.dxf","sizeBytes":1234}"#)
                .expect("camelCase");
        assert_eq!(camel.name(), "a.dxf");
        assert_eq!(camel.size(), 1234);
        let snake: ServiceFile =
            serde_json::from_str(r#"{"id":"6f1c1f5c-0000-4000-8000-000000000002","size_bytes":7}"#)
                .expect("snake_case");
        assert_eq!(snake.size(), 7);
        // No name falls back to the id, so a row is never blank.
        assert_eq!(snake.name(), "6f1c1f5c-0000-4000-8000-000000000002");
    }

    #[test]
    fn only_drawing_names_are_offered() {
        assert!(FileServices::is_drawing("ban-ve.dxf"));
        assert!(FileServices::is_drawing("chi-tiet.DWG"));
        assert!(!FileServices::is_drawing("bien-ban.pdf"));
        assert!(!FileServices::is_drawing("notes.txt"));
    }

    #[tokio::test]
    async fn an_identifier_that_is_not_a_service_guid_never_reaches_the_network() {
        let source = FileServices::new("http://127.0.0.1:1", "OCS");
        let user = crate::session::User {
            subject: "u".to_string(),
            email: String::new(),
            name: String::new(),
            roles: Vec::new(),
            access_token: "t".to_string(),
        };
        assert_eq!(
            source.open_async(&user, "../etc/passwd").await,
            Err(crate::drawings::OpenError::NotFound)
        );
        assert_eq!(
            source.open_async(&user, "not-a-guid").await,
            Err(crate::drawings::OpenError::NotFound)
        );
    }
}
