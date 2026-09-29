//! The menus' number and time text, on typed state: the IWRAM divide routine leaves its remainder in a global
//! (`MenuGlobals::div_remainder`), so the helpers take it as `rem`; the language decides the separators.

/// The IWRAM divide routine (`call_via_r3(n, d, 0x03006480, *0x03006494)`, `nfsgba_fixed::iwram_divmod`): the
/// quotient, with the remainder stored in `rem`.
pub fn iwram_div(rem: &mut u32, n: i32, d: i32) -> i32 {
    let (q, r) = nfsgba_fixed::iwram_divmod(n, d);
    *rem = r as u32;
    q
}

/// `FUN_081633CC` (buffer, n): `n` in decimal, a leading '-' when negative, digits from the millions down with
/// leading zeros dropped (a quotient above 9 prints as the character after '9').
pub fn number_text(rem: &mut u32, n: i32) -> Vec<u8> {
    let mut s = Vec::new();
    let mut n = n;
    if n < 0 {
        s.push(b'-');
        n = n.wrapping_neg();
    }
    if n < 10 {
        s.push(n as u8 + b'0');
        return s;
    }
    let (mut d, mut leading) = (1_000_000, true);
    for _ in 0..7 {
        let q = iwram_div(rem, n, d);
        if q != 0 || !leading {
            leading = false;
            s.push((q as u8).wrapping_add(b'0'));
        }
        n = *rem as i32;
        d = nfsgba_fixed::div(d, 10);
    }
    s
}

/// `FUN_0812D62C` (text, n): a thousands separator for `n` above 999 in French (a space) and German and Italian
/// (a dot), inserted `digits − 3` from the left of the text `number_text` made.
pub fn thousands(language: u32, s: &mut Vec<u8>, n: i32) {
    if language == 0 || language == 4 || n <= 999 {
        return;
    }
    let sep = if language == 1 { b' ' } else { b'.' };
    // NOT 1:1 (N1): above 999,999 the game uses a stale register as the position.
    let pos = match n {
        1_000..=9_999 => 1,
        10_000..=99_999 => 2,
        100_000..=999_999 => 3,
        _ => return,
    };
    s.resize(s.len().max(pos + 5), 0);
    let mut i = pos + 4;
    while pos < i {
        s[i] = s[i - 1];
        i -= 1;
    }
    s[i] = sep;
    s.truncate(s.iter().position(|&b| b == 0).unwrap_or(s.len()));
}

/// `FUN_0812FC00` (text, centiseconds): `FUN_08162FC0`'s "mm:ss:cc" (|t| capped at 0x57E3F), then the last colon
/// becomes a dot in French, German and Spanish, a comma in Italian.
pub fn time_text(rem: &mut u32, language: u32, cs: i32) -> Vec<u8> {
    let n = cs.unsigned_abs().min(0x57E3F) as i32;
    let secs = iwram_div(rem, n, 100);
    let hundredths = *rem as i32;
    let mins = iwram_div(rem, secs, 0x3C);
    let secs = *rem as i32;
    let mut s = Vec::new();
    for (i, v) in [mins, secs, hundredths].into_iter().enumerate() {
        let tens = iwram_div(rem, v, 10);
        s.push((tens as u8).wrapping_add(b'0'));
        s.push((*rem as u8).wrapping_add(b'0'));
        if i < 2 {
            s.push(b':');
        }
    }
    match language {
        3 => s[5] = b',',
        1 | 2 | 4 => s[5] = b'.',
        _ => {}
    }
    s
}

/// `frames_to_centiseconds` (`0x08142F74`).
pub fn frames_to_centiseconds(frames: i32) -> i32 {
    nfsgba_fixed::div(frames.wrapping_mul(100), 0x3C)
}
