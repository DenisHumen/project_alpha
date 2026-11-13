// SPDX-License-Identifier: MPL-2.0
//! Hex encoding, length-prefixed transcripts for signatures, randomness.

use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// Lowercase hex of `bytes`.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[usize::from(b >> 4)] as char);
        out.push(HEX[usize::from(b & 0xf)] as char);
    }
    out
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

/// Bytes of lowercase hex text, at most `max` bytes.
pub(crate) fn from_hex(text: &str, max: usize) -> Result<Vec<u8>> {
    let raw = text.as_bytes();
    if raw.len() % 2 != 0 || raw.len() > max * 2 {
        return Err(Error::Invalid("hex"));
    }
    raw.chunks_exact(2)
        .map(|p| Some((nibble(p[0])? << 4) | nibble(p[1])?))
        .collect::<Option<Vec<u8>>>()
        .ok_or(Error::Invalid("hex"))
}

/// Exactly `N` bytes of lowercase hex text.
pub(crate) fn from_hex_array<const N: usize>(text: &str) -> Result<[u8; N]> {
    from_hex(text, N)?
        .try_into()
        .map_err(|_| Error::Invalid("hex length"))
}

/// `N` bytes from the OS random generator.
pub(crate) fn random<const N: usize>() -> Result<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).map_err(|e| Error::Random(e.to_string()))?;
    Ok(buf)
}

/// SHA-256 of `data`.
pub(crate) fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// Bytes covered by a signature: a domain tag, then each field with a
/// 4-byte big-endian length. Unambiguous without a canonical JSON form.
pub(crate) struct Transcript(Vec<u8>);

impl Transcript {
    pub(crate) fn new(domain: &str) -> Self {
        let mut t = Self(Vec::with_capacity(256));
        t.put(domain.as_bytes());
        t
    }

    pub(crate) fn put(&mut self, bytes: &[u8]) -> &mut Self {
        let len = u32::try_from(bytes.len()).expect("transcript field fits u32");
        self.0.extend_from_slice(&len.to_be_bytes());
        self.0.extend_from_slice(bytes);
        self
    }

    pub(crate) fn put_str(&mut self, s: &str) -> &mut Self {
        self.put(s.as_bytes())
    }

    pub(crate) fn put_u64(&mut self, n: u64) -> &mut Self {
        self.put(&n.to_be_bytes())
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Serde adapter: fixed-size byte arrays as lowercase hex strings.
pub(crate) mod hex_array {
    use serde::{Deserialize, Deserializer, Serializer};

    pub(crate) fn serialize<S: Serializer, const N: usize>(
        bytes: &[u8; N],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.serialize_str(&super::to_hex(bytes))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        d: D,
    ) -> Result<[u8; N], D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        super::from_hex_array(&text).map_err(serde::de::Error::custom)
    }
}

/// Whether `id` is a valid device id: `[A-Za-z0-9_-]{1,64}`, the same rule
/// as `pa_sync::hlc::is_valid_device_id`.
pub fn is_valid_device_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let bytes = [0u8, 1, 0xab, 0xff];
        assert_eq!(to_hex(&bytes), "0001abff");
        assert_eq!(from_hex("0001abff", 4).unwrap(), bytes);
        assert!(from_hex("0001abff", 3).is_err());
        assert!(from_hex("ABFF", 2).is_err());
        assert!(from_hex("abf", 2).is_err());
        assert!(from_hex_array::<2>("abff00").is_err());
    }

    #[test]
    fn transcript_is_length_prefixed() {
        let mut a = Transcript::new("d");
        a.put_str("ab").put_str("c");
        let mut b = Transcript::new("d");
        b.put_str("a").put_str("bc");
        assert_ne!(a.bytes(), b.bytes());
    }
}
