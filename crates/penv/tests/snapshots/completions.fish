# penv completions fish
function __fish_penv_path --description 'the penv command path typed so far'
    set -l tokens (commandline -opc)
    if set -q tokens[1]
        set -e tokens[1]
    end
    set -l path
    set -l skip 0
    for token in $tokens
        if test $skip -eq 1
            set skip 0
        else if test "$token" = '--'
            break
        else if string match -q -- '-*' $token
            if contains -- $token --format --guards --output --env --org --value --out --approval
                set skip 1
            end
        else
            set -a path $token
        end
    end
    string join ' ' $path
end

function __fish_penv_at --description 'true at one penv command path'
    set -l path (__fish_penv_path)
    test "$path" = "$argv[1]"
end

complete -c penv -f
complete -c penv -l json -d 'Emit JSON on stdout, whatever stdout is attached to'
complete -c penv -l format -r -d 'Pick the output format: json or text' -a 'json text'
complete -c penv -l agent -d 'Treat this session as an agent: JSON out, values masked'
complete -c penv -n '__fish_penv_at ""' -a 'init' -d 'Read .env, write .env.schema, and keep .env out of the repository'
complete -c penv -n '__fish_penv_at ""' -a 'run' -d 'Validate, then run a command with the values in its environment only'
complete -c penv -n '__fish_penv_at ""' -a 'push' -d 'Move local values to the cloud and delete .env'
complete -c penv -n '__fish_penv_at ""' -a 'pull' -d 'Write a plain .env from the cloud'
complete -c penv -n '__fish_penv_at ""' -a 'login' -d 'Sign in with a device code; the credential goes to the OS keychain'
complete -c penv -n '__fish_penv_at ""' -a 'logout' -d 'Remove the stored credential'
complete -c penv -n '__fish_penv_at ""' -a 'set' -d 'Write one value without echoing it'
complete -c penv -n '__fish_penv_at ""' -a 'unset' -d 'Remove one value'
complete -c penv -n '__fish_penv_at ""' -a 'ls' -d 'List keys, types and which ones have a value'
complete -c penv -n '__fish_penv_at ""' -a 'check' -d 'Report schema problems and missing values'
complete -c penv -n '__fish_penv_at ""' -a 'gen' -d 'Write the typed file for a language target'
complete -c penv -n '__fish_penv_at ""' -a 'guard' -d 'Write the harness rules that keep agents out of .env'
complete -c penv -n '__fish_penv_at ""' -a 'reveal' -d 'Print one value; an agent session needs a person'\''s approval'
complete -c penv -n '__fish_penv_at ""' -a 'machine' -d 'Machine identities'
complete -c penv -n '__fish_penv_at ""' -a 'upgrade' -d 'Replace this binary from the latest release'
complete -c penv -n '__fish_penv_at ""' -a 'completions' -d 'Print the shell completion script'
complete -c penv -n '__fish_penv_at ""' -a 'hook' -d 'Run as a harness hook; a payload it cannot read is refused'
complete -c penv -n '__fish_penv_at ""' -a 'schema' -d 'Print the schema as JSON'
complete -c penv -n '__fish_penv_at ""' -a 'help' -d 'Print the command manifest'
complete -c penv -n '__fish_penv_at ""' -l json -d 'Emit JSON on stdout, whatever stdout is attached to'
complete -c penv -n '__fish_penv_at ""' -l format -r -d 'Pick the output format: json or text' -a 'json text'
complete -c penv -n '__fish_penv_at ""' -l agent -d 'Treat this session as an agent: JSON out, values masked'
complete -c penv -n '__fish_penv_at "init"' -l force -d 'Overwrite an existing .env.schema'
complete -c penv -n '__fish_penv_at "init"' -l guards -r -d 'Guard exactly these harnesses instead of the installed ones'
complete -c penv -n '__fish_penv_at "init"' -l no-guards -d 'Write no harness rules at all'
complete -c penv -n '__fish_penv_at "init"' -l output -r -d 'Write the generated typed file here, relative to the repository root'
complete -c penv -n '__fish_penv_at "run"' -F -a '(__fish_complete_command)'
complete -c penv -n '__fish_penv_at "run"' -l env -r -d 'The environment to read'
complete -c penv -n '__fish_penv_at "run"' -l no-mask -d 'Leave the child'\''s output unmasked'
complete -c penv -n '__fish_penv_at "push"' -l env -r -d 'The environment to write to'
complete -c penv -n '__fish_penv_at "push"' -l org -r -d 'The organisation that owns a project penv is about to create'
complete -c penv -n '__fish_penv_at "push"' -l prune -d 'Delete cloud keys the schema no longer lists'
complete -c penv -n '__fish_penv_at "pull"' -l env -r -d 'The environment to read'
complete -c penv -n '__fish_penv_at "pull"' -l i-am-human -d 'Confirm a person, not an agent, asked for the file'
complete -c penv -n '__fish_penv_at "set"' -l env -r -d 'The environment to write to'
complete -c penv -n '__fish_penv_at "set"' -l value -r -d 'Refused: a value passed here lands in the shell history'
complete -c penv -n '__fish_penv_at "unset"' -l env -r -d 'The environment to write to'
complete -c penv -n '__fish_penv_at "gen"' -l out -r -d 'Write here instead, relative to the repository root'
complete -c penv -n '__fish_penv_at "gen"' -l check -d 'Compare with what is on disk instead of writing'
complete -c penv -n '__fish_penv_at "gen"' -l options -d 'Show what this target'\''s options change instead of writing'
complete -c penv -n '__fish_penv_at "guard"' -l all -d 'Write every harness penv knows, installed or not'
complete -c penv -n '__fish_penv_at "guard"' -l check -d 'Report coverage instead of writing'
complete -c penv -n '__fish_penv_at "reveal"' -l env -r -d 'The environment to read'
complete -c penv -n '__fish_penv_at "reveal"' -l approval -r -d 'Print the value a person approved under this id'
complete -c penv -n '__fish_penv_at "machine"' -a 'enroll' -d 'Bind a server keypair from a one-time secret'
complete -c penv -n '__fish_penv_at "upgrade"' -l check -d 'Report what the release carries instead of replacing anything'
complete -c penv -n '__fish_penv_at "completions"' -a 'bash zsh fish powershell elvish'
