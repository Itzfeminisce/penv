//! The override a repository keeps so a question is asked once, ever.

use crate::target::Knob;

/// A value an `[options]` knob was settled to.
pub type OptionValue = toml::Value;

/// What marks a comment line as penv's own. Every other `#` line was written by
/// hand, and penv rewrites nothing it did not write.
pub const MARK: &str = "# penv:";

/// The override that says the answer: the path, and every knob at the value in
/// effect with a line saying what it changes. The rest of the target is still
/// inherited from the folder it came from.
pub fn override_body(name: &str, output: &str, knobs: &[(&Knob, &OptionValue)]) -> String {
    let mut body = format!("name = {}\noutput = {}\n", quoted(name), quoted(output));
    if !knobs.is_empty() {
        body.push_str("\n[options]\n");
        for (knob, value) in knobs {
            body.push_str(&format!(
                "{MARK} {}\n{} = {value}\n",
                about(knob),
                knob.name
            ));
        }
    }
    body
}

/// The `[[option]]` block as one line: what it changes, then what it takes.
fn about(knob: &Knob) -> String {
    if knob.values.is_empty() {
        knob.about.clone()
    } else {
        format!("{} ({})", knob.about, knob.words().join("|"))
    }
}

/// True when a folder says more than penv would, or says why in a line penv did
/// not write. penv rewrites neither; `options` is every knob this target has.
pub fn hand_written(existing: &str, options: &[String]) -> bool {
    if existing.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with('#') && !line.starts_with(MARK)
    }) {
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

    fn knob(name: &str, default: OptionValue, values: &[OptionValue], about: &str) -> Knob {
        Knob {
            name: name.to_string(),
            default,
            values: values.to_vec(),
            about: about.to_string(),
        }
    }

    fn ts() -> Vec<Knob> {
        vec![
            knob(
                "key_case",
                OptionValue::String("upper".into()),
                &[
                    OptionValue::String("upper".into()),
                    OptionValue::String("camel".into()),
                ],
                "Property names in the exported object.",
            ),
            knob(
                "runtime",
                OptionValue::String("node".into()),
                &[
                    OptionValue::String("node".into()),
                    OptionValue::String("vite".into()),
                    OptionValue::String("deno".into()),
                ],
                "Where the values are read at run time.",
            ),
        ]
    }

    fn names(knobs: &[Knob]) -> Vec<String> {
        knobs.iter().map(|knob| knob.name.clone()).collect()
    }

    #[test]
    fn an_override_carries_the_answer_and_every_knob_that_answered_with_it() {
        assert_eq!(
            override_body("ts", "apps/web/src/env.ts", &[]),
            "name = \"ts\"\noutput = \"apps/web/src/env.ts\"\n"
        );

        let knobs = ts();
        let vite = OptionValue::String("vite".into());
        let body = override_body(
            "ts",
            "src/env.ts",
            &[(&knobs[0], &knobs[0].default), (&knobs[1], &vite)],
        );
        assert_eq!(
            body,
            "name = \"ts\"\noutput = \"src/env.ts\"\n\n[options]\n\
             # penv: Property names in the exported object. (upper|camel)\n\
             key_case = \"upper\"\n\
             # penv: Where the values are read at run time. (node|vite|deno)\n\
             runtime = \"vite\"\n"
        );

        // What penv writes reads back as the table it wrote.
        let read: toml::Table = body.parse().expect("the override parses");
        assert_eq!(read["options"]["runtime"].as_str(), Some("vite"));
    }

    #[test]
    fn a_knob_with_no_values_listed_says_only_what_it_changes() {
        let free = knob(
            "header",
            OptionValue::String(String::new()),
            &[],
            "The line written above the file.",
        );
        let value = OptionValue::String("// generated".into());
        assert!(
            override_body("go", "env.go", &[(&free, &value)])
                .contains("# penv: The line written above the file.\nheader = \"// generated\"\n"),
        );
    }

    #[test]
    fn what_penv_writes_is_never_hand_written() {
        let knobs = ts();
        let body = override_body(
            "ts",
            "src/env.ts",
            &[
                (&knobs[0], &knobs[0].default),
                (&knobs[1], &knobs[1].default),
            ],
        );
        assert!(!hand_written(&body, &names(&knobs)));
        assert!(!hand_written(
            "name = \"ts\"\noutput = \"a/b.ts\"\n",
            &names(&knobs)
        ));
        // An answer edited in place is still penv's file, and is read back.
        assert!(!hand_written(
            &body.replace("key_case = \"upper\"", "key_case = \"camel\""),
            &names(&knobs)
        ));
    }

    #[test]
    fn a_folder_that_says_more_or_says_why_is_left_alone() {
        let knobs = names(&ts());
        for existing in [
            "# ours, on purpose\nname = \"ts\"\noutput = \"a/b.ts\"\n",
            "name = \"ts\"\noutput = \"a/b.ts\"\n[types]\nstring = \"string\"\n",
            "name = \"ts\"\noutput = \"a/b.ts\"\n[options]\nformatter = \"biome\"\n",
            "name = \"ts\" output",
        ] {
            assert!(hand_written(existing, &knobs), "{existing}");
        }
    }
}
