//! Constant-Time Authentication & Key Verification

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub fn verify_api_key(provided_key: &str, configured_keys: &[String]) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(provided_key.as_bytes());
    let provided_hash = hasher.finalize();

    for key in configured_keys {
        let mut key_hasher = Sha256::new();
        key_hasher.update(key.as_bytes());
        let expected_hash = key_hasher.finalize();

        if provided_hash.ct_eq(&expected_hash).into() {
            return true;
        }
    }
    false
}
