//! The completion scripts, generated from the manifest. One small generator per
//! shell; nothing about a command is written here.

use serde_json::Value;

use crate::error::CliError;

/// The shells penv writes a script for.
pub const SHELLS: [&str; 5] = ["bash", "zsh", "fish", "powershell", "elvish"];

/// The script for one shell, over the manifest `help --json` publishes.
pub fn script(shell: &str, manifest: &Value) -> Result<String, CliError> {
    let nodes = tree(manifest);
    match shell {
        "bash" => Ok(bash(&nodes)),
        "zsh" => Ok(zsh(&nodes)),
        "fish" => Ok(fish(&nodes)),
        "powershell" => Ok(powershell(&nodes)),
        "elvish" => Ok(elvish(&nodes)),
        other => Err(CliError::new(
            "unknown_shell",
            format!("penv writes no completion script for {other}."),
            format!("Pass one of {}.", SHELLS.join(", ")),
        )),
    }
}

struct Flag {
    long: String,
    about: String,
    takes_value: bool,
    values: Vec<String>,
}

/// One command, flattened: what it offers and what it is called.
struct Node {
    path: Vec<String>,
    subs: Vec<(String, String)>,
    flags: Vec<Flag>,
    /// The word list of the first positional, where it has one.
    values: Vec<String>,
    /// True when the first positional is another program's command line, which
    /// the shell completes better than any word list could.
    takes_command: bool,
}

impl Node {
    fn path(&self) -> String {
        self.path.join(" ")
    }

    fn name(&self) -> &str {
        self.path.last().map(String::as_str).unwrap_or("penv")
    }

    /// Everything a shell would offer here: subcommands, positional words, flags.
    fn words(&self) -> Vec<String> {
        self.subs
            .iter()
            .map(|(name, _)| name.clone())
            .chain(self.values.iter().cloned())
            .chain(self.flags.iter().map(|f| f.long.clone()))
            .collect()
    }
}

fn tree(manifest: &Value) -> Vec<Node> {
    let mut nodes = vec![Node {
        path: Vec::new(),
        subs: subs_of(&manifest["commands"]),
        flags: flags_of(&manifest["globalFlags"]),
        values: Vec::new(),
        takes_command: false,
    }];
    walk(&manifest["commands"], &[], &mut nodes);
    nodes
}

fn walk(commands: &Value, prefix: &[String], nodes: &mut Vec<Node>) {
    for command in commands.as_array().into_iter().flatten() {
        let mut path = prefix.to_vec();
        path.push(text(&command["name"]));
        let first = command["args"].as_array().and_then(|args| args.first());
        nodes.push(Node {
            subs: subs_of(&command["subcommands"]),
            flags: flags_of(&command["flags"]),
            values: first.map(|arg| strings(&arg["values"])).unwrap_or_default(),
            takes_command: first.is_some_and(|arg| arg["completes"] == "command"),
            path: path.clone(),
        });
        walk(&command["subcommands"], &path, nodes);
    }
}

fn subs_of(commands: &Value) -> Vec<(String, String)> {
    commands
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| (text(&c["name"]), text(&c["about"])))
        .collect()
}

fn flags_of(flags: &Value) -> Vec<Flag> {
    flags
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            Some(Flag {
                long: f["long"].as_str()?.to_string(),
                about: text(&f["about"]),
                takes_value: f["takesValue"].as_bool().unwrap_or(false),
                values: strings(&f["values"]),
            })
        })
        .collect()
}

fn text(value: &Value) -> String {
    value
        .as_str()
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

/// A description inside a single-quoted shell string.
fn sq(text: &str) -> String {
    text.replace('\'', "'\\''")
}

/// A description inside a zsh `[...]` action, where the brackets close it early.
fn zq(text: &str) -> String {
    sq(text).replace('[', "\\[").replace(']', "\\]")
}

/// A description inside a PowerShell or Elvish single-quoted string.
fn dq(text: &str) -> String {
    text.replace('\'', "''")
}

/// Every flag that takes a value, with the word list it accepts, deduplicated
/// across the tree: the shells match on the flag alone.
fn valued_flags(nodes: &[Node]) -> Vec<&Flag> {
    let mut out: Vec<&Flag> = Vec::new();
    for flag in nodes.iter().flat_map(|n| n.flags.iter()) {
        if flag.takes_value && !out.iter().any(|f| f.long == flag.long) {
            out.push(flag);
        }
    }
    out
}

/// The global flags belong to every command but the root, which lists them itself.
fn globals_for<'a>(node: &Node, nodes: &'a [Node]) -> &'a [Flag] {
    if node.path.is_empty() {
        &[]
    } else {
        &nodes[0].flags
    }
}

fn bash(nodes: &[Node]) -> String {
    let known = nodes
        .iter()
        .skip(1)
        .map(|n| {
            let path = n.path();
            if path.contains(' ') {
                format!("\"{path}\"")
            } else {
                path
            }
        })
        .collect::<Vec<_>>()
        .join("|");
    let valued = valued_flags(nodes)
        .iter()
        .map(|f| f.long.clone())
        .collect::<Vec<_>>()
        .join("|");

    let mut value_arms = String::new();
    for flag in valued_flags(nodes) {
        let reply = if flag.values.is_empty() {
            "COMPREPLY=()".to_string()
        } else {
            format!(
                "COMPREPLY=($(compgen -W \"{}\" -- \"$cur\"))",
                flag.values.join(" ")
            )
        };
        value_arms.push_str(&format!("        {}) {reply}; return ;;\n", flag.long));
    }

    let mut arms = String::new();
    for node in nodes {
        let mut words = node.words();
        words.extend(globals_for(node, nodes).iter().map(|f| f.long.clone()));
        let path = node.path();
        let pattern = match path.as_str() {
            "" => "\"\"".to_string(),
            path if path.contains(' ') => format!("\"{path}\""),
            path => path.to_string(),
        };
        // -c offers the executables the argument after this command names.
        let system = if node.takes_command {
            "; system=-c"
        } else {
            ""
        };
        arms.push_str(&format!(
            "        {pattern}) words=\"{}\"{system} ;;\n",
            words.join(" ")
        ));
    }

    format!(
        "# penv completions bash\n\
         _penv() {{\n\
         \x20   local cur prev cmd word skip i words system\n\
         \x20   cur=\"${{COMP_WORDS[COMP_CWORD]}}\"\n\
         \x20   prev=\"${{COMP_WORDS[COMP_CWORD-1]}}\"\n\
         \x20   cmd=\"\"\n\
         \x20   skip=\"\"\n\
         \x20   for ((i = 1; i < COMP_CWORD; i++)); do\n\
         \x20       word=\"${{COMP_WORDS[i]}}\"\n\
         \x20       if [ -n \"$skip\" ]; then skip=\"\"; continue; fi\n\
         \x20       case \"$word\" in\n\
         \x20           {valued}) skip=1; continue ;;\n\
         \x20           -*) continue ;;\n\
         \x20       esac\n\
         \x20       case \"${{cmd:+$cmd }}$word\" in\n\
         \x20           {known}) cmd=\"${{cmd:+$cmd }}$word\" ;;\n\
         \x20           *) break ;;\n\
         \x20       esac\n\
         \x20   done\n\
         \n\
         \x20   case \"$prev\" in\n\
         {value_arms}\
         \x20   esac\n\
         \n\
         \x20   words=\"\"\n\
         \x20   system=\"\"\n\
         \x20   case \"$cmd\" in\n\
         {arms}\
         \x20   esac\n\
         \x20   COMPREPLY=($(compgen -W \"$words\" $system -- \"$cur\"))\n\
         }}\n\
         complete -o default -F _penv penv\n"
    )
}

fn zsh(nodes: &[Node]) -> String {
    let mut out = String::from("#compdef penv\n# penv completions zsh\n");
    for node in nodes.iter().filter(|n| !n.subs.is_empty()) {
        out.push_str(&zsh_function(node, nodes));
    }
    out.push_str("\n_penv \"$@\"\n");
    out
}

/// A command that carries subcommands gets a function; its leaves are inline
/// `_arguments` calls in the one case statement.
fn zsh_function(node: &Node, nodes: &[Node]) -> String {
    let mut out = format!(
        "\n{}() {{\n    local -a commands\n    commands=(\n",
        zsh_name(node)
    );
    for (name, about) in &node.subs {
        out.push_str(&format!("        '{}:{}'\n", sq(name), sq(about)));
    }
    out.push_str("    )\n    local curcontext=\"$curcontext\" state line\n    _arguments -C \\\n");
    for spec in zsh_specs(node, nodes) {
        out.push_str(&format!("        {spec} \\\n"));
    }
    out.push_str(
        "        '1: :->command' \\\n        '*:: :->args' && return 0\n\n    case $state in\n",
    );
    out.push_str(&format!(
        "        command) _describe -t commands '{} command' commands ;;\n",
        sq(node.name())
    ));
    out.push_str("        args)\n            case $words[1] in\n");
    for (name, _) in &node.subs {
        let mut path = node.path.clone();
        path.push(name.clone());
        let child = nodes
            .iter()
            .find(|n| n.path == path)
            .expect("every subcommand is a node");
        let body = if child.subs.is_empty() {
            zsh_leaf(child, nodes)
        } else {
            format!("{} ", zsh_name(child))
        };
        out.push_str(&format!("                {}) {body};;\n", sq(name)));
    }
    out.push_str("                *) ;;\n            esac ;;\n    esac\n}\n");
    out
}

fn zsh_leaf(node: &Node, nodes: &[Node]) -> String {
    let mut specs = zsh_specs(node, nodes);
    if !node.values.is_empty() {
        specs.push(format!(
            "'1: :({})'",
            node.values
                .iter()
                .map(|v| sq(v))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    if node.takes_command {
        specs.push("'*:command:_command_names -e'".to_string());
    }
    if specs.is_empty() {
        return String::new();
    }
    format!("_arguments {} ", specs.join(" "))
}

/// Every flag this command answers to, its own and the global ones.
fn zsh_specs(node: &Node, nodes: &[Node]) -> Vec<String> {
    node.flags
        .iter()
        .chain(globals_for(node, nodes))
        .map(zsh_flag)
        .collect()
}

fn zsh_flag(flag: &Flag) -> String {
    let action = if flag.values.is_empty() {
        if flag.takes_value { ":value:" } else { "" }
    } else {
        return format!(
            "'{}[{}]:value:({})'",
            flag.long,
            zq(&flag.about),
            flag.values
                .iter()
                .map(|v| sq(v))
                .collect::<Vec<_>>()
                .join(" ")
        );
    };
    format!("'{}[{}]{action}'", flag.long, zq(&flag.about))
}

fn zsh_name(node: &Node) -> String {
    let mut name = String::from("_penv");
    for word in &node.path {
        name.push('_');
        name.push_str(&word.replace('-', "_"));
    }
    name
}

fn fish(nodes: &[Node]) -> String {
    let valued = valued_flags(nodes)
        .iter()
        .map(|f| f.long.clone())
        .collect::<Vec<_>>()
        .join(" ");
    // The command path, not the words seen anywhere, so a name that is also a
    // subcommand of another command completes for the one that was typed.
    let mut out = format!(
        "# penv completions fish\n\
         function __fish_penv_path --description 'the penv command path typed so far'\n\
         \x20   set -l tokens (commandline -opc)\n\
         \x20   if set -q tokens[1]\n\
         \x20       set -e tokens[1]\n\
         \x20   end\n\
         \x20   set -l path\n\
         \x20   set -l skip 0\n\
         \x20   for token in $tokens\n\
         \x20       if test $skip -eq 1\n\
         \x20           set skip 0\n\
         \x20       else if test \"$token\" = '--'\n\
         \x20           break\n\
         \x20       else if string match -q -- '-*' $token\n\
         \x20           if contains -- $token {valued}\n\
         \x20               set skip 1\n\
         \x20           end\n\
         \x20       else\n\
         \x20           set -a path $token\n\
         \x20       end\n\
         \x20   end\n\
         \x20   string join ' ' $path\n\
         end\n\n\
         function __fish_penv_at --description 'true at one penv command path'\n\
         \x20   set -l path (__fish_penv_path)\n\
         \x20   test \"$path\" = \"$argv[1]\"\n\
         end\n\n\
         complete -c penv -f\n"
    );
    for flag in &nodes[0].flags {
        out.push_str(&fish_flag(flag, None));
    }
    for node in nodes {
        let condition = format!("__fish_penv_at \"{}\"", node.path());
        for (name, about) in &node.subs {
            out.push_str(&format!(
                "complete -c penv -n '{condition}' -a '{}' -d '{}'\n",
                sq(name),
                sq(about)
            ));
        }
        if !node.values.is_empty() {
            out.push_str(&format!(
                "complete -c penv -n '{condition}' -a '{}'\n",
                sq(&node.values.join(" "))
            ));
        }
        if node.takes_command {
            out.push_str(&format!(
                "complete -c penv -n '{condition}' -F -a '(__fish_complete_command)'\n"
            ));
        }
        for flag in &node.flags {
            out.push_str(&fish_flag(flag, Some(&condition)));
        }
    }
    out
}

fn fish_flag(flag: &Flag, condition: Option<&str>) -> String {
    let mut line = String::from("complete -c penv");
    if let Some(condition) = condition {
        line.push_str(&format!(" -n '{condition}'"));
    }
    line.push_str(&format!(" -l {}", flag.long.trim_start_matches("--")));
    if flag.takes_value {
        line.push_str(" -r");
    }
    line.push_str(&format!(" -d '{}'", sq(&flag.about)));
    if !flag.values.is_empty() {
        line.push_str(&format!(" -a '{}'", sq(&flag.values.join(" "))));
    }
    line.push('\n');
    line
}

fn powershell(nodes: &[Node]) -> String {
    let mut valued = String::new();
    for flag in valued_flags(nodes) {
        valued.push_str(&format!(
            "        '{}' = @({})\n",
            dq(&flag.long),
            flag.values
                .iter()
                .map(|v| format!("'{}'", dq(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let mut out = format!(
        "# penv completions powershell\n\
         using namespace System.Management.Automation\n\
         using namespace System.Management.Automation.Language\n\n\
         Register-ArgumentCompleter -Native -CommandName 'penv', 'penv.exe' -ScriptBlock {{\n\
         \x20   param($wordToComplete, $commandAst, $cursorPosition)\n\n\
         \x20   $valued = @{{\n\
         {valued}\
         \x20   }}\n\n\
         \x20   $words = @($commandAst.CommandElements | Select-Object -Skip 1 | Where-Object {{ $_ -is [StringConstantExpressionAst] }} | ForEach-Object {{ $_.Value }})\n\
         \x20   $previous = ''\n\
         \x20   if ($words.Count -gt 0) {{\n\
         \x20       if ($words[-1] -eq $wordToComplete) {{\n\
         \x20           if ($words.Count -gt 1) {{ $previous = $words[-2] }}\n\
         \x20       }} else {{\n\
         \x20           $previous = $words[-1]\n\
         \x20       }}\n\
         \x20   }}\n\
         \x20   if ($valued.ContainsKey($previous)) {{\n\
         \x20       $valued[$previous] | Where-Object {{ $_ -like \"$wordToComplete*\" }} | ForEach-Object {{\n\
         \x20           [CompletionResult]::new($_, $_, [CompletionResultType]::ParameterValue, $_)\n\
         \x20       }}\n\
         \x20       return\n\
         \x20   }}\n\n\
         \x20   $command = @(\n\
         \x20       'penv'\n\
         \x20       $skip = $false\n\
         \x20       foreach ($word in $words) {{\n\
         \x20           if ($word -eq $wordToComplete) {{ break }}\n\
         \x20           if ($skip) {{ $skip = $false; continue }}\n\
         \x20           if ($word.StartsWith('-')) {{\n\
         \x20               if ($valued.ContainsKey($word)) {{ $skip = $true }}\n\
         \x20               continue\n\
         \x20           }}\n\
         \x20           $word\n\
         \x20       }}\n\
         \x20   ) -join ';'\n\n\
         \x20   $completions = @(switch ($command) {{\n"
    );
    for node in nodes {
        let mut path = vec!["penv".to_string()];
        path.extend(node.path.clone());
        out.push_str(&format!("        '{}' {{\n", path.join(";")));
        for (name, about) in &node.subs {
            out.push_str(&powershell_item(name, about));
        }
        for value in &node.values {
            out.push_str(&powershell_item(value, value));
        }
        for flag in node.flags.iter().chain(globals_for(node, nodes)) {
            out.push_str(&powershell_item(&flag.long, &flag.about));
        }
        out.push_str("            break\n        }\n");
    }
    out.push_str(
        "    })\n\n\
         \x20   $completions | Where-Object { $_.CompletionText -like \"$wordToComplete*\" }\n\
         }\n",
    );
    out
}

fn powershell_item(word: &str, about: &str) -> String {
    format!(
        "            [CompletionResult]::new('{}', '{}', [CompletionResultType]::ParameterValue, '{}')\n",
        dq(word),
        dq(word),
        dq(about)
    )
}

fn elvish(nodes: &[Node]) -> String {
    let valued = valued_flags(nodes)
        .iter()
        .map(|flag| {
            format!(
                "&'{}'=[{}]",
                dq(&flag.long),
                flag.values
                    .iter()
                    .map(|v| format!("'{}'", dq(v)))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join(" ");

    let mut out = format!(
        "# penv completions elvish\n\
         use builtin;\n\
         use str;\n\n\
         set edit:completion:arg-completer[penv] = {{|@words|\n\
         \x20   fn cand {{|text desc|\n\
         \x20       edit:complex-candidate $text &display=$text' '$desc\n\
         \x20   }}\n\
         \x20   var valued = [{valued}]\n\
         \x20   var previous = ''\n\
         \x20   if (> (count $words) 1) {{ set previous = $words[-2] }}\n\
         \x20   var command = 'penv'\n\
         \x20   var skip = $false\n\
         \x20   for word $words[1..-1] {{\n\
         \x20       if $skip {{ set skip = $false; continue }}\n\
         \x20       if (str:has-prefix $word '-') {{\n\
         \x20           if (has-key $valued $word) {{ set skip = $true }}\n\
         \x20           continue\n\
         \x20       }}\n\
         \x20       set command = $command';'$word\n\
         \x20   }}\n\
         \x20   if (has-key $valued $previous) {{\n\
         \x20       for value $valued[$previous] {{ cand $value $value }}\n\
         \x20   }} else {{\n"
    );
    for node in nodes {
        let mut path = vec!["penv".to_string()];
        path.extend(node.path.clone());
        out.push_str(&format!(
            "        if (eq $command '{}') {{\n",
            path.join(";")
        ));
        for (name, about) in &node.subs {
            out.push_str(&format!(
                "            cand '{}' '{}'\n",
                dq(name),
                dq(about)
            ));
        }
        for value in &node.values {
            out.push_str(&format!(
                "            cand '{}' '{}'\n",
                dq(value),
                dq(value)
            ));
        }
        for flag in node.flags.iter().chain(globals_for(node, nodes)) {
            out.push_str(&format!(
                "            cand '{}' '{}'\n",
                dq(&flag.long),
                dq(&flag.about)
            ));
        }
        out.push_str("        }\n");
    }
    out.push_str("    }\n}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::manifest;

    fn nodes() -> Vec<Node> {
        tree(&manifest())
    }

    fn node<'a>(nodes: &'a [Node], path: &str) -> &'a Node {
        nodes
            .iter()
            .find(|n| n.path() == path)
            .expect("a command in the tree")
    }

    #[test]
    fn an_unknown_shell_names_the_five_that_work() {
        let refused = script("nushell", &manifest()).unwrap_err();
        assert_eq!(refused.code, "unknown_shell");
        for shell in SHELLS {
            assert!(refused.fix.contains(shell), "{refused:?}");
        }
    }

    #[test]
    fn every_shell_completes_the_nested_command_and_the_format_words() {
        for shell in SHELLS {
            let out = script(shell, &manifest()).unwrap();
            assert!(out.contains("machine"), "{shell} forgot machine");
            assert!(out.contains("enroll"), "{shell} forgot machine enroll");
            // fish spells a long flag `-l format`, so the name is what every shell shares.
            assert!(out.contains("format"), "{shell} forgot the global flags");
            assert!(out.contains("json"), "{shell} forgot the format words");
            assert!(out.contains("elvish"), "{shell} forgot the shell words");
        }
    }

    #[test]
    fn the_run_command_is_the_one_the_shell_finishes_itself() {
        let nodes = nodes();
        assert!(node(&nodes, "run").takes_command);
        assert!(!node(&nodes, "guard").takes_command);
        assert!(!node(&nodes, "completions").takes_command);
    }

    #[test]
    fn every_shell_hands_run_to_its_own_command_and_file_completion() {
        assert!(bash(&nodes()).contains("complete -o default -F _penv penv"));
        assert!(bash(&nodes()).contains("system=-c"));
        assert!(zsh(&nodes()).contains("'*:command:_command_names -e'"));
        assert!(fish(&nodes()).contains("-n '__fish_penv_at \"run\"' -F"));
    }

    #[test]
    fn a_valued_flag_offers_its_words_in_every_shell() {
        let nodes = nodes();
        assert!(bash(&nodes).contains("--format) COMPREPLY=($(compgen -W \"json text\""));
        assert!(
            zsh(&nodes)
                .contains("'--format[Pick the output format: json or text]:value:(json text)'")
        );
        assert!(
            fish(&nodes)
                .contains("-l format -r -d 'Pick the output format: json or text' -a 'json text'")
        );
        assert!(powershell(&nodes).contains("'--format' = @('json', 'text')"));
        assert!(elvish(&nodes).contains("&'--format'=['json' 'text']"));
    }

    #[test]
    fn a_value_never_becomes_a_command_in_the_shells_that_scan_the_line() {
        let nodes = nodes();
        assert!(bash(&nodes).contains("skip=1; continue"));
        assert!(powershell(&nodes).contains("if ($valued.ContainsKey($word)) { $skip = $true }"));
        assert!(elvish(&nodes).contains("if (has-key $valued $word) { set skip = $true }"));
    }

    #[test]
    fn the_global_flags_reach_every_subcommand() {
        let nodes = nodes();
        let zsh = zsh(&nodes);
        let leaf = zsh
            .lines()
            .find(|line| line.trim_start().starts_with("ls) "))
            .expect("ls is a leaf of the root function");
        assert!(leaf.contains("--json"), "{leaf}");
        assert!(leaf.contains("--agent"), "{leaf}");
        assert!(
            zsh.contains("_penv_machine() {"),
            "machine keeps its own function"
        );
    }

    #[test]
    fn powershell_registers_the_name_windows_actually_runs() {
        assert!(powershell(&nodes()).contains("-CommandName 'penv', 'penv.exe'"));
    }
}
