mod common;

use penv::manifest::{MANIFEST_VERSION, command_paths, has_meta, manifest};
use serde_json::Value;

const SCHEMA: &str = include_str!("../../../docs/manifest.schema.json");

fn commands(manifest: &Value) -> Vec<&Value> {
    fn walk<'a>(list: &'a Value, out: &mut Vec<&'a Value>) {
        for command in list.as_array().into_iter().flatten() {
            out.push(command);
            walk(&command["subcommands"], out);
        }
    }
    let mut out = Vec::new();
    walk(&manifest["commands"], &mut out);
    out
}

fn find<'a>(manifest: &'a Value, path: &str) -> &'a Value {
    commands(manifest)
        .into_iter()
        .find(|c| c["path"] == path)
        .unwrap_or_else(|| panic!("{path} is not in the manifest"))
}

#[test]
fn the_manifest_matches_its_published_schema() {
    let schema: Value = serde_json::from_str(SCHEMA).expect("docs/manifest.schema.json is JSON");
    let errors = common::validate(&schema, &manifest());
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn the_structural_check_actually_rejects() {
    let schema: Value = serde_json::from_str(SCHEMA).unwrap();
    let mut broken = manifest();
    broken["commands"][0]["exitCodes"] = Value::String("nope".into());
    broken.as_object_mut().unwrap().remove("penvVersion");
    let errors = common::validate(&schema, &broken);
    assert_eq!(errors.len(), 2, "{errors:#?}");
}

#[test]
fn every_command_in_the_tree_has_its_own_metadata() {
    let missing: Vec<String> = command_paths()
        .into_iter()
        .filter(|path| !has_meta(path))
        .collect();
    assert!(missing.is_empty(), "no metadata for {missing:?}");
}

#[test]
fn the_manifest_carries_the_whole_command_table() {
    let manifest = manifest();
    let paths: Vec<&str> = commands(&manifest)
        .iter()
        .map(|c| c["path"].as_str().unwrap())
        .collect();
    for expected in [
        "init",
        "run",
        "push",
        "pull",
        "login",
        "logout",
        "set",
        "unset",
        "ls",
        "check",
        "gen",
        "guard",
        "reveal",
        "machine",
        "machine enroll",
        "upgrade",
        "completions",
        "help",
        "schema",
        "hook",
    ] {
        assert!(paths.contains(&expected), "{expected} is missing");
    }
    assert_eq!(manifest["schemaVersion"], MANIFEST_VERSION);
    assert_eq!(manifest["penvVersion"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn this_build_implements_local_mode_only() {
    let manifest = manifest();
    let implemented: Vec<&str> = commands(&manifest)
        .iter()
        .filter(|c| c["implemented"] == true)
        .map(|c| c["path"].as_str().unwrap())
        .collect();
    assert_eq!(implemented, ["init", "ls", "check", "schema", "help"]);
}

#[test]
fn the_policy_flags_say_what_the_design_says() {
    let manifest = manifest();
    let reveal = find(&manifest, "reveal");
    assert_eq!(reveal["revealsValues"], true);
    assert_eq!(reveal["requiresApproval"], true);
    assert_eq!(reveal["exitCodes"], serde_json::json!([0, 1, 2, 4, 5, 6]));

    let pull = find(&manifest, "pull");
    assert_eq!(pull["revealsValues"], true);
    let human: Vec<&str> = pull["flags"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["human"] == true)
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(human, ["i-am-human"]);

    let run = find(&manifest, "run");
    let env_flag = run["flags"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "env")
        .unwrap();
    assert_eq!(env_flag["env"], "PENV_ENV");
    assert_eq!(env_flag["takesValue"], true);
    assert_eq!(
        run["flags"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == "no-mask")
            .unwrap()["human"],
        true
    );
}

#[test]
fn the_exit_code_table_is_published_whole() {
    let manifest = manifest();
    let codes: Vec<i64> = manifest["exitCodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["code"].as_i64().unwrap())
        .collect();
    assert_eq!(codes, [0, 1, 2, 3, 4, 5, 6]);
}

#[test]
fn global_flags_are_listed_once() {
    let manifest = manifest();
    let names: Vec<&str> = manifest["globalFlags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["json", "agent"]);
    for command in commands(&manifest) {
        let flags: Vec<&str> = command["flags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect();
        assert!(
            !flags.contains(&"json") && !flags.contains(&"agent"),
            "{} repeats a global flag",
            command["path"]
        );
    }
}
