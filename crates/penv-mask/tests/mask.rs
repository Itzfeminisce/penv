use penv_mask::{BLOCKS, Masker, redaction};

const SECRET: &str = "sk_test_FAKE0000";

fn through(secrets: &[&str], chunks: &[&[u8]]) -> String {
    let mut masker = Masker::new(secrets.iter().map(|s| s.to_string()).collect());
    let mut out = Vec::new();
    for chunk in chunks {
        masker.feed(chunk, &mut out);
    }
    masker.finish(&mut out);
    String::from_utf8(out).expect("the scrubber only ever splits on pattern boundaries")
}

fn one(secrets: &[&str], input: &str) -> String {
    through(secrets, &[input.as_bytes()])
}

#[test]
fn a_secret_is_replaced_by_its_first_two_characters_and_blocks() {
    assert_eq!(redaction(SECRET), format!("sk{BLOCKS}"));
    assert_eq!(
        one(&[SECRET], &format!("token={SECRET}\n")),
        format!("token=sk{BLOCKS}\n")
    );
}

#[test]
fn a_secret_split_at_any_byte_is_still_caught() {
    let line = format!("before {SECRET} after");
    let start = line.find(SECRET).unwrap();
    for split in start..start + SECRET.len() {
        let (head, tail) = line.split_at(split);
        let out = through(&[SECRET], &[head.as_bytes(), tail.as_bytes()]);
        assert!(!out.contains(SECRET), "split at {split}: {out}");
        assert_eq!(out, format!("before sk{BLOCKS} after"), "split at {split}");
    }
}

#[test]
fn a_secret_split_one_byte_at_a_time_is_still_caught() {
    let chunks: Vec<Vec<u8>> = format!("a{SECRET}b").bytes().map(|b| vec![b]).collect();
    let borrowed: Vec<&[u8]> = chunks.iter().map(|c| c.as_slice()).collect();
    assert_eq!(through(&[SECRET], &borrowed), format!("ask{BLOCKS}b"));
}

#[test]
fn a_secret_that_is_a_prefix_of_another_does_not_win() {
    let short = "sk_test_FAKE";
    let long = "sk_test_FAKE0000";
    let out = one(&[short, long], &format!("{long}\n"));
    assert!(!out.contains(short), "{out}");
    assert_eq!(out, format!("sk{BLOCKS}\n"));

    // The order the secrets arrive in does not change which one matches.
    assert_eq!(one(&[long, short], &format!("{long}\n")), out);
}

#[test]
fn overlapping_secrets_both_disappear() {
    let out = one(&["abcdef00", "ef0011223"], "xxabcdef0011223yy");
    assert!(!out.contains("abcdef00"), "{out}");
    assert!(!out.contains("ef0011223"), "{out}");
    assert!(out.starts_with("xx") && out.ends_with("yy"));
}

#[test]
fn the_base64_forms_are_caught_too() {
    // Padded and unpadded, in both alphabets. Computed here, not hard-coded.
    let value = "pw?A_FAKE1234~";
    let forms = [
        "cHc/QV9GQUtFMTIzNH4=",
        "cHc/QV9GQUtFMTIzNH4",
        "cHc_QV9GQUtFMTIzNH4=",
        "cHc_QV9GQUtFMTIzNH4",
    ];
    for form in forms {
        let out = one(&[value], &format!("body {form} end"));
        assert_eq!(out, format!("body pw{BLOCKS} end"), "{form}");
    }
}

#[test]
fn the_json_escaped_form_is_caught_too() {
    let value = "ab\"c\\FAKE";
    let escaped = "ab\\\"c\\\\FAKE";
    let out = one(&[value], &format!("{{\"k\":\"{escaped}\"}}"));
    assert_eq!(out, format!("{{\"k\":\"ab{BLOCKS}\"}}"));

    // A value that needs no escaping still matches as written.
    assert_eq!(one(&[SECRET], SECRET), format!("sk{BLOCKS}"));
}

#[test]
fn short_secrets_are_left_alone() {
    // Masking "1" would destroy the output; the design would rather print it.
    let out = one(&["1", "on", "abc"], "port 1 mode on abc");
    assert_eq!(out, "port 1 mode on abc");
}

#[test]
fn an_empty_secret_list_is_a_pass_through() {
    let mut masker = Masker::new(Vec::new());
    assert!(masker.is_pass_through());
    let mut out = Vec::new();
    masker.feed(b"anything at all", &mut out);
    masker.finish(&mut out);
    assert_eq!(String::from_utf8(out).unwrap(), "anything at all");
}

#[test]
fn interleaved_chunks_keep_the_rest_of_the_stream_intact() {
    let out = through(
        &[SECRET, "hunter2_FAKE"],
        &[
            b"line one\n",
            b"sk_test_",
            b"FAKE0000 and hun",
            b"ter2_FAKE\n",
            b"line three\n",
        ],
    );
    assert_eq!(
        out,
        format!("line one\nsk{BLOCKS} and hu{BLOCKS}\nline three\n")
    );
}

#[test]
fn nothing_is_held_back_once_the_stream_ends() {
    let mut masker = Masker::new(vec![SECRET.to_string()]);
    let mut out = Vec::new();
    masker.feed(b"sk_test_FAKE00", &mut out);
    assert!(!out.ends_with(b"00"), "a possible prefix is held back");
    masker.finish(&mut out);
    assert_eq!(String::from_utf8(out).unwrap(), "sk_test_FAKE00");
}

#[test]
fn a_megabyte_through_twenty_secrets_is_quick() {
    let secrets: Vec<String> = (0..20).map(|i| format!("sk_test_FAKE{i:04}")).collect();
    let mut masker = Masker::new(secrets.clone());
    let block = format!(
        "{}\n",
        "the quick brown fox jumps over the lazy dog. ".repeat(4)
    );
    let mut input = String::with_capacity(1 << 20);
    while input.len() < (1 << 20) {
        input.push_str(&block);
        input.push_str(&secrets[input.len() % 20]);
    }

    let started = std::time::Instant::now();
    let mut out = Vec::with_capacity(input.len() + 4096);
    for chunk in input.as_bytes().chunks(8192) {
        masker.feed(chunk, &mut out);
    }
    masker.finish(&mut out);
    let elapsed = started.elapsed();

    let text = String::from_utf8(out).unwrap();
    for secret in &secrets {
        assert!(!text.contains(secret.as_str()), "{secret} survived");
    }
    assert!(elapsed.as_secs_f64() < 1.0, "took {elapsed:?}");
}
