use penv_schema::{BaseType, Values, parse, render, validate};
use serde_json::json;

const EXAMPLE: &str = "\
# @penv=acme/api-gateway @schema=1
# @defaultSensitive=true

# @type=url
DATABASE_URL=

# @type=string(startsWith=sk_) @rotate=90d
STRIPE_SECRET_KEY=

# @type=url @sensitive=false
NEXT_PUBLIC_APP_URL=http://localhost:3000

# @type=port
PORT=3000

# @type=enum(development,staging,production)
NODE_ENV=development
";

fn values(pairs: &[(&str, &str)]) -> Values {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn parses_the_design_example() {
    let schema = parse(EXAMPLE).expect("the design example parses");
    assert_eq!(schema.org.as_deref(), Some("acme"));
    assert_eq!(schema.project.as_deref(), Some("api-gateway"));
    assert_eq!(schema.schema_version, 1);
    assert!(schema.default_sensitive);
    assert!(schema.is_cloud());

    let names: Vec<&str> = schema.keys.iter().map(|k| k.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "DATABASE_URL",
            "STRIPE_SECRET_KEY",
            "NEXT_PUBLIC_APP_URL",
            "PORT",
            "NODE_ENV"
        ]
    );

    let db = schema.get("DATABASE_URL").unwrap();
    assert_eq!(db.ty.base, BaseType::Url);
    assert!(db.required, "an empty value is required");
    assert!(db.sensitive);

    let stripe = schema.get("STRIPE_SECRET_KEY").unwrap();
    assert_eq!(stripe.ty.constraint("startsWith"), Some("sk_"));
    assert_eq!(stripe.rotate.as_deref(), Some("90d"));

    let public = schema.get("NEXT_PUBLIC_APP_URL").unwrap();
    assert!(!public.sensitive);
    assert!(!public.required, "a value on the line is a default");
    assert_eq!(public.default.as_deref(), Some("http://localhost:3000"));

    let node_env = schema.get("NODE_ENV").unwrap();
    assert_eq!(node_env.ty.base, BaseType::Enum);
    assert_eq!(
        node_env.ty.members,
        ["development", "staging", "production"]
    );
}

#[test]
fn a_bundler_prefix_makes_a_key_public_without_a_decorator() {
    let schema =
        parse("# @schema=1\n\n# @type=url\nVITE_API_URL=https://api.example.test\n").unwrap();
    let key = schema.get("VITE_API_URL").unwrap();
    assert!(!key.sensitive);
    assert_eq!(key.sensitive_decorator, None);
}

#[test]
fn sensitive_on_a_prefixed_key_is_an_error() {
    let err = parse("# @type=string @sensitive\nNEXT_PUBLIC_TOKEN=\n").unwrap_err();
    assert_eq!(err.len(), 1);
    assert_eq!(err[0].code, "sensitive_public_key");
    assert!(err[0].message.contains("NEXT_PUBLIC_TOKEN"));
    assert_eq!(err[0].line, 2);
}

#[test]
fn an_unknown_decorator_names_the_line() {
    let err = parse("# @schema=1\n\n# @type=string\n# @scope=team\nAPI_HOST=\n").unwrap_err();
    assert_eq!(err[0].code, "unknown_decorator");
    assert_eq!(err[0].line, 4);
    assert!(err[0].message.contains("@scope"));
    assert!(err[0].message.contains("line 4"));
}

#[test]
fn decorators_are_order_insensitive_and_take_quoted_values() {
    let a = parse("# @rotate=30d @type=string @deprecated=\"use API_HOST\"\nOLD_HOST=\n").unwrap();
    let b = parse("# @deprecated=\"use API_HOST\" @type=string @rotate=30d\nOLD_HOST=\n").unwrap();
    assert_eq!(a, b);
    assert_eq!(
        a.get("OLD_HOST").unwrap().deprecated.as_deref(),
        Some("use API_HOST")
    );
}

#[test]
fn function_call_constraints_parse() {
    let schema = parse(
        "# @type=number(min=1,max=10)\nWORKERS=4\n\n# @type=string(startsWith=sk_,maxLength=64)\nTOKEN=\n",
    )
    .unwrap();
    let workers = schema.get("WORKERS").unwrap();
    assert_eq!(workers.ty.base, BaseType::Number);
    assert_eq!(workers.ty.constraint("min"), Some("1"));
    assert_eq!(workers.ty.constraint("max"), Some("10"));
    let token = schema.get("TOKEN").unwrap();
    assert_eq!(token.ty.constraint("maxLength"), Some("64"));
}

#[test]
fn an_unknown_constraint_is_an_error() {
    let err = parse("# @type=string(isEven=true)\nA_KEY=\n").unwrap_err();
    assert_eq!(err[0].code, "unknown_constraint");
    assert!(err[0].message.contains("isEven"));
}

#[test]
fn comment_lines_without_an_at_are_the_description() {
    let schema = parse("# @schema=1\n\n# Where the API lives.\n# @type=url\nAPI_URL=\n").unwrap();
    assert_eq!(
        schema.get("API_URL").unwrap().description.as_deref(),
        Some("Where the API lives.")
    );
}

#[test]
fn defaults_can_be_flipped_in_the_header() {
    let schema = parse(
        "# @schema=1 @defaultSensitive=false @defaultRequired=false\n\n# @type=string\nA_KEY=\n",
    )
    .unwrap();
    let key = schema.get("A_KEY").unwrap();
    assert!(!key.sensitive);
    assert!(!key.required);
}

#[test]
fn required_and_optional_override_the_inference() {
    let schema =
        parse("# @type=string @optional\nA_KEY=\n\n# @type=string @required\nB_KEY=fallback\n")
            .unwrap();
    assert!(!schema.get("A_KEY").unwrap().required);
    assert!(schema.get("B_KEY").unwrap().required);
}

#[test]
fn a_duplicate_key_is_an_error() {
    let err = parse("# @type=string\nA_KEY=\n\n# @type=string\nA_KEY=\n").unwrap_err();
    assert_eq!(err[0].code, "duplicate_key");
    assert!(err[0].message.contains("A_KEY"));
}

#[test]
fn crlf_and_a_bom_are_tolerated() {
    let text = "\u{feff}# @penv=acme/api\r\n\r\n# @type=port\r\nPORT=3000\r\n";
    let schema = parse(text).unwrap();
    assert_eq!(schema.org.as_deref(), Some("acme"));
    assert_eq!(schema.get("PORT").unwrap().ty.base, BaseType::Port);
}

#[test]
fn round_trips_through_render() {
    let once = parse(EXAMPLE).unwrap();
    let rendered = render(&once);
    let twice = parse(&rendered).expect("rendered output parses");
    assert_eq!(once, twice);
    assert_eq!(render(&twice), rendered, "rendering is stable");
}

#[test]
fn round_trips_every_decorator() {
    let source = "\
# @penv=acme/api @schema=1
# @defaultSensitive=false
# @defaultRequired=false

# The primary store.
# @type=url @required @sensitive @example=\"postgres://localhost/app\" @docs=https://example.test/db @since=1.2.0 @deprecated=\"use STORE_URL\" @rotate=90d @dynamicFrom=vault @dynamic
DATABASE_URL=
";
    let once = parse(source).unwrap();
    let twice = parse(&render(&once)).unwrap();
    assert_eq!(once, twice);
    let key = twice.get("DATABASE_URL").unwrap();
    assert_eq!(key.since.as_deref(), Some("1.2.0"));
    assert_eq!(key.dynamic_from.as_deref(), Some("vault"));
    assert_eq!(key.dynamic, Some(true));
    assert!(key.sensitive);
}

#[test]
fn json_carries_the_ir() {
    let schema = parse(EXAMPLE).unwrap();
    let json = schema.to_json();
    assert_eq!(json["schemaVersion"], 1);
    assert_eq!(json["org"], "acme");
    assert_eq!(json["project"], "api-gateway");
    assert_eq!(json["defaultSensitive"], true);
    assert_eq!(
        json["keys"][1],
        json!({
            "name": "STRIPE_SECRET_KEY",
            "description": null,
            "type": {
                "name": "string",
                "raw": "string(startsWith=sk_)",
                "members": [],
                "constraints": { "startsWith": "sk_" }
            },
            "required": true,
            "sensitive": true,
            "default": null,
            "example": null,
            "docs": null,
            "since": null,
            "deprecated": null,
            "rotate": "90d",
            "dynamic": null,
            "dynamicFrom": null
        })
    );
}

#[test]
fn validate_reports_a_missing_required_value() {
    let schema = parse(EXAMPLE).unwrap();
    let found = validate(&schema, &values(&[("STRIPE_SECRET_KEY", "sk_test_fake")]));
    let missing: Vec<&str> = found
        .iter()
        .filter(|v| v.rule == "required")
        .map(|v| v.key.as_str())
        .collect();
    assert_eq!(missing, ["DATABASE_URL"]);
    assert!(found[0].message.contains("DATABASE_URL"));
}

#[test]
fn validate_checks_each_type() {
    let schema = parse(
        "# @type=url\nA_URL=\n\n# @type=port\nA_PORT=\n\n# @type=email\nAN_EMAIL=\n\n# @type=boolean\nA_FLAG=\n\n# @type=number(isInt=true)\nA_COUNT=\n",
    )
    .unwrap();
    let found = validate(
        &schema,
        &values(&[
            ("A_URL", "localhost:3000"),
            ("A_PORT", "70000"),
            ("AN_EMAIL", "nobody"),
            ("A_FLAG", "maybe"),
            ("A_COUNT", "1.5"),
        ]),
    );
    let keys: Vec<&str> = found.iter().map(|v| v.key.as_str()).collect();
    assert_eq!(keys, ["A_URL", "A_PORT", "AN_EMAIL", "A_FLAG", "A_COUNT"]);
    assert!(found.iter().all(|v| v.rule == "type" || v.rule == "isInt"));

    let ok = validate(
        &schema,
        &values(&[
            ("A_URL", "https://example.test/x"),
            ("A_PORT", "8080"),
            ("AN_EMAIL", "dev@example.test"),
            ("A_FLAG", "yes"),
            ("A_COUNT", "12"),
        ]),
    );
    assert!(ok.is_empty(), "{ok:?}");
}

#[test]
fn validate_checks_constraints_and_names_the_rule() {
    let schema = parse(
        "# @type=string(startsWith=sk_,minLength=8)\nA_TOKEN=\n\n# @type=number(min=1,max=10)\nWORKERS=\n\n# @type=enum(a,b)\nMODE=\n",
    )
    .unwrap();
    let found = validate(
        &schema,
        &values(&[("A_TOKEN", "pk_x"), ("WORKERS", "99"), ("MODE", "c")]),
    );
    let rules: Vec<(&str, &str)> = found
        .iter()
        .map(|v| (v.key.as_str(), v.rule.as_str()))
        .collect();
    assert_eq!(
        rules,
        [
            ("A_TOKEN", "minLength"),
            ("A_TOKEN", "startsWith"),
            ("WORKERS", "max"),
            ("MODE", "enum"),
        ]
    );
}

#[test]
fn no_violation_message_carries_a_value() {
    let schema = parse("# @type=url\nA_URL=\n").unwrap();
    let found = validate(&schema, &values(&[("A_URL", "totally-fake-value")]));
    assert_eq!(found.len(), 1);
    assert!(!found[0].message.contains("totally-fake-value"));
}

#[test]
fn a_key_with_a_default_is_satisfied_when_the_value_is_absent() {
    let schema = parse("# @type=port @required\nPORT=3000\n").unwrap();
    assert!(validate(&schema, &values(&[])).is_empty());
}

#[test]
fn a_type_call_may_carry_whitespace_inside_its_brackets() {
    let schema = parse(
        "# @type=enum(development, staging, production) @sensitive=false\nNODE_ENV=development\n\n# @type=string( startsWith = sk_ , maxLength = 64 )\nA_TOKEN=\n",
    )
    .expect("whitespace inside the brackets parses");
    let node_env = schema.get("NODE_ENV").unwrap();
    assert_eq!(
        node_env.ty.members,
        ["development", "staging", "production"]
    );
    assert!(!node_env.sensitive, "the decorator after the call is read");
    let token = schema.get("A_TOKEN").unwrap();
    assert_eq!(token.ty.constraint("startsWith"), Some("sk_"));
    assert_eq!(token.ty.constraint("maxLength"), Some("64"));
}

#[test]
fn a_header_block_sitting_on_the_first_key_is_still_the_header() {
    let schema =
        parse("# @penv=acme/api-gateway @schema=1 @defaultSensitive=false\nDATABASE_URL=\n")
            .expect("a header with no blank line after it parses");
    assert_eq!(schema.org.as_deref(), Some("acme"));
    assert_eq!(schema.project.as_deref(), Some("api-gateway"));
    assert!(!schema.default_sensitive);
    let key = schema.get("DATABASE_URL").unwrap();
    assert_eq!(
        key.ty.base,
        BaseType::String,
        "the header block gives the key nothing"
    );
    assert!(!key.sensitive, "the header applied to the key under it");
}

#[test]
fn integer_is_not_a_type_and_isint_is_the_constraint() {
    let err = parse("# @type=integer\nA_COUNT=\n").unwrap_err();
    assert_eq!(err[0].code, "unknown_type");
    assert!(err[0].message.contains("isInt"), "{}", err[0].message);

    let schema = parse("# @type=number(isInt=true)\nA_COUNT=\n").unwrap();
    let ty = &schema.get("A_COUNT").unwrap().ty;
    assert_eq!(ty.base, BaseType::Number);
    assert_eq!(ty.constraint("isInt"), Some("true"));
    assert_eq!(ty.to_json()["name"], "number");
    assert_eq!(ty.to_json()["constraints"]["isInt"], "true");

    assert!(validate(&schema, &values(&[("A_COUNT", "12")])).is_empty());
    let found = validate(&schema, &values(&[("A_COUNT", "1.5")]));
    assert_eq!(found[0].rule, "isInt");
}

#[test]
fn matches_and_precision_are_preserved_and_never_enforced() {
    let schema =
        parse("# @type=string(matches=\"^sk_[a-z]+$\")\nA_TOKEN=\n\n# @type=number(precision=2)\nA_RATE=\n")
            .unwrap();
    assert_eq!(
        schema.get("A_TOKEN").unwrap().ty.constraint("matches"),
        Some("^sk_[a-z]+$")
    );
    assert_eq!(
        schema.get("A_RATE").unwrap().ty.constraint("precision"),
        Some("2")
    );
    // penv carries no regex engine, so neither constraint can fail a value.
    assert!(
        validate(
            &schema,
            &values(&[("A_TOKEN", "nothing-like-it"), ("A_RATE", "1.5555")])
        )
        .is_empty()
    );
    assert_eq!(parse(&render(&schema)).unwrap(), schema);
}

#[test]
fn the_spec_booleans_are_kept_and_never_take_a_value() {
    let schema =
        parse("# @type=string @static\nA_KEY=\n\n# @type=string @dynamic\nB_KEY=\n").unwrap();
    assert_eq!(schema.get("A_KEY").unwrap().dynamic, Some(false));
    assert_eq!(schema.get("B_KEY").unwrap().dynamic, Some(true));
    assert_eq!(parse(&render(&schema)).unwrap(), schema);

    let err = parse("# @type=string @dynamic=false\nA_KEY=\n").unwrap_err();
    assert_eq!(err[0].code, "invalid_decorator_value");
}

#[test]
fn extras_name_the_keys_the_schema_does_not_declare() {
    let schema = parse(EXAMPLE).unwrap();
    let found = penv_schema::extras(
        &schema,
        &values(&[("PORT", "3000"), ("LEGACY_API_KEY", "unused")]),
    );
    assert_eq!(found, ["LEGACY_API_KEY"]);
}
