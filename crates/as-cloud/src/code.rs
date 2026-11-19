//! Top-up codes as people type them: `XXXX-XXXX-XXXX-XXXX`, fifteen random
//! symbols of Crockford's base 32 (75 bits) and a check symbol that catches a
//! mistyped or swapped symbol before the gateway is asked.
//!
//! The randomness and the code's worth are the gateway's: a code is only
//! ever checked there.

/// Crockford's base 32: no I, L, O or U.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Random symbols in a code.
pub const SYMBOLS: usize = 15;

/// The check symbol of `values` (each below 32): a weighted sum modulo 31, so
/// any one changed symbol and any two swapped neighbours change it.
fn check(values: &[u8]) -> u8 {
    let sum: u32 = values.iter().enumerate().map(|(i, &v)| (i as u32 + 1) * u32::from(v)).sum();
    (sum % 31) as u8
}

/// The code of fifteen random values (each taken modulo 32), grouped by four.
pub fn format(values: &[u8; SYMBOLS]) -> String {
    let values: Vec<u8> = values.iter().map(|v| v % 32).collect();
    let mut symbols: Vec<u8> = values.iter().map(|&v| ALPHABET[usize::from(v)]).collect();
    symbols.push(ALPHABET[usize::from(check(&values))]);
    symbols.chunks(4).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join("-")
}

/// The code someone typed, in its one canonical form (sixteen symbols, no
/// dashes), if it is one: case, spaces and dashes don't matter, and I, L and
/// O read as 1, 1 and 0 the way Crockford's base 32 reads them.
pub fn normalize(typed: &str) -> Option<String> {
    let mut values = Vec::with_capacity(SYMBOLS + 1);
    for c in typed.chars().filter(|c| !c.is_whitespace() && *c != '-') {
        let c = match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            c => c,
        };
        let v = ALPHABET.iter().position(|&a| char::from(a) == c)?;
        values.push(v as u8);
    }
    if values.len() != SYMBOLS + 1 || check(&values[..SYMBOLS]) != values[SYMBOLS] {
        return None;
    }
    Some(values.iter().map(|&v| char::from(ALPHABET[usize::from(v)])).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_reads_back_however_it_is_typed() {
        let values = [3, 31, 0, 17, 8, 22, 5, 9, 30, 1, 2, 11, 27, 14, 6];
        let code = format(&values);
        assert_eq!(code.len(), 19, "{code}");
        assert_eq!(code.matches('-').count(), 3);
        let canon = normalize(&code).unwrap();
        assert_eq!(canon, code.replace('-', ""));
        assert_eq!(normalize(&code.to_lowercase()).as_deref(), Some(canon.as_str()));
        assert_eq!(normalize(&format!("  {}  ", code.replace('-', " "))).as_deref(), Some(canon.as_str()));
        // O for 0, as people type it.
        let with_zero = format(&[0; SYMBOLS]);
        assert!(normalize(&with_zero.replace('0', "O")).is_some());
    }

    #[test]
    fn typos_are_caught() {
        let values = [3, 31, 0, 17, 8, 22, 5, 9, 30, 1, 2, 11, 27, 14, 6];
        let canon = normalize(&format(&values)).unwrap();
        let bytes = canon.as_bytes();
        for i in 0..bytes.len() {
            for &a in ALPHABET {
                if a == bytes[i] {
                    continue;
                }
                let mut changed = bytes.to_vec();
                changed[i] = a;
                let typed = String::from_utf8(changed).unwrap();
                // A change by 31 (0 for Z) is the one blind spot of modulo 31.
                let delta = (ALPHABET.iter().position(|&x| x == a).unwrap() as i32
                    - ALPHABET.iter().position(|&x| x == bytes[i]).unwrap() as i32)
                    .abs();
                if delta != 31 {
                    assert!(normalize(&typed).is_none(), "{typed} passed");
                }
            }
        }
        for i in 0..SYMBOLS - 1 {
            let at = |j: usize| ALPHABET.iter().position(|&x| x == bytes[j]).unwrap() as i32;
            if bytes[i] != bytes[i + 1] && (at(i) - at(i + 1)).abs() != 31 {
                let mut swapped = bytes.to_vec();
                swapped.swap(i, i + 1);
                assert!(normalize(&String::from_utf8(swapped).unwrap()).is_none());
            }
        }
        assert!(normalize("").is_none());
        assert!(normalize("ABCD-EFGH").is_none());
        assert!(normalize("UUUU-UUUU-UUUU-UUUU").is_none());
    }
}
