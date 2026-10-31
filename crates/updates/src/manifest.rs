//! The list of a release's installers, signed by the release workflow with the key whose public
//! half the app carries.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

/// A release's installers, as the release workflow lists them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The release's version, without a leading `v`.
    pub version: String,
    pub files: Vec<UpdateFile>,
}

/// One installer of a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateFile {
    /// The release asset's name.
    pub name: String,
    pub system: System,
    pub arch: Arch,
    pub kind: FileKind,
    pub size: u64,
    /// Lowercase hexadecimal.
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum System {
    Windows,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    X64,
    Arm64,
}

/// What an installer is, which decides how it is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    /// The Windows setup program.
    Setup,
    AppImage,
    Deb,
    Rpm,
}

/// Why a manifest is not trusted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    #[error("this build has no key to check updates with")]
    NoKey,
    #[error("the update's signature does not match")]
    BadSignature,
    #[error("the list of the update's files cannot be read ({0})")]
    Unreadable(String),
}

impl System {
    /// The system this build runs on, if updates know it.
    #[must_use]
    pub const fn current() -> Option<Self> {
        if cfg!(windows) {
            Some(Self::Windows)
        } else if cfg!(target_os = "linux") {
            Some(Self::Linux)
        } else {
            None
        }
    }
}

impl Arch {
    /// The architecture this build runs on, if updates know it.
    #[must_use]
    pub const fn current() -> Option<Self> {
        if cfg!(target_arch = "x86_64") {
            Some(Self::X64)
        } else if cfg!(target_arch = "aarch64") {
            Some(Self::Arm64)
        } else {
            None
        }
    }
}

impl Manifest {
    /// The manifest in `bytes`, once `signature` proves `public_key` signed exactly those bytes.
    /// Keys and signatures are unpadded base64url.
    ///
    /// # Errors
    ///
    /// [`ManifestError::NoKey`] for a key that is not one, [`ManifestError::BadSignature`], or
    /// [`ManifestError::Unreadable`].
    pub fn verified(
        bytes: &[u8],
        signature: &str,
        public_key: &str,
    ) -> Result<Self, ManifestError> {
        let key = URL_SAFE_NO_PAD
            .decode(public_key)
            .ok()
            .and_then(|key| <[u8; 32]>::try_from(key).ok())
            .and_then(|key| VerifyingKey::from_bytes(&key).ok())
            .ok_or(ManifestError::NoKey)?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature.trim())
            .ok()
            .and_then(|bytes| Signature::from_slice(&bytes).ok())
            .ok_or(ManifestError::BadSignature)?;
        key.verify_strict(bytes, &signature)
            .map_err(|_| ManifestError::BadSignature)?;
        let manifest: Self =
            serde_json::from_slice(bytes).map_err(|e| ManifestError::Unreadable(e.to_string()))?;
        if let Some(file) = manifest.files.iter().find(|file| !is_sha256(&file.sha256)) {
            return Err(ManifestError::Unreadable(format!(
                "{} has no SHA-256",
                file.name
            )));
        }
        Ok(manifest)
    }

    /// The installer of `kind` for `system` on `arch`.
    #[must_use]
    pub fn file(&self, system: System, arch: Arch, kind: FileKind) -> Option<&UpdateFile> {
        self.files
            .iter()
            .find(|file| file.system == system && file.arch == arch && file.kind == kind)
    }
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};

    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn public(key: &SigningKey) -> String {
        URL_SAFE_NO_PAD.encode(key.verifying_key().as_bytes())
    }

    fn signed(bytes: &[u8], key: &SigningKey) -> String {
        URL_SAFE_NO_PAD.encode(key.sign(bytes).to_bytes())
    }

    fn manifest_bytes(sha256: &str) -> Vec<u8> {
        format!(
            r#"{{"version":"0.2.0","files":[{{"name":"Wirecatch-0.2.0-windows-x64-setup.exe","system":"windows","arch":"x64","kind":"setup","size":10,"sha256":"{sha256}"}}]}}"#
        )
        .into_bytes()
    }

    #[test]
    fn a_manifest_signed_with_the_key_is_trusted() {
        let bytes = manifest_bytes(SHA);
        let manifest =
            Manifest::verified(&bytes, &signed(&bytes, &key()), &public(&key())).unwrap();
        assert_eq!(manifest.version, "0.2.0");
        let file = manifest
            .file(System::Windows, Arch::X64, FileKind::Setup)
            .unwrap();
        assert_eq!(file.size, 10);
        assert_eq!(
            manifest.file(System::Windows, Arch::Arm64, FileKind::Setup),
            None
        );
    }

    /// The manifest `scripts/release/manifest.mjs` writes, which its own test checks against
    /// the same file.
    #[test]
    fn the_release_workflow_manifest_is_read() {
        let bytes = include_bytes!("../samples/update.json");
        let manifest = Manifest::verified(bytes, &signed(bytes, &key()), &public(&key())).unwrap();
        assert_eq!(manifest.files.len(), 4);
        let appimage = manifest
            .file(System::Linux, Arch::Arm64, FileKind::AppImage)
            .unwrap();
        assert_eq!(appimage.size, 8);
    }

    #[test]
    fn a_changed_manifest_or_another_key_is_refused() {
        let bytes = manifest_bytes(SHA);
        let signature = signed(&bytes, &key());
        let mut changed = bytes.clone();
        changed[20] ^= 1;
        assert_eq!(
            Manifest::verified(&changed, &signature, &public(&key())),
            Err(ManifestError::BadSignature)
        );
        let other = SigningKey::from_bytes(&[8; 32]);
        assert_eq!(
            Manifest::verified(&bytes, &signature, &public(&other)),
            Err(ManifestError::BadSignature)
        );
        assert_eq!(
            Manifest::verified(&bytes, "not a signature", &public(&key())),
            Err(ManifestError::BadSignature)
        );
    }

    /// The product's key, the placeholder until the maintainer makes one, never trusts what
    /// another key signed.
    #[test]
    fn the_product_key_trusts_nothing_signed_elsewhere() {
        let bytes = manifest_bytes(SHA);
        let signature = signed(&bytes, &key());
        assert!(
            Manifest::verified(&bytes, &signature, wirecatch_product::UPDATER_PUBLIC_KEY).is_err()
        );
    }

    #[test]
    fn a_file_without_a_sha256_is_refused() {
        let bytes = manifest_bytes("ABC");
        assert!(matches!(
            Manifest::verified(&bytes, &signed(&bytes, &key()), &public(&key())),
            Err(ManifestError::Unreadable(_))
        ));
    }
}
