use axum::http::HeaderMap;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
pub struct Identity {
    pub user: String,
    pub nonce: String,
}
pub fn verify(
    key: &[u8],
    method: &str,
    path: &str,
    headers: &HeaderMap,
    body: &[u8],
    now: i64,
) -> Result<Identity, ()> {
    let value = |name: &str| headers.get(name).ok_or(())?.to_str().map_err(|_| ());
    let user = value("x-cc-user")?;
    let stamp = value("x-cc-timestamp")?;
    let nonce = value("x-cc-nonce")?;
    if user.is_empty()
        || user.len() > 128
        || !user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.'))
    {
        return Err(());
    }
    let time = stamp.parse::<i64>().map_err(|_| ())?;
    if time.abs_diff(now) > 60 || nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(());
    }
    let signature = hex::decode(value("x-cc-signature")?).map_err(|_| ())?;
    let message = format!(
        "{method}\n{path}\n{user}\n{stamp}\n{nonce}\n{}",
        hex::encode(Sha256::digest(body))
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| ())?;
    mac.update(message.as_bytes());
    mac.verify_slice(&signature).map_err(|_| ())?;
    Ok(Identity {
        user: user.into(),
        nonce: nonce.into(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn signed() -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in [
            ("x-cc-user", "google-123"),
            ("x-cc-timestamp", "1000"),
            ("x-cc-nonce", "00112233445566778899aabbccddeeff"),
        ] {
            h.insert(k, v.parse().unwrap());
        }
        let msg = format!(
            "POST\n/api/v1/projects\ngoogle-123\n1000\n00112233445566778899aabbccddeeff\n{}",
            hex::encode(Sha256::digest(b"{}"))
        );
        let mut mac = Hmac::<Sha256>::new_from_slice(&[7; 32]).unwrap();
        mac.update(msg.as_bytes());
        h.insert(
            "x-cc-signature",
            hex::encode(mac.finalize().into_bytes()).parse().unwrap(),
        );
        h
    }
    #[test]
    fn authentic_request() {
        assert!(verify(&[7; 32], "POST", "/api/v1/projects", &signed(), b"{}", 1000).is_ok());
    }
    #[test]
    fn tampering_rejected() {
        for (method, path, body) in [
            ("GET", "/api/v1/projects", b"{}".as_slice()),
            ("POST", "/api/v1/status", b"{}".as_slice()),
            ("POST", "/api/v1/projects", b"[]".as_slice()),
        ] {
            assert!(verify(&[7; 32], method, path, &signed(), body, 1000).is_err());
        }
    }
    #[test]
    fn expiration_and_wrong_key() {
        assert!(verify(&[7; 32], "POST", "/api/v1/projects", &signed(), b"{}", 1061).is_err());
        assert!(verify(&[8; 32], "POST", "/api/v1/projects", &signed(), b"{}", 1000).is_err());
    }
    #[test]
    fn identity_cannot_be_swapped() {
        let mut h = signed();
        h.insert("x-cc-user", "other-user".parse().unwrap());
        assert!(verify(&[7; 32], "POST", "/api/v1/projects", &h, b"{}", 1000).is_err());
    }
}
