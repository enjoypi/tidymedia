use sha2::{Digest, Sha512};

use crate::entities::SecureHash;

#[must_use]
pub(crate) fn sha512_bytes(bytes: &[u8]) -> SecureHash {
    let mut h = Sha512::new();
    h.update(bytes);
    h.finalize()
}
