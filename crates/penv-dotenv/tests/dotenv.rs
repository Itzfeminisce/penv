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
    assert_eq!(ty("WORKERS"), BaseType::Integer);
    assert_eq!(ty("RATE"), BaseType::Number);
    assert_eq!(ty("OWNER_EMAIL"), BaseType::Email);
    assert_eq!(ty("APP_NAME"), BaseType::String);
    assert!(
        schema.keys.iter().all(|k| k.ty.base != BaseType::Enum),
        "enum is never inferred"
    );
}

#[test]
fn infers_sensitivity_by_name_and_by_value() {
    let env = read(
        "STRIPE_SECRET_KEY=sk_test_0000000000\nAPI_TOKEN=plain\nDB_PASSWORD=hunter2\nSENTRY_DSN=https://example.test/1\nSESSION_JWT=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJmYWtlIn0.c2lnbmF0dXJlZmFrZQ\nDATABASE_URL=postgres://someone:fakepassword@localhost:5432/app\nAPP_NAME=demo\nNEXT_PUBLIC_API_KEY=pk_public_fake\n",
    );
    let schema = infer(&env);
    let sensitive = |name: &str| schema.get(name).unwrap().sensitive;
    assert!(sensitive("STRIPE_SECRET_KEY"));
    assert!(sensitive("API_TOKEN"));
    assert!(sensitive("DB_PASSWORD"));
    assert!(sensitive("SENTRY_DSN"));
    assert!(sensitive("SESSION_JWT"), "a JWT value reads as a secret");
    assert!(
        sensitive("DATABASE_URL"),
        "a connection string carrying a password is a secret"
    );
    assert!(
        sensitive("DATABASE_URL"),
        "a connection string carrying a password is a secret"
    );
    assert!(!sensitive("APP_NAME"));
    assert!(
        !sensitive("NEXT_PUBLIC_API_KEY"),
        "a bundler prefix beats the name pattern"
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
