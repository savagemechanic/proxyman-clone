use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::{fs, sync::RwLock};

const CA_CERT_FILE: &str = "ca-cert.pem";
const CA_KEY_FILE: &str = "ca-key.pem";

#[derive(Clone)]
pub struct CertificateAuthority {
    inner: Arc<Inner>,
}

struct Inner {
    ca_cert: Certificate,
    ca_key: KeyPair,
    ca_cert_pem: String,
    storage_dir: PathBuf,
    cache: RwLock<HashMap<String, Arc<LeafIdentity>>>,
}

#[derive(Debug)]
pub struct LeafIdentity {
    pub cert_chain: Vec<CertificateDer<'static>>,
    pub private_key: PrivateKeyDer<'static>,
}

impl Clone for LeafIdentity {
    fn clone(&self) -> Self {
        Self {
            cert_chain: self.cert_chain.clone(),
            private_key: self.private_key.clone_key(),
        }
    }
}

impl CertificateAuthority {
    pub async fn load_or_create(storage_dir: impl Into<PathBuf>) -> Result<Self> {
        let storage_dir = storage_dir.into();
        fs::create_dir_all(&storage_dir)
            .await
            .with_context(|| format!("failed to create CA directory {}", storage_dir.display()))?;

        let cert_path = storage_dir.join(CA_CERT_FILE);
        let key_path = storage_dir.join(CA_KEY_FILE);

        let (ca_cert, ca_key, ca_cert_pem) = if cert_path.exists() && key_path.exists() {
            let cert_pem = fs::read_to_string(&cert_path)
                .await
                .context("failed to read local CA certificate")?;
            let key_pem = fs::read_to_string(&key_path)
                .await
                .context("failed to read local CA private key")?;
            let key = KeyPair::from_pem(&key_pem).context("failed to parse local CA key")?;
            let params = CertificateParams::from_ca_cert_pem(&cert_pem)
                .context("failed to parse local CA certificate")?;
            let cert = params
                .self_signed(&key)
                .context("failed to reconstruct local CA")?;
            (cert, key, cert_pem)
        } else {
            let key = KeyPair::generate().context("failed to generate CA private key")?;
            let mut params = CertificateParams::new(Vec::<String>::new())?;
            params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
            let mut dn = DistinguishedName::new();
            dn.push(DnType::CommonName, "Proxyman Clone Local Development CA");
            dn.push(DnType::OrganizationName, "Proxyman Clone");
            params.distinguished_name = dn;
            let cert = params
                .self_signed(&key)
                .context("failed to self-sign local CA")?;
            let cert_pem = cert.pem();
            let key_pem = key.serialize_pem();
            write_private_file(&key_path, key_pem.as_bytes()).await?;
            fs::write(&cert_path, cert_pem.as_bytes())
                .await
                .context("failed to persist local CA certificate")?;
            (cert, key, cert_pem)
        };

        Ok(Self {
            inner: Arc::new(Inner {
                ca_cert,
                ca_key,
                ca_cert_pem,
                storage_dir,
                cache: RwLock::new(HashMap::new()),
            }),
        })
    }

    pub fn certificate_path(&self) -> PathBuf {
        self.inner.storage_dir.join(CA_CERT_FILE)
    }

    pub fn certificate_pem(&self) -> &str {
        &self.inner.ca_cert_pem
    }

    pub async fn identity_for_host(&self, host: &str) -> Result<Arc<LeafIdentity>> {
        if let Some(identity) = self.inner.cache.read().await.get(host).cloned() {
            return Ok(identity);
        }

        let leaf_key = KeyPair::generate().context("failed to generate leaf private key")?;
        let mut params = CertificateParams::new(vec![host.to_owned()])?;
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, host);
        params.distinguished_name = dn;
        let leaf_cert = params
            .signed_by(&leaf_key, &self.inner.ca_cert, &self.inner.ca_key)
            .context("failed to sign leaf certificate")?;

        let identity = Arc::new(LeafIdentity {
            cert_chain: vec![leaf_cert.der().clone()],
            private_key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
        });
        self.inner
            .cache
            .write()
            .await
            .insert(host.to_owned(), Arc::clone(&identity));
        Ok(identity)
    }
}

async fn write_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    fs::write(path, contents)
        .await
        .with_context(|| format!("failed to persist private key {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .await
            .with_context(|| format!("failed to restrict permissions on {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn generates_distinct_leaf_identities_and_caches_by_host() {
        let dir =
            std::env::temp_dir().join(format!("proxyman-clone-ca-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir).await;
        let ca = CertificateAuthority::load_or_create(&dir).await.unwrap();
        let first = ca.identity_for_host("example.com").await.unwrap();
        let second = ca.identity_for_host("example.com").await.unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert!(ca.certificate_path().exists());
        assert!(ca.certificate_pem().contains("BEGIN CERTIFICATE"));
        let _ = fs::remove_dir_all(&dir).await;
    }
}
