use crate::{Result, ServingError as E, atomic, private_dir};
use devforge_core::SecretStore;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, GeneralSubtree, IsCa, Issuer,
    KeyPair, KeyUsagePurpose, NameConstraints,
};
use std::{
    fs,
    path::{Path, PathBuf},
};
use time::{Duration, OffsetDateTime};
use zeroize::Zeroizing;
pub struct Authority {
    owner: String,
    root: PathBuf,
    der: Vec<u8>,
    issuer: Issuer<'static, KeyPair>,
}
#[derive(Clone)]
pub struct Leaf {
    pub certificate: PathBuf,
    pub key: PathBuf,
}
impl Authority {
    pub fn open(root: impl AsRef<Path>, owner: &str, secrets: &dyn SecretStore) -> Result<Self> {
        if !crate::site::owner(owner) {
            return Err(E::AuthorityInvalid);
        }
        private_dir(root.as_ref())?;
        let root = fs::canonicalize(root)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("authority.lock"))?;
        lock.try_lock().map_err(|_| E::AuthorityInvalid)?;
        let reference = format!("serving/{owner}/authority-key");
        let certificate = root.join("ca.der");
        let stored = secrets
            .get(&reference)
            .map_err(|_| E::SecretUnavailable)?
            .map(Zeroizing::new);
        let der;
        let key = if certificate.exists() {
            der = fs::read(&certificate)?;
            validate_root(&der, owner)?;
            KeyPair::from_pem(&stored.ok_or(E::SecretUnavailable)?)
                .map_err(|_| E::AuthorityInvalid)?
        } else {
            if stored.is_some() {
                return Err(E::AuthorityInvalid);
            }
            let key = KeyPair::generate().map_err(|_| E::AuthorityInvalid)?;
            let mut params =
                CertificateParams::new(Vec::<String>::new()).map_err(|_| E::AuthorityInvalid)?;
            let mut dn = DistinguishedName::new();
            dn.push(
                DnType::CommonName,
                format!("DevForge Development CA {owner}"),
            );
            dn.push(DnType::OrganizationName, "DevForge");
            params.distinguished_name = dn;
            params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
            params.key_usages = vec![
                KeyUsagePurpose::KeyCertSign,
                KeyUsagePurpose::CrlSign,
                KeyUsagePurpose::DigitalSignature,
            ];
            params.name_constraints = Some(NameConstraints {
                permitted_subtrees: vec![
                    GeneralSubtree::DnsName("devforge.test".into()),
                    GeneralSubtree::DnsName("localhost".into()),
                ],
                excluded_subtrees: Vec::new(),
            });
            params.not_before = OffsetDateTime::now_utc() - Duration::days(1);
            params.not_after = OffsetDateTime::now_utc() + Duration::days(3650);
            let cert = params.self_signed(&key).map_err(|_| E::AuthorityInvalid)?;
            der = cert.der().to_vec();
            let secret = Zeroizing::new(key.serialize_pem());
            secrets
                .set(&reference, &secret)
                .map_err(|_| E::SecretUnavailable)?;
            atomic(&certificate, &der)?;
            atomic(&root.join("ca.pem"), cert.pem().as_bytes())?;
            key
        };
        let (_, cert) =
            x509_parser::parse_x509_certificate(&der).map_err(|_| E::AuthorityInvalid)?;
        if cert.public_key().subject_public_key.data.as_ref() != key.public_key_raw() {
            return Err(E::AuthorityInvalid);
        }
        let issuer = Issuer::from_ca_cert_der(&der.as_slice().into(), key)
            .map_err(|_| E::AuthorityInvalid)?;
        Ok(Self {
            owner: owner.into(),
            root,
            der,
            issuer,
        })
    }
    pub fn der(&self) -> &[u8] {
        &self.der
    }
    pub fn owner(&self) -> &str {
        &self.owner
    }
    pub fn fingerprint(&self) -> String {
        crate::hash(&self.der)
    }
    pub fn pem(&self) -> String {
        pem::encode(&pem::Pem::new("CERTIFICATE", self.der.clone()))
    }
    pub fn issue(&self, session: &Path, names: &[String]) -> Result<Leaf> {
        if names.is_empty()
            || names.len() > 8
            || names.iter().any(|n| !crate::site::certificate_name(n))
        {
            return Err(E::InvalidDomain);
        }
        let mut params = CertificateParams::new(names.to_vec()).map_err(|_| E::AuthorityInvalid)?;
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, names[0].clone());
        params.not_before = OffsetDateTime::now_utc() - Duration::days(1);
        params.not_after = OffsetDateTime::now_utc() + Duration::days(30);
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        let key = KeyPair::generate().map_err(|_| E::AuthorityInvalid)?;
        let cert = params
            .signed_by(&key, &self.issuer)
            .map_err(|_| E::AuthorityInvalid)?;
        private_dir(session)?;
        let certificate = session.join("leaf.pem");
        let path = session.join("leaf-key.pem");
        atomic(&certificate, cert.pem().as_bytes())?;
        atomic(&path, Zeroizing::new(key.serialize_pem()).as_bytes())?;
        Ok(Leaf {
            certificate,
            key: path,
        })
    }
    pub fn directory(&self) -> &Path {
        &self.root
    }
}
pub fn validate_root(der: &[u8], owner: &str) -> Result<()> {
    if !crate::site::owner(owner) || der.len() > 16 * 1024 {
        return Err(E::AuthorityInvalid);
    }
    let (rest, cert) = x509_parser::parse_x509_certificate(der).map_err(|_| E::AuthorityInvalid)?;
    if !rest.is_empty() || cert.issuer() != cert.subject() || !cert.is_ca() {
        return Err(E::AuthorityInvalid);
    }
    if !cert.validity().is_valid() {
        return Err(E::CertificateExpired);
    }
    let name = cert
        .subject()
        .iter_common_name()
        .next()
        .and_then(|a| a.as_str().ok())
        .ok_or(E::AuthorityInvalid)?;
    if name != format!("DevForge Development CA {owner}") {
        return Err(E::AuthorityInvalid);
    }
    cert.verify_signature(None)
        .map_err(|_| E::AuthorityInvalid)?;
    let constraints = cert
        .name_constraints()
        .map_err(|_| E::AuthorityInvalid)?
        .ok_or(E::AuthorityInvalid)?;
    if !constraints.critical || constraints.value.excluded_subtrees.is_some() {
        return Err(E::AuthorityInvalid);
    }
    let permitted = constraints
        .value
        .permitted_subtrees
        .as_ref()
        .ok_or(E::AuthorityInvalid)?;
    let mut names = Vec::new();
    for item in permitted {
        if let x509_parser::extensions::GeneralName::DNSName(name) = item.base {
            names.push(name);
        } else {
            return Err(E::AuthorityInvalid);
        }
    }
    names.sort();
    if names != vec!["devforge.test", "localhost"] {
        return Err(E::AuthorityInvalid);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use devforge_core::ErrorCode;
    use std::{collections::BTreeMap, sync::Mutex};
    #[derive(Default)]
    struct Secrets(Mutex<BTreeMap<String, String>>);
    impl SecretStore for Secrets {
        fn get(&self, k: &str) -> std::result::Result<Option<String>, ErrorCode> {
            Ok(self.0.lock().unwrap().get(k).cloned())
        }
        fn set(&self, k: &str, v: &str) -> std::result::Result<(), ErrorCode> {
            self.0.lock().unwrap().insert(k.into(), v.into());
            Ok(())
        }
        fn delete(&self, k: &str) -> std::result::Result<(), ErrorCode> {
            self.0.lock().unwrap().remove(k);
            Ok(())
        }
    }
    #[test]
    fn persistent_authority_is_scoped_and_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = Secrets::default();
        let owner = crate::id();
        let ca = Authority::open(dir.path(), &owner, &secrets).unwrap();
        let fingerprint = ca.fingerprint();
        validate_root(ca.der(), &owner).unwrap();
        assert!(
            ca.issue(&dir.path().join("leaf"), &["example.com".into()])
                .is_err()
        );
        let leaf = ca
            .issue(
                &dir.path().join("leaf"),
                &["a.devforge.test".into(), "a.localhost".into()],
            )
            .unwrap();
        assert!(leaf.key.is_file());
        drop(ca);
        assert_eq!(
            Authority::open(dir.path(), &owner, &secrets)
                .unwrap()
                .fingerprint(),
            fingerprint
        );
        assert!(Authority::open(dir.path(), &crate::id(), &secrets).is_err());
    }
}
