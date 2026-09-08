use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use proxy_core::rules::RewriteRule;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const RULE_STORE_VERSION: u16 = 1;
pub const MAX_RULE_STORE_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub struct RuleStore {
    path: PathBuf,
    write_lock: Arc<Mutex<()>>,
}

#[derive(Debug, thiserror::Error)]
pub enum RuleStoreError {
    #[error("rule store is too large ({actual} bytes; maximum {maximum})")]
    TooLarge { actual: usize, maximum: usize },
    #[error("unsupported rule store version {0}")]
    UnsupportedVersion(u16),
    #[error("invalid rule store JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("rule store I/O error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, Serialize, Deserialize)]
struct RuleStoreDocument {
    version: u16,
    rules: Vec<RewriteRule>,
}

impl RuleStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn load(&self) -> Result<Vec<RewriteRule>, RuleStoreError> {
        let bytes = match tokio::fs::read(&self.path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        if bytes.len() > MAX_RULE_STORE_BYTES {
            return Err(RuleStoreError::TooLarge {
                actual: bytes.len(),
                maximum: MAX_RULE_STORE_BYTES,
            });
        }

        let document: RuleStoreDocument = serde_json::from_slice(&bytes)?;
        if document.version != RULE_STORE_VERSION {
            return Err(RuleStoreError::UnsupportedVersion(document.version));
        }
        Ok(document.rules)
    }

    pub async fn save(&self, rules: &[RewriteRule]) -> Result<(), RuleStoreError> {
        let _guard = self.write_lock.lock().await;
        let document = RuleStoreDocument {
            version: RULE_STORE_VERSION,
            rules: rules.to_vec(),
        };
        let bytes = serde_json::to_vec_pretty(&document)?;
        if bytes.len() > MAX_RULE_STORE_BYTES {
            return Err(RuleStoreError::TooLarge {
                actual: bytes.len(),
                maximum: MAX_RULE_STORE_BYTES,
            });
        }

        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        tokio::fs::create_dir_all(parent).await?;
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("rewrite-rules.json");
        let temporary = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));

        tokio::fs::write(&temporary, &bytes).await?;
        set_private_permissions(&temporary).await?;
        if let Err(error) = tokio::fs::rename(&temporary, &self.path).await {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(error.into());
        }
        set_private_permissions(&self.path).await?;
        Ok(())
    }
}

#[cfg(unix)]
async fn set_private_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let permissions = std::fs::Permissions::from_mode(0o600);
    tokio::fs::set_permissions(path, permissions).await
}

#[cfg(not(unix))]
async fn set_private_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use proxy_core::rules::{ResponseRewriteAction, RewriteAction};

    use super::*;

    fn temporary_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("proxyman-clone-{name}-{}-{nonce}.json", std::process::id()))
    }

    fn sample_rule() -> RewriteRule {
        RewriteRule {
            id: "example".into(),
            enabled: true,
            host_contains: Some("example.com".into()),
            path_prefix: Some("/api".into()),
            actions: vec![RewriteAction::SetHeader {
                name: "X-Debug".into(),
                value: "1".into(),
            }],
            response_actions: vec![ResponseRewriteAction::SetStatus { value: 503 }],
        }
    }

    #[tokio::test]
    async fn round_trip_preserves_rules() {
        let path = temporary_path("round-trip");
        let store = RuleStore::new(path.clone());
        let rules = vec![sample_rule()];
        store.save(&rules).await.unwrap();
        assert_eq!(store.load().await.unwrap(), rules);
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn missing_file_loads_as_empty() {
        let store = RuleStore::new(temporary_path("missing"));
        assert!(store.load().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn corrupt_json_is_rejected() {
        let path = temporary_path("corrupt");
        tokio::fs::write(&path, b"not-json").await.unwrap();
        let store = RuleStore::new(path.clone());
        assert!(matches!(store.load().await, Err(RuleStoreError::Json(_))));
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn unsupported_version_is_rejected() {
        let path = temporary_path("version");
        tokio::fs::write(&path, br#"{"version":999,"rules":[]}"#)
            .await
            .unwrap();
        let store = RuleStore::new(path.clone());
        assert!(matches!(
            store.load().await,
            Err(RuleStoreError::UnsupportedVersion(999))
        ));
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn oversized_file_is_rejected_before_json_parse() {
        let path = temporary_path("oversized");
        tokio::fs::write(&path, vec![b' '; MAX_RULE_STORE_BYTES + 1])
            .await
            .unwrap();
        let store = RuleStore::new(path.clone());
        assert!(matches!(store.load().await, Err(RuleStoreError::TooLarge { .. })));
        let _ = tokio::fs::remove_file(path).await;
    }
}
