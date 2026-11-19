//! Secrets: random tokens and codes, and the hashes the database keeps of
//! them.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the system's random source");
    b
}

/// A bearer secret: 256 random bits as hex.
pub fn token() -> String {
    hex(&random_bytes::<32>())
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// What the database keeps of a high-entropy token.
pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// What the database keeps of a low-entropy secret (a code, a computer's
/// hash): keyed, so a stolen database can't be searched without the pepper.
pub fn keyed(pepper: &str, data: &str) -> String {
    let mut m = <HmacSha256 as KeyInit>::new_from_slice(pepper.as_bytes()).expect("any key length");
    m.update(data.as_bytes());
    hex(&m.finalize().into_bytes())
}

/// A top-up code as people type it, from fifteen random symbols.
pub fn new_code() -> String {
    let bytes = random_bytes::<{ as_cloud::code::SYMBOLS }>();
    as_cloud::code::format(&bytes)
}

/// A short account id people read out to support: `AS-XXXX-XXXX`.
pub fn support_id() -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let b = random_bytes::<8>();
    let s: String = b.iter().map(|v| char::from(ALPHABET[usize::from(v % 32)])).collect();
    format!("AS-{}-{}", &s[..4], &s[4..])
}

/// Whether `header` (`Stripe-Signature: t=…,v1=…`) signs `payload` with
/// `secret`, made within `tolerance` seconds of `now`.
pub fn stripe_signed(secret: &str, header: &str, payload: &[u8], now: i64, tolerance: i64) -> bool {
    let mut t = None;
    let mut sigs = Vec::new();
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", v)) => t = v.parse::<i64>().ok(),
            Some(("v1", v)) => sigs.push(v),
            _ => {}
        }
    }
    let Some(t) = t else { return false };
    if (now - t).abs() > tolerance {
        return false;
    }
    sigs.iter().any(|sig| {
        let Some(sig) = unhex(sig) else { return false };
        let mut m = <HmacSha256 as KeyInit>::new_from_slice(secret.as_bytes()).expect("any key length");
        m.update(t.to_string().as_bytes());
        m.update(b".");
        m.update(payload);
        m.verify_slice(&sig).is_ok()
    })
}

/// A `Stripe-Signature` header for `payload`, as Stripe makes it (tests).
pub fn stripe_sign(secret: &str, payload: &[u8], t: i64) -> String {
    let mut m = <HmacSha256 as KeyInit>::new_from_slice(secret.as_bytes()).expect("any key length");
    m.update(t.to_string().as_bytes());
    m.update(b".");
    m.update(payload);
    format!("t={t},v1={}", hex(&m.finalize().into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stripe_signature_holds_only_for_its_payload_secret_and_time() {
        let body = br#"{"id":"evt_1"}"#;
        let h = stripe_sign("whsec_x", body, 1_000);
        assert!(stripe_signed("whsec_x", &h, body, 1_100, 300));
        assert!(!stripe_signed("whsec_y", &h, body, 1_100, 300));
        assert!(!stripe_signed("whsec_x", &h, br#"{"id":"evt_2"}"#, 1_100, 300));
        assert!(!stripe_signed("whsec_x", &h, body, 2_000, 300));
        // Stripe may send several signatures while a secret rolls over.
        let both = format!("{h},v1={}", "00".repeat(32));
        assert!(stripe_signed("whsec_x", &both, body, 1_000, 300));
        assert!(!stripe_signed("whsec_x", "v1=abc", body, 1_000, 300));
    }

    #[test]
    fn codes_and_ids_have_their_shape() {
        let c = new_code();
        assert!(as_cloud::code::normalize(&c).is_some(), "{c}");
        let s = support_id();
        assert_eq!(s.len(), 12, "{s}");
        assert!(s.starts_with("AS-"));
        assert_ne!(token(), token());
        assert_eq!(unhex(&hex(&[0, 255, 16])).unwrap(), [0, 255, 16]);
        assert_ne!(keyed("a", "x"), keyed("b", "x"));
    }
}
