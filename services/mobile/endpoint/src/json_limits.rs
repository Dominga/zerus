//! The relay codec's lexical budget check, without enabling its JSON-number
//! features in hgs. Successful preflight is followed by strict typed decoding.
pub(crate) fn within(raw: &[u8], depth_limit: usize, value_limit: usize) -> bool {
    let mut depth = 0usize;
    let mut values = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    let mut scalar = false;
    for &c in raw {
        if quoted {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                quoted = false;
            }
            continue;
        }
        if scalar {
            if c.is_ascii_whitespace() || b",:]}".contains(&c) {
                scalar = false;
            } else {
                continue;
            }
        }
        match c {
            b'"' => {
                quoted = true;
                values += 1;
            }
            b'{' | b'[' => {
                depth += 1;
                values += 1;
                if depth > depth_limit {
                    return false;
                }
            }
            b'}' | b']' => {
                let Some(next) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next;
            }
            b',' | b':' | b' ' | b'\t' | b'\r' | b'\n' => {}
            _ => {
                scalar = true;
                values += 1;
            }
        }
        if values > value_limit {
            return false;
        }
    }
    if quoted || depth != 0 {
        return false;
    }
    true
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strings_do_not_change_the_depth_or_value_budget() {
        assert!(within(br#"{"a":[1,true],"b":"escaped \" [{}]"}"#, 2, 7));
        assert!(!within(br#"{"a":[1,true]}"#, 1, 5));
        assert!(!within(br#"{"a":[1,true]}"#, 2, 4));
        assert!(within(br#"{"a":[1,true]}"#, 2, 5));
    }
}
