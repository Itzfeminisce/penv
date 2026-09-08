use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::Error;
use crate::folder::Source;

/// The schema base types a target must map. A target that misses one cannot
/// render every schema, so the loader refuses it. `integer` is not one of them:
/// it is the optional entry used for `number(isInt=true)`.
pub const BASE_TYPES: [&str; 7] = [
    "string", "number", "boolean", "url", "email", "port", "enum",
];

/// The `integer` entry, used when a number carries `isInt`.
pub const INT_TYPE: &str = "integer";

/// How `gen --check` compiles what the target rendered. `{file}` in an argument
/// is the rendered file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub command: Vec<String>,
    /// What proves the toolchain is installed; a failure is a skip, not an error.
    #[serde(default)]
    pub probe: Vec<String>,
    /// Extra files written beside the rendered one before the command runs.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    /// Where `gen` writes, relative to the directory holding `.env.schema`.
    pub output: String,
    /// Files whose presence says this target belongs in the repository.
    pub detect: Vec<String>,
    /// Schema base type name to a language type. The `enum` entry is a
    /// minijinja expression over `values`.
    pub types: BTreeMap<String, String>,
    pub check: Option<Check>,
    pub template: String,
    pub source: Source,
    pub dir: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    name: String,
    output: String,
    #[serde(default)]
    detect: Vec<String>,
    #[serde(default)]
    types: BTreeMap<String, String>,
    #[serde(default)]
    check: Option<Check>,
}

/// Read one target folder that has already been fetched into strings.
pub fn parse(
    name: &str,
    config: &str,
    template: &str,
    source: Source,
    dir: &str,
) -> Result<Target, Error> {
    let file: File = toml::from_str(config).map_err(|e| Error::Malformed {
        dir: dir.to_string(),
        message: e.message().to_string(),
    })?;

    if file.name != name {
        return Err(Error::Malformed {
            dir: dir.to_string(),
            message: format!("target.toml names {}, the folder names {name}", file.name),
        });
    }
    if file.output.trim().is_empty() {
        return Err(Error::Malformed {
            dir: dir.to_string(),
            message: "output is empty".into(),
        });
    }
    for base in BASE_TYPES {
        if !file.types.contains_key(base) {
            return Err(Error::Malformed {
                dir: dir.to_string(),
                message: format!("[types] has no entry for {base}"),
            });
        }
    }
    if file.check.as_ref().is_some_and(|c| c.command.is_empty()) {
        return Err(Error::Malformed {
            dir: dir.to_string(),
            message: "[check] has an empty command".into(),
        });
    }

    Ok(Target {
        name: file.name,
        output: file.output,
        detect: file.detect,
        types: file.types,
        check: file.check,
        template: template.to_string(),
        source,
        dir: dir.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TYPES: &str = r#"
[types]
string = "string"
number = "number"
integer = "number"
boolean = "boolean"
url = "string"
email = "string"
port = "number"
enum = "values | join(' | ')"
"#;

    fn config(head: &str) -> String {
        format!("{head}{TYPES}")
    }

    #[test]
    fn a_folder_of_toml_and_a_template_is_a_target() {
        let target = parse(
            "ts",
            &config("name = \"ts\"\noutput = \"src/env.ts\"\ndetect = [\"package.json\"]\n"),
            "hello",
            Source::Repo,
            ".penv/targets/ts",
        )
        .unwrap();
        assert_eq!(target.output, "src/env.ts");
        assert_eq!(target.detect, ["package.json"]);
        assert_eq!(target.types["port"], "number");
        assert_eq!(target.source, Source::Repo);
    }

    #[test]
    fn the_folder_name_and_the_declared_name_have_to_agree() {
        let error = parse(
            "py",
            &config("name = \"ts\"\noutput = \"src/env.ts\"\n"),
            "",
            Source::Repo,
            ".penv/targets/py",
        )
        .unwrap_err();
        assert!(error.to_string().contains("names ts"));
    }

    #[test]
    fn a_missing_base_type_is_refused_before_anything_renders() {
        let error = parse(
            "ts",
            "name = \"ts\"\noutput = \"src/env.ts\"\n[types]\nstring = \"string\"\n",
            "",
            Source::BuiltIn,
            "ts",
        )
        .unwrap_err();
        assert!(error.to_string().contains("no entry for"));
    }

    #[test]
    fn the_integer_entry_is_optional_because_the_schema_has_no_integer_type() {
        let target = parse(
            "go",
            &config("name = \"go\"\noutput = \"env.go\"\n").replace("integer = \"number\"\n", ""),
            "",
            Source::Repo,
            ".penv/targets/go",
        )
        .unwrap();
        assert!(!target.types.contains_key(INT_TYPE));
    }

    #[test]
    fn the_check_command_is_data_the_folder_carries() {
        let target = parse(
            "ts",
            &config(
                "name = \"ts\"\noutput = \"src/env.ts\"\n[check]\ncommand = [\"tsc\", \"{file}\"]\nprobe = [\"tsc\", \"--version\"]\n",
            ),
            "",
            Source::Repo,
            ".penv/targets/ts",
        )
        .unwrap();
        let check = target.check.unwrap();
        assert_eq!(check.command, ["tsc", "{file}"]);
        assert_eq!(check.probe, ["tsc", "--version"]);
    }

    #[test]
    fn an_unknown_field_is_a_typo_not_an_extension() {
        let error = parse(
            "ts",
            &config("name = \"ts\"\noutput = \"a\"\ndetects = []\n"),
            "",
            Source::Repo,
            "ts",
        )
        .unwrap_err();
        assert!(matches!(error, Error::Malformed { .. }));
    }
}
