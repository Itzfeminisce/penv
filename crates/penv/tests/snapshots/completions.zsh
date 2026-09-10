#compdef penv
# penv completions zsh

_penv() {
    local -a commands
    commands=(
        'init:Read .env, write .env.schema, and keep .env out of the repository'
        'run:Validate, then run a command with the values in its environment only'
        'push:Move local values to the cloud and delete .env'
        'pull:Write a plain .env from the cloud'
        'login:Sign in with a device code; the credential goes to the OS keychain'
        'logout:Remove the stored credential'
        'set:Write one value without echoing it'
        'unset:Remove one value'
        'ls:List keys, types and which ones have a value'
        'check:Report schema problems and missing values'
        'gen:Write the typed file for a language target'
        'guard:Write the harness rules that keep agents out of .env'
        'reveal:Print one value; an agent session needs a person'\''s approval'
        'machine:Machine identities'
        'upgrade:Replace this binary from the latest release'
        'completions:Print the shell completion script'
        'hook:Run as a harness hook; a payload it cannot read is refused'
        'schema:Print the schema as JSON'
        'help:Print the command manifest'
    )
    local curcontext="$curcontext" state line
    _arguments -C \
        '--json[Emit JSON on stdout, whatever stdout is attached to]' \
        '--format[Pick the output format: json or text]:value:(json text)' \
        '--agent[Treat this session as an agent: JSON out, values masked]' \
        '1: :->command' \
        '*:: :->args' && return 0

    case $state in
        command) _describe -t commands 'penv command' commands ;;
        args)
            case $words[1] in
                init) _arguments '--force[Overwrite an existing .env.schema]' '--guards[Guard exactly these harnesses instead of the installed ones]:value:' '--no-guards[Write no harness rules at all]' '--output[Write the generated typed file here, relative to the repository root]:value:' '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                run) _arguments '--env[The environment to read]:value:' '--no-mask[Leave the child'\''s output unmasked]' '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' '*:command:_command_names -e' ;;
                push) _arguments '--env[The environment to write to]:value:' '--org[The organisation that owns a project penv is about to create]:value:' '--prune[Delete cloud keys the schema no longer lists]' '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                pull) _arguments '--env[The environment to read]:value:' '--i-am-human[Confirm a person, not an agent, asked for the file]' '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                login) _arguments '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                logout) _arguments '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                set) _arguments '--env[The environment to write to]:value:' '--value[Refused: a value passed here lands in the shell history]:value:' '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                unset) _arguments '--env[The environment to write to]:value:' '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                ls) _arguments '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                check) _arguments '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                gen) _arguments '--out[Write here instead, relative to the repository root]:value:' '--check[Compare with what is on disk instead of writing]' '--options[Show what this target'\''s options change instead of writing]' '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                guard) _arguments '--all[Write every harness penv knows, installed or not]' '--check[Report coverage instead of writing]' '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                reveal) _arguments '--env[The environment to read]:value:' '--approval[Print the value a person approved under this id]:value:' '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                machine) _penv_machine ;;
                upgrade) _arguments '--check[Report what the release carries instead of replacing anything]' '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                completions) _arguments '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' '1: :(bash zsh fish powershell elvish)' ;;
                hook) _arguments '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                schema) _arguments '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                help) _arguments '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                *) ;;
            esac ;;
    esac
}

_penv_machine() {
    local -a commands
    commands=(
        'enroll:Bind a server keypair from a one-time secret'
    )
    local curcontext="$curcontext" state line
    _arguments -C \
        '--json[Emit JSON on stdout, whatever stdout is attached to]' \
        '--format[Pick the output format: json or text]:value:(json text)' \
        '--agent[Treat this session as an agent: JSON out, values masked]' \
        '1: :->command' \
        '*:: :->args' && return 0

    case $state in
        command) _describe -t commands 'machine command' commands ;;
        args)
            case $words[1] in
                enroll) _arguments '--json[Emit JSON on stdout, whatever stdout is attached to]' '--format[Pick the output format: json or text]:value:(json text)' '--agent[Treat this session as an agent: JSON out, values masked]' ;;
                *) ;;
            esac ;;
    esac
}

_penv "$@"
