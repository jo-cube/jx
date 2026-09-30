// regress joins adjacent escaped surrogate units even in legacy mode. Feed them
// as literal units so both concatenation and character classes retain JS meaning.
pub(super) fn units(source: &str) -> Vec<u32> {
    let mut chars = source.encode_utf16().peekable();
    let mut result = Vec::new();
    while let Some(unit) = chars.next() {
        if unit == 92 {
            let mut probe = chars.clone();
            if probe.next() == Some(117) {
                let mut value = 0;
                let mut valid = true;
                for _ in 0..4 {
                    match probe
                        .next()
                        .and_then(|u| char::from_u32(u32::from(u)))
                        .and_then(|c| c.to_digit(16))
                    {
                        Some(digit) => value = value * 16 + digit,
                        None => {
                            valid = false;
                            break;
                        }
                    }
                }
                if valid && (0xd800..=0xdfff).contains(&value) {
                    result.push(value);
                    chars = probe;
                    continue;
                }
            }
            // Preserve escaped backslashes; the following u is then ordinary text.
            result.push(u32::from(unit));
            if let Some(next) = chars.next() {
                result.push(u32::from(next));
            }
        } else {
            result.push(u32::from(unit));
        }
    }
    result
}
