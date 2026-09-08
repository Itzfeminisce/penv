//! Streaming scrubber that masks sensitive values in a child process's output.
//! A secret is caught however it was split across reads, and in the forms a
//! program is likely to have re-encoded it into on the way out.

/// Shorter than this and the mask would swallow ordinary text.
pub const MIN_SECRET_LEN: usize = 4;

/// What replaces a match, after the first two characters of the secret.
pub const BLOCKS: &str = "\u{2592}\u{2592}\u{2592}\u{2592}\u{2592}\u{2592}";

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// What a matched secret is replaced with: enough to recognise which one it was,
/// and nothing that helps use it.
pub fn redaction(secret: &str) -> String {
    let head: String = secret.chars().take(2).collect();
    format!("{head}{BLOCKS}")
}

struct Pattern {
    bytes: Vec<u8>,
    replacement: Vec<u8>,
}

/// Masks a byte stream fed to it in arbitrary chunks.
pub struct Masker {
    patterns: Vec<Pattern>,
    /// Pattern indices by first byte, longest first, so a scan step is a lookup.
    by_first_byte: Vec<Vec<u32>>,
    longest: usize,
    held: Vec<u8>,
}

impl Masker {
    pub fn new(secrets: Vec<String>) -> Masker {
        let mut patterns: Vec<Pattern> = Vec::new();
        for secret in &secrets {
            if secret.len() < MIN_SECRET_LEN {
                continue;
            }
            let replacement = redaction(secret).into_bytes();
            for form in forms(secret) {
                if form.len() >= MIN_SECRET_LEN
                    && !patterns.iter().any(|p| p.bytes == form.as_bytes())
                {
                    patterns.push(Pattern {
                        bytes: form.into_bytes(),
                        replacement: replacement.clone(),
                    });
                }
            }
        }
        patterns.sort_by_key(|p| std::cmp::Reverse(p.bytes.len()));

        let longest = patterns.first().map_or(0, |p| p.bytes.len());
        let mut by_first_byte = vec![Vec::new(); 256];
        for (index, pattern) in patterns.iter().enumerate() {
            by_first_byte[pattern.bytes[0] as usize].push(index as u32);
        }

        Masker {
            patterns,
            by_first_byte,
            longest,
            held: Vec::new(),
        }
    }

    /// True when nothing is worth scanning for, so the stream can be copied.
    pub fn is_pass_through(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Scrub a chunk. Bytes that could still start a secret are held back until
    /// the next chunk or `finish`.
    pub fn feed(&mut self, chunk: &[u8], out: &mut Vec<u8>) {
        if self.is_pass_through() {
            out.extend_from_slice(chunk);
            return;
        }
        self.held.extend_from_slice(chunk);
        let limit = self.held.len().saturating_sub(self.longest - 1);
        self.scan(limit, out);
    }

    /// Scrub what is held back and release it. The stream ends here.
    pub fn finish(&mut self, out: &mut Vec<u8>) {
        if self.is_pass_through() {
            return;
        }
        let limit = self.held.len();
        self.scan(limit, out);
    }

    fn scan(&mut self, limit: usize, out: &mut Vec<u8>) {
        let mut index = 0;
        let mut run = 0;
        while index < limit {
            match self.match_at(index) {
                Some(pattern) => {
                    out.extend_from_slice(&self.held[run..index]);
                    out.extend_from_slice(&self.patterns[pattern].replacement);
                    index += self.patterns[pattern].bytes.len();
                    run = index;
                }
                None => index += 1,
            }
        }
        out.extend_from_slice(&self.held[run..index]);
        self.held.drain(..index);
    }

    /// The longest pattern that fits whole at this position.
    fn match_at(&self, index: usize) -> Option<usize> {
        let rest = &self.held[index..];
        self.by_first_byte[rest[0] as usize]
            .iter()
            .map(|i| *i as usize)
            .find(|i| rest.starts_with(&self.patterns[*i].bytes))
    }
}

/// The shapes one secret can leave a process in: as written, base64 in both
/// alphabets padded and not, and escaped as a JSON string body.
fn forms(secret: &str) -> Vec<String> {
    let raw = secret.as_bytes();
    let mut out = vec![
        secret.to_string(),
        base64(raw, STANDARD, true),
        base64(raw, STANDARD, false),
        base64(raw, URL_SAFE, true),
        base64(raw, URL_SAFE, false),
    ];
    let escaped = json_escaped(secret);
    if escaped != secret {
        out.push(escaped);
    }
    out
}

fn base64(input: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for group in input.chunks(3) {
        let high = group[0] as u32;
        let mid = *group.get(1).unwrap_or(&0) as u32;
        let low = *group.get(2).unwrap_or(&0) as u32;
        let n = (high << 16) | (mid << 8) | low;
        let digits = [
            alphabet[((n >> 18) & 63) as usize],
            alphabet[((n >> 12) & 63) as usize],
            alphabet[((n >> 6) & 63) as usize],
            alphabet[(n & 63) as usize],
        ];
        let kept = group.len() + 1;
        for digit in digits.iter().take(kept) {
            out.push(*digit as char);
        }
        if pad {
            for _ in kept..4 {
                out.push('=');
            }
        }
    }
    out
}

/// The body of a JSON string, without the quotes around it.
fn json_escaped(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
