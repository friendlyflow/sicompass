//! SHA-256 verification + optional ed25519 signature checks for downloaded
//! plugin entry files.
//!
//! v1 ships SHA-256 enforcement plus trust-on-first-use ed25519 verification:
//! a plugin's currently-installed manifest may embed an ed25519 public key,
//! and any update served for that plugin must carry a signature verifiable
//! against the embedded key. The very first install has no embedded key to
//! check against — that's accepted, with the understanding that the install
//! itself was done out-of-band by the user.

use base64::Engine;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

/// Compute hex-encoded SHA-256 of a file. Returns an error if the file
/// can't be read.
pub fn sha256_hex_of(path: &Path) -> std::io::Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Verify a downloaded entry file matches the served manifest's `sha256`.
/// Optionally also verifies an ed25519 signature when the **currently
/// installed** plugin manifest embedded a public key — that key is the
/// trust root, not the one served in the new manifest. The new manifest
/// may also publish its own (rotated) key, but the rotation is only
/// honored if the old key signed the new key — left as a follow-up.
///
/// Returns `Ok(())` on success or `Err(reason)` on any mismatch.
pub fn verify_entry(
    entry_path: &Path,
    expected_sha256_hex: &str,
    trusted_pubkey_b64: Option<&str>,
    sig_b64: Option<&str>,
) -> Result<(), String> {
    let got = sha256_hex_of(entry_path).map_err(|e| format!("hash {}: {e}", entry_path.display()))?;
    if !got.eq_ignore_ascii_case(expected_sha256_hex) {
        return Err(format!(
            "sha256 mismatch: expected {expected_sha256_hex}, got {got}"
        ));
    }

    // ed25519 verification only kicks in when the trust root (the
    // currently-installed plugin's pubkey) is present AND the served
    // manifest carries a signature. If the trust root exists but the
    // signature is absent, that's a rollback to unsigned — reject it.
    if let Some(pk_b64) = trusted_pubkey_b64 {
        let Some(sig_b64) = sig_b64 else {
            return Err("trusted pubkey present but update has no signature".into());
        };
        verify_ed25519(entry_path, pk_b64, sig_b64)?;
    }

    Ok(())
}

fn verify_ed25519(entry_path: &Path, pk_b64: &str, sig_b64: &str) -> Result<(), String> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let pk_bytes = B64.decode(pk_b64).map_err(|e| format!("pubkey base64: {e}"))?;
    let pk_bytes: [u8; 32] = pk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "pubkey must be 32 bytes".to_string())?;
    let pk = VerifyingKey::from_bytes(&pk_bytes).map_err(|e| format!("pubkey: {e}"))?;

    let sig_bytes = B64.decode(sig_b64).map_err(|e| format!("sig base64: {e}"))?;
    let sig_bytes: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "sig must be 64 bytes".to_string())?;
    let sig = Signature::from_bytes(&sig_bytes);

    let mut data = Vec::new();
    File::open(entry_path)
        .map_err(|e| format!("open {}: {e}", entry_path.display()))?
        .read_to_end(&mut data)
        .map_err(|e| format!("read {}: {e}", entry_path.display()))?;

    pk.verify(&data, &sig).map_err(|e| format!("ed25519 verify: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_file(content: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn sha256_matches_known_vector() {
        let f = write_file(b"abc");
        let got = sha256_hex_of(f.path()).unwrap();
        // Known SHA-256("abc")
        assert_eq!(got, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn verify_entry_passes_with_correct_hash() {
        let f = write_file(b"abc");
        let r = verify_entry(
            f.path(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            None,
            None,
        );
        assert!(r.is_ok());
    }

    #[test]
    fn verify_entry_case_insensitive() {
        let f = write_file(b"abc");
        let r = verify_entry(
            f.path(),
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD",
            None,
            None,
        );
        assert!(r.is_ok());
    }

    #[test]
    fn verify_entry_rejects_wrong_hash() {
        let f = write_file(b"abc");
        let r = verify_entry(f.path(), "00".repeat(32).as_str(), None, None);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("sha256 mismatch"));
    }

    #[test]
    fn verify_entry_requires_sig_when_trusted_pubkey_present() {
        let f = write_file(b"abc");
        let r = verify_entry(
            f.path(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
            None,
        );
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("no signature"));
    }

    #[test]
    fn verify_entry_with_valid_ed25519_signature() {
        use ed25519_dalek::{Signer, SigningKey};

        // Deterministic seed so tests don't depend on OS entropy.
        let seed = [7u8; 32];
        let sk = SigningKey::from_bytes(&seed);
        let pk = sk.verifying_key();

        let content = b"hello plugin world";
        let f = write_file(content);
        let sig = sk.sign(content);

        let sha = sha256_hex_of(f.path()).unwrap();
        let pk_b64 = B64.encode(pk.to_bytes());
        let sig_b64 = B64.encode(sig.to_bytes());

        verify_entry(f.path(), &sha, Some(&pk_b64), Some(&sig_b64)).expect("should verify");
    }

    #[test]
    fn verify_entry_rejects_wrong_signature() {
        use ed25519_dalek::{Signer, SigningKey};

        let sk = SigningKey::from_bytes(&[1u8; 32]);
        let pk = sk.verifying_key();
        let f = write_file(b"original");
        let sha = sha256_hex_of(f.path()).unwrap();
        // Sign different data than what's in the file — verification must fail.
        let bogus_sig = sk.sign(b"something else");

        let pk_b64 = B64.encode(pk.to_bytes());
        let sig_b64 = B64.encode(bogus_sig.to_bytes());
        let r = verify_entry(f.path(), &sha, Some(&pk_b64), Some(&sig_b64));
        assert!(r.is_err());
    }
}
