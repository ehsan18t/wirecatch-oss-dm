//! Checking yt-dlp's OpenPGP signature on a release's checksum list (D21.1).
//!
//! A newer yt-dlp is only installed when its `SHA2-256SUMS` carries a valid
//! detached signature (`SHA2-256SUMS.sig`) from yt-dlp's release signing key,
//! which is embedded in the app and pinned by its full fingerprint. A release
//! made by anyone who can publish on yt-dlp's GitHub page, but who does not
//! hold that key, is refused.

use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
use pgp::packet::SignatureType;
use pgp::types::KeyDetails;

/// yt-dlp's release signing key ("Simon Sawicki (yt-dlp signing key)
/// <contact@grub4k.xyz>", RSA 4096, created 2023-02-26, no expiry).
///
/// Taken from `public.key` in github.com/yt-dlp/yt-dlp (master), which
/// yt-dlp's README names as the key for its `SHA2-256SUMS.sig` files. The
/// fingerprint was cross-checked on 2026-10-11 against two independent
/// sources: keys.openpgp.org (`/vks/v1/by-fingerprint/AC0C...7581`, served
/// with the e-mail address verified by that keyserver) and
/// keyserver.ubuntu.com, which both return this key with the same fingerprint,
/// and the key verifies the real signature on yt-dlp 2026.08.19's checksum
/// list (`gpg --verify`; also a test below).
const YTDLP_PUBLIC_KEY: &str = include_str!("ytdlp-signing-key.asc");

/// The full fingerprint of [`YTDLP_PUBLIC_KEY`]'s primary key.
const YTDLP_FINGERPRINT: &str = "AC0CBBE6848D6A873464AF4E57CF65933B5A7581";

/// An OpenPGP public key trusted for one purpose, pinned by its full
/// fingerprint: a key whose fingerprint differs is never used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedKey {
    /// The public key, ASCII-armored.
    pub armored: String,
    /// The primary key's full fingerprint, in hex (spaces and case do not
    /// matter).
    pub fingerprint: String,
}

impl PinnedKey {
    /// yt-dlp's release signing key, embedded in the app.
    pub fn ytdlp() -> Self {
        Self {
            armored: YTDLP_PUBLIC_KEY.into(),
            fingerprint: YTDLP_FINGERPRINT.into(),
        }
    }
}

/// Checks that `signature` (a detached OpenPGP signature, binary or armored)
/// is a valid signature over `data` by `key`'s primary key, and that the key
/// is the pinned one. `Err` says why not, as a short phrase.
pub(crate) fn verify_detached(
    key: &PinnedKey,
    data: &[u8],
    signature: &[u8],
) -> Result<(), String> {
    let (public, _) = SignedPublicKey::from_string(&key.armored)
        .map_err(|e| format!("the signing key cannot be read ({e})"))?;
    let pinned: String = key
        .fingerprint
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    let actual = format!("{:X}", public.fingerprint());
    if actual != pinned {
        return Err(format!(
            "the signing key's fingerprint is {actual}, not the pinned {pinned}"
        ));
    }
    public
        .verify_bindings()
        .map_err(|e| format!("the signing key is not valid ({e})"))?;
    let (signatures, _) = DetachedSignature::from_reader_many(signature)
        .map_err(|_| "the signature file is not an OpenPGP signature".to_string())?;
    let mut seen = false;
    for parsed in signatures {
        let Ok(parsed) = parsed else {
            continue;
        };
        seen = true;
        // Only a signature over a document counts, not (say) a key
        // certification that happens to verify.
        if !matches!(
            parsed.signature.typ(),
            Some(SignatureType::Binary | SignatureType::Text)
        ) {
            continue;
        }
        if parsed.verify(&public.primary_key, data).is_ok() {
            return Ok(());
        }
    }
    if seen {
        Err("the signature does not match the checksum list and the pinned key".into())
    } else {
        Err("the signature file is not an OpenPGP signature".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pgp::composed::{KeyType, SecretKeyParamsBuilder, SignedSecretKey};
    use pgp::crypto::hash::HashAlgorithm;
    use pgp::ser::Serialize;
    use pgp::types::Password;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    const SUMS: &[u8] = include_bytes!("testdata/ytdlp-2026.08.19-SHA2-256SUMS");
    const SUMS_SIG: &[u8] = include_bytes!("testdata/ytdlp-2026.08.19-SHA2-256SUMS.sig");

    /// A throwaway key made for the test; never the real key's private part.
    fn throwaway(seed: u64) -> (SignedSecretKey, PinnedKey) {
        let mut params = SecretKeyParamsBuilder::default();
        params
            .key_type(KeyType::Ed25519Legacy)
            .can_certify(true)
            .can_sign(true)
            .primary_user_id("Test signer <test@example.invalid>".into());
        let secret = params
            .build()
            .unwrap()
            .generate(StdRng::seed_from_u64(seed))
            .unwrap();
        let public = SignedPublicKey::from(secret.clone());
        let pinned = PinnedKey {
            armored: public.to_armored_string(Default::default()).unwrap(),
            fingerprint: format!("{:X}", public.fingerprint()),
        };
        (secret, pinned)
    }

    fn sign(secret: &SignedSecretKey, data: &[u8]) -> Vec<u8> {
        DetachedSignature::sign_binary_data(
            StdRng::seed_from_u64(7),
            &secret.primary_key,
            &Password::empty(),
            HashAlgorithm::Sha256,
            data,
        )
        .unwrap()
        .to_bytes()
        .unwrap()
    }

    #[test]
    fn the_embedded_key_is_the_pinned_one_and_verifies_a_real_release() {
        let key = PinnedKey::ytdlp();
        verify_detached(&key, SUMS, SUMS_SIG).unwrap();
        // The pinned yt-dlp in the app is in that signed list.
        let official = super::super::manifest::MediaAddonSources::official();
        let listed =
            super::super::checksum_for(std::str::from_utf8(SUMS).unwrap(), &official.ytdlp_asset);
        assert_eq!(listed.as_deref(), Some(official.ytdlp.sha256.as_str()));
        assert_eq!(official.ytdlp.version, "2026.08.19");
        // One changed byte in the list breaks the signature.
        let mut changed = SUMS.to_vec();
        changed[0] ^= 1;
        assert!(verify_detached(&key, &changed, SUMS_SIG).is_err());
    }

    #[test]
    fn a_good_signature_verifies() {
        let (secret, key) = throwaway(1);
        let data = b"abc  yt-dlp\n";
        verify_detached(&key, data, &sign(&secret, data)).unwrap();
        // Armored works too.
        let armored = DetachedSignature::sign_binary_data(
            StdRng::seed_from_u64(8),
            &secret.primary_key,
            &Password::empty(),
            HashAlgorithm::Sha256,
            &data[..],
        )
        .unwrap()
        .to_armored_bytes(Default::default())
        .unwrap();
        verify_detached(&key, data, &armored).unwrap();
    }

    #[test]
    fn a_signature_over_other_data_is_refused() {
        let (secret, key) = throwaway(2);
        let signature = sign(&secret, b"abc  yt-dlp\n");
        let error = verify_detached(&key, b"evil  yt-dlp\n", &signature).unwrap_err();
        assert!(error.contains("does not match"), "{error}");
    }

    #[test]
    fn a_missing_or_garbled_signature_is_refused() {
        let (_, key) = throwaway(3);
        for signature in [
            &b""[..],
            b"not a signature",
            b"-----BEGIN PGP SIGNATURE-----\n",
        ] {
            let error = verify_detached(&key, b"abc", signature).unwrap_err();
            assert!(error.contains("not an OpenPGP signature"), "{error}");
        }
    }

    #[test]
    fn a_signature_by_another_key_is_refused() {
        let (_, key) = throwaway(4);
        let (other, _) = throwaway(5);
        let data = b"abc  yt-dlp\n";
        assert!(verify_detached(&key, data, &sign(&other, data)).is_err());
        // The real key refuses the throwaway one's signature as well.
        assert!(verify_detached(&PinnedKey::ytdlp(), data, &sign(&other, data)).is_err());
    }

    #[test]
    fn a_key_that_is_not_the_pinned_one_is_never_used() {
        let (secret, mut key) = throwaway(6);
        let data = b"abc  yt-dlp\n";
        let signature = sign(&secret, data);
        // The right key, but pinned to another fingerprint (for example the
        // embedded key file swapped for an attacker's).
        key.fingerprint = YTDLP_FINGERPRINT.into();
        let error = verify_detached(&key, data, &signature).unwrap_err();
        assert!(error.contains("fingerprint"), "{error}");
        // Spaces and lower case in a pinned fingerprint are fine.
        let (_, mut spaced) = throwaway(6);
        spaced.fingerprint = spaced
            .fingerprint
            .to_ascii_lowercase()
            .as_bytes()
            .chunks(4)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join(" ");
        verify_detached(&spaced, data, &signature).unwrap();
    }
}
