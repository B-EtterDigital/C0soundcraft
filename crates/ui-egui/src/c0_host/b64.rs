//! Standard base64, decoding only (`control::base64` encodes): the app has no base64 dependency.

/// Decodes standard-alphabet base64, padded or not; ASCII whitespace is skipped. `None` for any
/// other byte, a misplaced `=`, or a length no encoder produces.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3 + 3);
    let (mut acc, mut bits, mut pad) = (0u32, 0u32, 0u32);
    for b in text.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                pad += 1;
                continue;
            }
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        if pad > 0 {
            return None;
        }
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from(acc >> bits).ok()?);
            acc &= (1 << bits) - 1;
        }
    }
    // A single leftover character carries 6 bits: no encoder writes that. More than two `=` is wrong too.
    (bits < 6 && pad <= 2).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn round_trips_with_the_encoder() {
        for len in 0..40usize {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 % 251) as u8).collect();
            let text = crate::control::base64(&data);
            assert_eq!(decode(&text).as_deref(), Some(data.as_slice()), "{len}");
            assert_eq!(decode(text.trim_end_matches('=')).as_deref(), Some(data.as_slice()), "{len} unpadded");
        }
    }

    #[test]
    fn refuses_what_is_not_base64() {
        for bad in ["!!!!", "AAAA=AAA", "A", "AAAAA", "AA===", "é"] {
            assert!(decode(bad).is_none(), "{bad:?}");
        }
        assert_eq!(decode("QUJD\nREVG").as_deref(), Some(b"ABCDEF".as_slice()));
    }
}
