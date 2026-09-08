use penv_dotenv::{WriteError, ensure_ignored, infer, read, write};
use penv_schema::{BaseType, render};

fn codes(warnings: &[penv_dotenv::Warning]) -> Vec<&str> {
    warnings.iter().map(|w| w.code).collect()
}

#[test]
fn reads_the_safe_subset() {
    let env = read("A_KEY=one\nB_KEY=two\n");
    assert_eq!(
        env.entries
            .iter()
            .map(|e| (e.key.as_str(), e.value.as_str()))
            .collect::<Vec<_>>(),
        [("A_KEY", "one"), ("B_KEY", "two")]
    );
    assert!(env.warnings.is_empty());
}

#[test]
fn tolerates_what_real_files_contain() {
    let source = "\u{feff}# a comment\r\n\r\nexport A_KEY = one\r\nB_KEY='two three'\r\nC_KEY=\"four\"\r\nD_KEY=five # trailing\r\n";
    let env = read(source);
    assert_eq!(env.get("A_KEY"), Some("one"));
    assert_eq!(env.get("B_KEY"), Some("two three"));
    assert_eq!(env.get("C_KEY"), Some("four"));
    assert_eq!(env.get("D_KEY"), Some("five"));
    let found = codes(&env.warnings);
    for expected in [
        "bom",
        "crlf",
        "export_prefix",
        "spaces_around_equals",
        "inline_comment",
    ] {
        assert!(found.contains(&expected), "missing {expected} in {found:?}");
    }
}

#[test]
fn warns_on_constructs_outside_the_subset() {
    let env =
        read("A_KEY=one\nA_KEY=two\nB_KEY=$A_KEY\nC_KEY=\"line one\nline two\"\nnot a pair\n");
    assert_eq!(env.get("A_KEY"), Some("two"), "the last value wins");
    assert_eq!(env.get("C_KEY"), Some("line one\nline two"));
    let found = codes(&env.warnings);
    for expected in [
        "duplicate_key",
        "interpolation",
        "multiline_value",
        "invalid_line",
    ] {
        assert!(found.contains(&expected), "missing {expected} in {found:?}");
    }
}

#[test]
fn writes_only_the_safe_subset() {
    let out = write(&[
        ("A_KEY", "one"),
        ("B_KEY", "two three"),
        ("C_KEY", ""),
        ("D_KEY", "has#hash"),
    ])
    .unwrap();
    assert_eq!(
        out,
        "A_KEY=one\nB_KEY=\"two three\"\nC_KEY=\nD_KEY=\"has#hash\"\n"
    );
}

#[test]
fn the_writer_refuses_what_the_subset_excludes() {
    assert_eq!(
        write(&[("A_KEY", "$OTHER")]),
        Err(WriteError::Interpolation {
            key: "A_KEY".into()
        })
    );
    assert_eq!(
        write(&[("A_KEY", "one\ntwo")]),
        Err(WriteError::MultiLine {
            key: "A_KEY".into()
        })
    );
    assert_eq!(
        write(&[("A_KEY", "one"), ("A_KEY", "two")]),
        Err(WriteError::DuplicateKey {
            key: "A_KEY".into()
        })
    );
    assert_eq!(
        write(&[("a-key", "one")]),
        Err(WriteError::InvalidKey {
            key: "a-key".into()
        })
    );
    assert!(write(&[("A_KEY", "both ' and \"")]).is_err());
    assert_eq!(
        write(&[("A_KEY", r"a \ and a '")]),
        Err(WriteError::Unquotable {
            key: "A_KEY".into()
        })
    );
}

#[test]
fn what_the_writer_emits_reads_back_unchanged() {
    let pairs = [("A_KEY", "one two"), ("B_KEY", "has#hash"), ("C_KEY", "")];
    let text = write(&pairs).unwrap();
    let env = read(&text);
    assert!(env.warnings.is_empty(), "{:?}", env.warnings);
    for (key, value) in pairs {
        assert_eq!(env.get(key), Some(value));
    }
}

#[test]
fn infers_types_from_values() {
    let env = read(
        "APP_URL=https://app.example.test\nPORT=3000\nDEBUG=true\nWORKERS=4\nRATE=1.5\nOWNER_EMAIL=dev@example.test\nAPP_NAME=demo\n",
    );
    let schema = infer(&env);
    let ty = |name: &str| schema.get(name).unwrap().ty.base;
    assert_eq!(ty("APP_URL"), BaseType::Url);
    assert_eq!(ty("PORT"), BaseType::Port);
    assert_eq!(ty("DEBUG"), BaseType::Boolean);
    assert_eq!(ty("WORKERS"), BaseType::Number);
    assert_eq!(
        schema.get("WORKERS").unwrap().ty.constraint("isInt"),
        Some("true"),
        "a whole number is number(isInt=true)"
    );
    assert_eq!(ty("RATE"), BaseType::Number);
    assert_eq!(ty("OWNER_EMAIL"), BaseType::Email);
    assert_eq!(ty("APP_NAME"), BaseType::String);
    assert!(
        schema.keys.iter().all(|k| k.ty.base != BaseType::Enum),
        "enum is never inferred"
    );
}

#[test]
fn every_key_is_sensitive_unless_a_prefix_or_a_dull_value_says_otherwise() {
    let env = read(concat!(
        "STRIPE_SECRET_KEY=sk_test_0000000000
",
        "SESSION_JWT=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJmYWtlIn0.c2lnbmF0dXJlZmFrZQ
",
        "DATABASE_URL=postgres://someone:fakepassword@localhost:5432/app
",
        "APP_NAME=demo
",
        "NEXT_PUBLIC_API_KEY=pk_public_fake
",
    ));
    let schema = infer(&env);
    let sensitive = |name: &str| schema.get(name).unwrap().sensitive;
    assert!(sensitive("STRIPE_SECRET_KEY"));
    assert!(sensitive("SESSION_JWT"));
    assert!(
        sensitive("DATABASE_URL"),
        "a connection string is never copied, however it is spelled"
    );
    assert!(!sensitive("APP_NAME"), "a lowercase word is not a secret");
    assert!(
        !sensitive("NEXT_PUBLIC_API_KEY"),
        "a bundler prefix ships the value to the browser anyway"
    );
    assert_eq!(
        schema
            .get("NEXT_PUBLIC_API_KEY")
            .unwrap()
            .sensitive_decorator,
        None,
        "the prefix rule needs no decorator"
    );
}

#[test]
fn sensitive_values_never_reach_the_schema() {
    let env = read("STRIPE_SECRET_KEY=sk_test_0000000000\nAPP_NAME=demo\nPORT=3000\n");
    let schema = infer(&env);
    let secret = schema.get("STRIPE_SECRET_KEY").unwrap();
    assert_eq!(secret.default, None);
    assert!(secret.required);
    assert_eq!(
        schema.get("APP_NAME").unwrap().default.as_deref(),
        Some("demo")
    );
    assert!(!schema.get("APP_NAME").unwrap().required);

    let rendered = render(&schema);
    assert!(!rendered.contains("sk_test_0000000000"));
    assert!(rendered.contains("STRIPE_SECRET_KEY=\n"));
}

#[test]
fn an_inferred_schema_parses_back() {
    let env =
        read("APP_URL=https://app.example.test\nSTRIPE_SECRET_KEY=sk_test_0000000000\nPORT=3000\n");
    let schema = infer(&env);
    let reparsed = penv_schema::parse(&render(&schema)).expect("the draft parses");
    assert_eq!(schema, reparsed);
}

#[test]
fn the_gitignore_helper_is_idempotent() {
    let first = ensure_ignored("node_modules/\n");
    assert_eq!(first.added, [".env", ".env.*", "!.env.schema"]);
    assert!(first.content.contains("\n.env\n"));
    assert!(first.content.contains("\n.env.*\n"));
    assert!(
        first.content.contains("\n!.env.schema\n"),
        ".env.* would otherwise swallow the one committed penv file"
    );

    let second = ensure_ignored(&first.content);
    assert!(second.added.is_empty());
    assert_eq!(second.content, first.content);
    assert!(!second.changed());
}

#[test]
fn the_gitignore_helper_adds_only_what_is_missing() {
    let update = ensure_ignored("/.env\ndist/\n");
    assert_eq!(update.added, [".env.*", "!.env.schema"]);
}

#[test]
fn awkward_values_round_trip_through_the_writer() {
    let values = [
        "plain",
        "one two",
        "has#hash",
        r"back\slash",
        r"C:\Users\dev\app",
        r"back\slash and a space",
        "quote\"inside",
        "apostrophe'inside",
        "trailing space ",
        "=equals=",
        "",
    ];
    for value in values {
        let text = write(&[("A_KEY", value)]).unwrap_or_else(|e| panic!("{value:?}: {e}"));
        let env = read(&text);
        assert_eq!(env.get("A_KEY"), Some(value), "wrote {text:?}");
        assert!(env.warnings.is_empty(), "{value:?}: {:?}", env.warnings);
    }
}

#[test]
fn only_a_dull_value_is_copied_into_the_committed_schema() {
    let env = read(concat!(
        "SLACK_WEBHOOK_URL=https://hooks.slack.test/services/T00/B00/xoxbFAKETOKEN\n",
        "DATABASE_URL=postgres://app.example.test/db?password=hunter2\n",
        "SESSION_SEED=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n",
        "ADMIN_PW=hunter2\n",
        "NODE_ENV=development\n",
        "PORT=3000\n",
        "NEXT_PUBLIC_APP_URL=https://example.test/x?y=1\n",
        "DEBUG=true\n",
        "API_URL=http://localhost:3000\n",
    ));
    let schema = infer(&env);
    let default = |name: &str| schema.get(name).unwrap().default.clone();
    let sensitive = |name: &str| schema.get(name).unwrap().sensitive;
    let required = |name: &str| schema.get(name).unwrap().required;

    for kept in [
        "SLACK_WEBHOOK_URL",
        "DATABASE_URL",
        "SESSION_SEED",
        "ADMIN_PW",
    ] {
        assert_eq!(
            default(kept),
            None,
            "{kept} carried its value into the schema"
        );
        assert!(sensitive(kept), "{kept} is not sensitive");
        assert!(required(kept), "{kept} is not required");
    }
    for copied in [
        "NODE_ENV",
        "PORT",
        "NEXT_PUBLIC_APP_URL",
        "DEBUG",
        "API_URL",
    ] {
        assert!(default(copied).is_some(), "{copied} lost its value");
        assert!(!sensitive(copied), "{copied} is sensitive but was copied");
        assert!(
            !required(copied),
            "{copied} has a default, so it is optional"
        );
    }

    let rendered = render(&schema);
    for secret in ["xoxbFAKETOKEN", "hunter2", "0123456789abcdef"] {
        assert!(!rendered.contains(secret), "{secret} reached the schema");
    }
    assert!(rendered.contains("NEXT_PUBLIC_APP_URL=https://example.test/x?y=1"));
    assert_eq!(penv_schema::parse(&rendered).unwrap(), schema);
}

#[test]
fn types_are_inferred_even_when_the_value_stays_out() {
    let env = read(concat!(
        "DATABASE_URL=postgres://app.example.test/db?password=hunter2\n",
        "SMTP_PORT=2525\n",
        "OWNER_EMAIL=dev@example.test\n",
        "RETRY_BUDGET=7\n",
        "SAMPLE_RATE=0.25\n",
    ));
    let schema = infer(&env);
    let ty = |name: &str| schema.get(name).unwrap().ty.clone();
    assert_eq!(ty("DATABASE_URL").base, BaseType::Url);
    assert_eq!(ty("SMTP_PORT").base, BaseType::Port);
    assert_eq!(ty("OWNER_EMAIL").base, BaseType::Email);
    assert_eq!(ty("RETRY_BUDGET").constraint("isInt"), Some("true"));
    assert_eq!(ty("SAMPLE_RATE").base, BaseType::Number);
    assert_eq!(
        schema.get("DATABASE_URL").unwrap().default,
        None,
        "the type came from the value, the value stayed out"
    );
}
