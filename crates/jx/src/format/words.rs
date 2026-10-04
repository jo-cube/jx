const SMALL: [&str; 20] = [
    "Zero",
    "One",
    "Two",
    "Three",
    "Four",
    "Five",
    "Six",
    "Seven",
    "Eight",
    "Nine",
    "Ten",
    "Eleven",
    "Twelve",
    "Thirteen",
    "Fourteen",
    "Fifteen",
    "Sixteen",
    "Seventeen",
    "Eighteen",
    "Nineteen",
];
const ORDINAL: [&str; 20] = [
    "Zeroth",
    "First",
    "Second",
    "Third",
    "Fourth",
    "Fifth",
    "Sixth",
    "Seventh",
    "Eighth",
    "Ninth",
    "Tenth",
    "Eleventh",
    "Twelfth",
    "Thirteenth",
    "Fourteenth",
    "Fifteenth",
    "Sixteenth",
    "Seventeenth",
    "Eighteenth",
    "Nineteenth",
];
const TENS: [&str; 8] = [
    "Twenty", "Thirty", "Forty", "Fifty", "Sixty", "Seventy", "Eighty", "Ninety",
];
const TENS_ORDINAL: [&str; 8] = [
    "Twentieth",
    "Thirtieth",
    "Fortieth",
    "Fiftieth",
    "Sixtieth",
    "Seventieth",
    "Eightieth",
    "Ninetieth",
];
const SCALE: [&str; 4] = ["Thousand", "Million", "Billion", "Trillion"];

pub(super) fn write(n: f64, ordinal: bool, after: bool, output: &mut String) {
    if after {
        output.push_str(if n < 100.0 { " and " } else { ", " });
    }
    if n < 20.0 {
        output.push_str(if ordinal {
            ORDINAL[n as usize]
        } else {
            SMALL[n as usize]
        });
    } else if n < 100.0 {
        let tens = (n / 10.0).floor() as usize - 2;
        let rest = n % 10.0;
        output.push_str(if ordinal && rest == 0.0 {
            TENS_ORDINAL[tens]
        } else {
            TENS[tens]
        });
        if rest != 0.0 {
            output.push('-');
            write(rest, ordinal, false, output);
        }
    } else if n < 1000.0 {
        output.push_str(SMALL[(n / 100.0).floor() as usize]);
        output.push_str(" Hundred");
        let rest = n % 100.0;
        if rest != 0.0 {
            write(rest, ordinal, true, output);
        } else if ordinal {
            output.push_str("th");
        }
    } else {
        let scale = ((n.log10() / 3.0).floor() as usize).clamp(1, 4);
        let factor = 10_f64.powi((scale * 3) as i32);
        let head = (n / factor).floor();
        let rest = n - head * factor;
        write(head, false, false, output);
        output.push(' ');
        output.push_str(SCALE[scale - 1]);
        if rest != 0.0 {
            write(rest, ordinal, true, output);
        } else if ordinal {
            output.push_str("th");
        }
    }
}
fn value(word: &str) -> Option<f64> {
    for (i, (plain, ordinal)) in SMALL.iter().zip(ORDINAL).enumerate() {
        if word.eq_ignore_ascii_case(plain) || word.eq_ignore_ascii_case(ordinal) {
            return Some(i as f64);
        }
    }
    for (i, (plain, ordinal)) in TENS.iter().zip(TENS_ORDINAL).enumerate() {
        if word.eq_ignore_ascii_case(plain) || word.eq_ignore_ascii_case(ordinal) {
            return Some(((i + 2) * 10) as f64);
        }
    }
    if word.eq_ignore_ascii_case("hundred") || word.eq_ignore_ascii_case("hundredth") {
        return Some(100.0);
    }
    for (i, name) in SCALE.iter().enumerate() {
        if word.eq_ignore_ascii_case(name)
            || word.len() == name.len() + 2
                && word
                    .get(..name.len())
                    .is_some_and(|s| s.eq_ignore_ascii_case(name))
                && word
                    .get(name.len()..)
                    .is_some_and(|s| s.eq_ignore_ascii_case("th"))
        {
            return Some(10_f64.powi(((i + 1) * 3) as i32));
        }
    }
    None
}
pub(super) fn parse(text: &str) -> f64 {
    let mut total = 0.0;
    let mut segment = 0.0;
    let mut start = 0;
    let mut at = 0;
    let mut add = |word: &str| {
        let n = value(word).unwrap_or(f64::NAN);
        if n < 100.0 {
            if segment >= 1000.0 {
                total += segment;
                segment = 0.0;
            }
            segment += n;
        } else {
            segment *= n;
        }
    };
    while at < text.len() {
        let rest = &text[at..];
        let length = if rest
            .get(..5)
            .is_some_and(|s| s.eq_ignore_ascii_case(" and "))
        {
            5
        } else if rest.starts_with(", ") {
            2
        } else {
            let ch = rest.chars().next().unwrap();
            if ch.is_whitespace() || ch == '-' || ch == '\\' {
                ch.len_utf8()
            } else {
                at += ch.len_utf8();
                continue;
            }
        };
        add(&text[start..at]);
        at += length;
        start = at;
    }
    add(&text[start..]);
    total + segment
}
pub(super) fn pattern() -> String {
    let mut tokens: Vec<_> = SMALL
        .iter()
        .chain(ORDINAL.iter())
        .chain(TENS.iter())
        .chain(TENS_ORDINAL.iter())
        .map(|s| s.to_ascii_lowercase())
        .collect();
    tokens.extend(["hundred".into(), "hundredth".into(), "and".into()]);
    for name in SCALE {
        let name = name.to_ascii_lowercase();
        tokens.push(format!("{name}th"));
        tokens.push(name);
    }
    format!("(?:{}|[\\\\, -])+", tokens.join("|"))
}
