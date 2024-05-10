use std::fmt::Write;

pub fn encode_into(s: &mut String, x: u32, y: u32) {
    encode_one_dim_into(s, x);
    encode_one_dim_into(s, y);
}

fn encode_one_dim_into(s: &mut String, x: u32) {
    let char = x % 52;
    let postfix = (if char < 26 {
        b'a' + char as u8
    } else {
        b'A' + char as u8 - 26
    } as char);
    if x < 52 {
        s.push(postfix);
    } else {
        write!(s, "{}", (x / 52)).unwrap();
        s.push(postfix);
    }
}

pub fn decode(s: &str) -> Option<(u32, u32)> {
    let (num, s) = span(&s, char::is_ascii_digit);
    let mut x = if num == "" { 0 } else { num.parse().ok()? } * 52;
    if s.len() == 0 {
        return None;
    }
    let (yeah, s) = (s.chars().next().unwrap(), &s[1..]);
    x += match yeah {
        'A'..='Z' => yeah as u8 - b'A' + 26,
        'a'..='z' => yeah as u8 - b'a',
        _ => return None,
    } as u32;

    let (num, s) = span(&s, char::is_ascii_digit);
    let mut y = if num == "" { 0 } else { num.parse().ok()? } * 52;
    if s.len() == 0 {
        return None;
    }
    let (yeah, _) = (s.chars().next().unwrap(), &s[1..]);
    y += match yeah {
        'A'..='Z' => yeah as u8 - b'A' + 26,
        'a'..='z' => yeah as u8 - b'a',
        _ => return None,
    } as u32;

    Some((x, y))
}

fn span<P: Fn(&char) -> bool>(xs: &str, p: P) -> (&str, &str) {
    for (ix, c) in xs.char_indices() {
        if !p(&c) {
            return (&xs[..ix], &xs[ix..]);
        }
    }

    (xs, "")
}

#[cfg(test)]
use proptest::prelude::*;
#[cfg(test)]
proptest! {
    #[test]
    fn lol(x: u32, y: u32) {
        let mut encoded = String::new();
        encode_into(&mut encoded, x, y);
        assert_eq!((x, y), decode(&encoded).unwrap(), "{}, {} -> {}", x, y, encoded);
    }
}
