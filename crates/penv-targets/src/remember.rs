//! The override a repository keeps so a question is asked once, ever.

/// A value an `[options]` knob was settled to.
pub type OptionValue = toml::Value;

/// The smallest override that says an answer: the rest of the target is still
/// inherited from the folder it came from.
pub fn override_body(name: &str, output: &str, options: &[(String, OptionValue)]) -> String {
    let mut body = format!("name = {}\noutput = {}\n", quoted(name), quoted(output));
    if !options.is_empty() {
        body.push_str("\n[options]\n");
        for (option, value) in options {
            body.push_str(&format!("{option} = {value}\n"));
        }
    }
    body
}

/// True when a folder says more than penv would, or says why. penv rewrites
/// neither; `options` is what this target's `[[suggest]]` blocks may set.
pub fn hand_written(existing: &str, options: &[String]) -> bool {
    if existing
        .lines()
        .any(|line| line.trim_start().starts_with('#'))
    {
        return true;
    }
    let Ok(table) = existing.parse::<toml::Table>() else {
        return true;
    };
    table.iter().any(|(field, value)| match field.as_str() {
        "name" | "output" => false,
        "options" => match value.as_table() {
            None => true,
            Some(held) => held.keys().any(|key| !options.contains(key)),
        },
        _ => true,
    })
}

fn quoted(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Vec<String> {
        vec!["runtime".to_string()]
    }

    #[test]
    fn an_override_carries_the_answer_and_nothing_else() {
        assert_eq!(
            override_body("ts", "apps/web/src/env.ts", &[]),
            "name = \"ts\"\noutput = \"apps/web/src/env.ts\"\n"
        );
        assert_eq!(
            override_body(
                "ts",
                "src/env.ts",
                &[("runtime".into(), OptionValue::String("vite".into()))]
            ),
            "name = \"ts\"\noutput = \"src/env.ts\"\n\n[options]\nruntime = \"vite\"\n"
        );
        assert_eq!(
            override_body(
                "py",
                "penv_env.py",
                &[("pydantic".into(), OptionValue::Boolean(true))]
            ),
            "name = \"py\"\noutput = \"penv_env.py\"\n\n[options]\npydantic = true\n"
        );
    }

    #[test]
    fn what_penv_writes_is_never_hand_written() {
        let body = override_body(
            "ts",
            "src/env.ts",
            &[("runtime".into(), OptionValue::String("vite".into()))],
        );
        assert!(!hand_written(&body, &options()));
        assert!(!hand_written(
            "name = \"ts\"\noutput = \"a/b.ts\"\n",
            &options()
        ));
    }

    #[test]
    fn a_folder_that_says_more_or_says_why_is_left_alone() {
        for existing in [
            "# ours, on purpose\nname = \"ts\"\noutput = \"a/b.ts\"\n",
            "name = \"ts\"\noutput = \"a/b.ts\"\n[types]\nstring = \"string\"\n",
            "name = \"ts\"\noutput = \"a/b.ts\"\n[options]\nkey_case = \"camel\"\n",
            "name = \"ts\" output",
        ] {
            assert!(hand_written(existing, &options()), "{existing}");
        }
    }
}
