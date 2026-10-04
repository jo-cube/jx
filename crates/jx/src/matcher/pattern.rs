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

// regress uses simple uppercase mappings where legacy ECMAScript excludes
// non-ASCII-to-ASCII and multi-unit uppercase results. Keep the mismatching
// lowercase units explicit until the dependency implements Canonicalize exactly.
const EXCLUDED: &[(u32, u32)] = &[
    (0x131, 0x131),
    (0x17f, 0x17f),
    (0x1f80, 0x1f87),
    (0x1f90, 0x1f97),
    (0x1fa0, 0x1fa7),
    (0x1fb3, 0x1fb3),
    (0x1fc3, 0x1fc3),
    (0x1ff3, 0x1ff3),
];
pub(super) fn unsupported_case_fold(unit: u32) -> bool {
    matches!(unit, 0x131 | 0x17f | 0x1f80..=0x1f87 | 0x1f90..=0x1f97 | 0x1fa0..=0x1fa7 | 0x1fb3 | 0x1fc3 | 0x1ff3)
}
pub(crate) fn legacy_case_safe_text(text: &str) -> bool {
    text.is_ascii() || !text.chars().any(|c| unsupported_case_fold(c as u32))
}
pub(super) fn legacy_case_safe(source: &str) -> bool {
    let mut chars = source.chars().peekable();
    let mut class = false;
    let mut previous = None;
    let mut range = false;
    while let Some(mut ch) = chars.next() {
        let escaped = ch == '\\';
        if escaped {
            let Some(next) = chars.next() else { break };
            ch = next;
            if matches!(ch, 'u' | 'x') {
                let count = if ch == 'u' { 4 } else { 2 };
                let mut probe = chars.clone();
                let mut value = 0;
                let valid = (0..count).all(|_| {
                    probe
                        .next()
                        .and_then(|c| c.to_digit(16))
                        .is_some_and(|digit| {
                            value = value * 16 + digit;
                            true
                        })
                });
                if valid {
                    ch = char::from_u32(value).unwrap_or('\0');
                    chars = probe;
                }
            }
        }
        if unsupported_case_fold(ch as u32) {
            return false;
        }
        if class && range {
            if previous
                .is_some_and(|start| EXCLUDED.iter().any(|&(a, b)| start <= b && a <= ch as u32))
            {
                return false;
            }
            range = false;
        }
        if !escaped && ch == '[' {
            class = true;
            previous = None;
        } else if !escaped && ch == ']' {
            class = false;
        } else if class && !escaped && ch == '-' {
            range = previous.is_some();
        } else {
            previous = Some(ch as u32);
        }
    }
    true
}
