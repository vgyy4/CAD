//! Server identity for the stdio bridge (security review #13).
//!
//! A connection file can be stale (the app crashed) and its port taken by another local
//! process. Before the bridge sends anything that matters (the bearer token, the agent's
//! tool calls) it asks the port to prove it knows the token: it sends a fresh random
//! nonce to [`IDENTITY_PATH`] (no token) and checks the answer, a BLAKE3 keyed hash of
//! the nonce under the token. Every later request carries a new nonce in
//! [`NONCE_HEADER`] and every response must carry the matching [`PROOF_HEADER`], so a
//! process that takes over the port mid-session cannot feed the agent responses.

/// Unauthenticated path that answers an identity challenge (Host/Origin checks apply).
pub const IDENTITY_PATH: &str = "/gusset/identity";
/// Request header with a 64-hex-digit random nonce.
pub const NONCE_HEADER: &str = "x-gusset-nonce";
/// Response header with [`proof`] of the request's nonce.
pub const PROOF_HEADER: &str = "x-gusset-proof";

const CONTEXT: &[u8] = b"gusset-mcp-identity-v1\0";

fn unhex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        let pair = s.get(2 * i..2 * i + 2)?;
        *o = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

/// The proof for `nonce_hex` under `token_hex` (both 64 hex digits), as hex. `None` if
/// either is malformed.
#[must_use]
pub fn proof(token_hex: &str, nonce_hex: &str) -> Option<String> {
    let key = unhex32(token_hex)?;
    let nonce = unhex32(nonce_hex)?;
    let mut h = blake3::Hasher::new_keyed(&key);
    h.update(CONTEXT);
    h.update(&nonce);
    Some(h.finalize().to_hex().to_string())
}

/// A fresh 256-bit nonce as hex.
#[must_use]
pub fn new_nonce() -> Option<String> {
    crate::approval::random_hex(32)
}

/// Whether `answer` proves knowledge of `token_hex` for `nonce_hex` (constant time).
#[must_use]
pub fn verify(token_hex: &str, nonce_hex: &str, answer: &str) -> bool {
    proof(token_hex, nonce_hex).is_some_and(|p| crate::http::ct_eq(p.as_bytes(), answer.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proofs_bind_token_and_nonce() {
        let t = "11".repeat(32);
        let n = new_nonce().unwrap();
        let p = proof(&t, &n).unwrap();
        assert!(verify(&t, &n, &p));
        assert!(!verify(&"22".repeat(32), &n, &p), "another token");
        assert!(!verify(&t, &new_nonce().unwrap(), &p), "another nonce");
        assert!(proof("zz", &n).is_none());
        assert!(!verify(&t, &n, ""));
    }
}
