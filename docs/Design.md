# penv v1 design

penv is a secrets manager. The cloud (penv.cloud) is the store. The `penv` binary gets the right values into the right process at the right time, validated, and never onto disk in the clear unless a human asks. This document is the contract every crate, target and docs page is built from. Decisions here were taken on 2026-09-07 and 2026-09-08 with the research in the penv-cloud repo (`docs/Research-Dotenv-Dialects-And-Typed-Env.md`, `docs/Research-Agent-Secret-Isolation.md`).

## 1. Principles

1. **The first minute needs no account.** `penv init` and `penv run` work on a plain `.env` before anyone logs in. The cloud is the upgrade that takes the second minute.
2. **`.env` is the interchange format, never the truth once pushed.** The file is regenerable. Losing it costs nothing.
3. **One binary, no runtime.** No Node, no plugins, no JavaScript config. A static binary per platform.
4. **Validation happens in the binary before the process starts.** A Go service with no adapter still gets a refusal naming the key.
5. **Types are generated, never a runtime.** Language targets are data folders (template plus type map). A new language is a folder.
6. **The agent is a principal.** Trusted to write code, not to hold credentials. Detection changes defaults and friction; it is never the last line of defence.
7. **Say only what is true.** The security claim is a string the binary owns and prints, and it differs by state.
8. **Drop-in or evict.** Every part is independent enough that removing it leaves the rest working, and adding a sibling touches no existing part. This applies to language targets, harness guards, credential kinds, output formats and commands.

## 2. Files in a repository

Exactly one penv file is committed: `.env.schema`. There is no config file.

```dotenv
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
```

Rules:
- The header is the first comment block, whether or not a blank line follows it; a first block that sits directly on a key and contains a header decorator is still the header. `@penv=<org>/<project>` names the cloud project; absent means local mode. `@schema=<n>` is the grammar version the file was written with.
- Decorators sit in `#` comment lines directly above a key. A comment line not starting with `@` is the key's description. The blank line ends a block.
- A decorator is `@name` or `@name=value`. Never positional. Order never matters. Quote a value that contains whitespace or `#`.
- Vocabulary follows `@env-spec` (varlock) verbatim: `@type=`, `@required`, `@optional`, `@sensitive`, `@sensitive=false`, `@defaultSensitive`, `@defaultRequired`, `@example`, `@docs`. Types: `string`, `number`, `boolean`, `url`, `email`, `port`, `enum(a,b,c)`, plus the spec's function-call constraints such as `string(startsWith=sk_)` and `number(isInt=true)`. There is no `integer` type; integers are `number(isInt=true)`, and a target's `[types]` map may carry an `integer` entry used when that constraint is set. Whitespace inside parentheses is allowed.
- Required is inferred: an empty value is required, a value present on the line is the default and makes the key optional. `@required`/`@optional` override.
- Sensitive is inferred: sensitive by default, public when the key carries a bundler prefix (`NEXT_PUBLIC_`, `VITE_`, `PUBLIC_`, `EXPO_PUBLIC_`, `NUXT_PUBLIC_`, `REACT_APP_`) or `@sensitive=false`. A key that is both prefixed and `@sensitive` fails `check`.
- Adopted from the spec as-is: `@deprecated`, and the boolean `@dynamic`/`@static` pair (accepted and preserved, not used by penv).
- penv extensions, only where the spec has no concept: `@since=<version>`, `@rotate=<duration>`, `@dynamicFrom=<engine>` (read-only marker rendered by the cloud). `@scope` does not exist; who reads a key is an authorization rule in the console.
- One concept, one name. No aliases. Unknown decorators are an error from `check`.
- Nearest `.env.schema` upward from the working directory wins. A monorepo holds one per app.

The generated `.env` (from `pull`, or the developer's own in local mode) is plain: UTF-8 without BOM, LF, `KEY=value`, upper snake case keys, no `export`, no spaces around `=`, quotes only when needed, no `$`, no escapes, no duplicates, no multi-line values, no comments emitted. That is the subset every parser in the research agrees on.

## 3. State machine

```text
local   .env on disk, .env.schema present, no @penv header or not logged in
cloud   @penv header, credential in the keychain, .env absent (or present only after an explicit pull)
```

`penv` with no arguments prints the state and the one next command.

## 4. Commands

| Command | Does | Fires automatically when |
|---|---|---|
| `penv` | Prints state and the one next command | no args |
| `init` | Reads `.env`, writes `.env.schema`, gitignores `.env`, prints what it inferred; never prompts. Every key is sensitive and required unless bundler-prefixed. A value is copied into the schema as a default only when the key is bundler-prefixed, or the value is a boolean, an integer, a lowercase word of letters, or a localhost URL with no userinfo and no query | `run` finds a `.env` with no schema |
| `run [--env E] -- cmd` | Validates, injects into the child only, masks child output when an agent is present | never |
| `push` | Moves local values to the cloud, deletes `.env` | `init` when logged in, as an offer |
| `pull` | Writes a plain `.env` | never |
| `login` / `logout` | Device-code sign in; credential in the OS keychain | `run`/`push` lack a credential and a human is at a TTY |
| `set KEY` / `unset KEY` | Prompted or piped write, never echoed | never |
| `ls` | Names, types, presence; values masked; JSON when stdout is not a TTY | never |
| `check [KEY]` | Schema validity, missing values, why a key fails, guard status | `run` before exec; `init` after import |
| `gen <target>` | Writes the typed file for a language target | `init`/`push` when a target's detect files exist |
| `guard [--check]` | Writes every recognised harness config; `--check` reports coverage | `init`; any command that detects a new harness |
| `reveal KEY` | Prints one value after console approval | never; refused outright in an agent session |
| `machine enroll <secret>` | Binds a server keypair from a one-time secret | never |
| `upgrade` | Replaces the binary from the signed GitHub release | any command reads a schema newer than it understands |
| `completions <shell>` | From the manifest | never |
| `help --json` | The command manifest | never |

Not commands: `env`/`use` (use `--env` or `PENV_ENV`, default `development`), `config`, `doctor` (it is `check`), `agent` (agents are detected).

### Environment selection
`--env`, else `PENV_ENV`, else `development`. A human identity running `production` is refused unless the console unlocked it for that environment. Machine identities read their bound environment only.

### Output contract
- Human at a TTY: aligned text, no spinners in non-TTY, `NO_COLOR` and `CLICOLOR=0` honoured.
- stdout not a TTY, or an agent marker present: JSON on stdout, one object; errors as JSON on stderr `{ "error": "<code>", "message": "...", "fix": "..." }`.
- `--json` forces JSON; `--agent` forces JSON plus masking and is a parse error combined with any raw-value format.
- Exit codes: 0 ok, 1 error, 2 auth, 3 validation, 4 confirmation required (JSON carries the exact replay command), 5 no credential, 6 environment refused. `help --json` publishes the table.

### The manifest
`penv help --json` emits every command with args, flags (`env` var alias, `default`, `human: true` for flags stripped under agent policy), `revealsValues`, `requiresApproval`, and per-command exit codes, plus `schemaVersion`. The docs generator, completions and the agent skill consume it. Nothing about commands is hand-written twice.

## 5. `run`

```text
find .env.schema (nearest upward)               <1ms
local mode: parse .env, validate, exec
cloud mode:
  open encrypted cache (keychain key)          ~2ms
  fresh (<60s dev, 0s otherwise)  -> exec
  HEAD /envs/:id with ETag                     ~40ms
  changed -> GET, rewrite cache
  offline -> dev: use cache, warn once a day; other envs: fail closed (exit 5)
no keychain (containers, servers) -> no cache, always online
```

Target: under 50ms to exec on a warm cache. Injection is into the child environment only. Output masking scrubs the child's stdout and stderr for every sensitive value, boundary-safe across chunk splits, plus base64 and JSON-escaped forms of each value. The masker's secret list is every value present in the resolved environment except keys the schema marks public; a `.env` key the schema does not list is masked and `check` names it as drift. Masking is on by default when an agent marker is present, and never TTY-gated in that case. `--no-mask` is a `human: true` flag honoured only when stdin and stdout are both terminals and no agent is detected.

## 6. Agents

Detection is advisory and ordered, because vendors collide:

1. `AGENT=amp`  2. `COPILOT_CLI=1`  3. `CLAUDE_CODE_CHILD_SESSION=1`  4. `CLAUDECODE=1`  5. `CODEX_THREAD_ID`/`CODEX_SESSION_ID`  6. `GEMINI_CLI=1`  7. `CURSOR_SANDBOX`/`CURSOR_AGENT`  8. `CLINE_ACTIVE`, `ROO_ACTIVE`/`ROO_CLI_RUNTIME`, `OR_APP_NAME=Aider`, `###PS1JSON###` in `PS1`, `AI_AGENT` (parse both `name_version_mode` and `name@version`), `AGENT` against an allowlist, `/opt/.devin`  9. none: process-ancestry walk. Non-TTY plus non-interactive `GIT_EDITOR` tightens, never loosens.

An agent session flips: JSON output, masking on, `reveal` refused, `pull` refused unless `--i-am-human` is passed by a person, shorter credential TTL, and the session id (`CLAUDE_CODE_SESSION_ID`, `CODEX_THREAD_ID`, `CURSOR_TRACE_ID`, `AMP_CURRENT_THREAD_ID`, `COPILOT_AGENT_SESSION_ID`) stamped on every cloud request for audit.

`guard` writes what each harness enforces, from `.env.schema`, idempotently and additively. Guards are folders (`guards/<harness>/`) with a `guard.toml` (detect paths, files to merge, scope, and the hook response shape the harness expects) and templates; the binary knows no harness by name, and `penv hook <harness>` renders the deny response from the folder. Deny patterns are `.env` and `.env.*` (never `.env.schema`, which the hook allows by name), so a new environment file is covered without a list; the binary merges JSON or TOML fragments without ever weakening an existing rule. Ranked: Claude Code (`.claude/settings.json` deny rules in project scope, `sandbox.credentials` mask block printed for user scope, static-binary PreToolUse hook), Codex (permission profile denying `**/*.env`, `ignore_default_excludes=false`), Cursor (`.cursor/cli.json` deny, `.cursor/hooks.json` with `failClosed`), Amp (`amp.guardedFiles.allowlist: []`), Copilot CLI (permissions config plus `--secret-env-vars` names), Gemini (`.gemini/settings.json` PreToolUse), Cline (`.clinerules/hooks/`), Windsurf (`.windsurf/hooks.json`). Native Windows has no Claude Code sandbox; `guard --check` says so.

The hook binary is `penv` itself (`penv hook claude-code`), never a script needing an interpreter, because a missing interpreter fails open.

## 7. Language targets

```text
targets/<name>/target.toml   name, output path, detect files, [types] map
targets/<name>/env.tmpl      minijinja template over the schema JSON
```

Lookup order: `.penv/targets/<name>/` in the repo, `~/.penv/targets/<name>/`, built in (ts, py). Same layout in all three. Targets receive `penv schema --json` and nothing else: no values, no network. One fixture schema is snapshot-rendered through every target in CI; `gen --check` compiles the output when the toolchain is present.

## 8. Cloud

The API already exists in penv-cloud (`/api/v1/secrets`, `/api/v1/auth/{oidc,aws,keypair,revoke}`, `/api/v1/dynamic`). A machine identity is bound to one project and environment with a role and proves itself by one of: `oidc` (platform JWT exchanged for a short-lived credential; the binary fixes the lifetime at 15 minutes), `aws-iam` (SigV4-signed STS request), `bound-keypair` (Ed25519 challenge-response with a generation counter, for hosts that can attest nothing), `token` (`pck_` bearer with a required expiry; the last resort). The variable is `PENV_TOKEN`.

Cloud-side: the schema is stored per key next to values; the console renders and edits it; `push` and `pull` carry it. Push targets (Vercel, Netlify, etc.) are cloud integrations, not CLI features. There is no fetch SDK.

## 9. Claim

```text
local:  penv validates your .env, keeps values out of your agent's output, and blocks it from reading the file where its harness allows.
cloud:  penv keeps secrets out of the files, the repo and the shell history your coding agent reads, and out of the output it captures. It cannot stop a process running as you from looking, so every value is short-lived, scoped and attributable to the session that used it.
```

Only the cloud sentence is marketed. Both are printed by `guard --check`.

## 10. Distribution

GitHub Releases are the source of truth: signed archives for linux x86_64/aarch64 (musl), macOS x86_64/aarch64, windows x86_64. Install paths: `curl -fsSL https://penv.cloud/install | sh`, Homebrew tap, winget, a Docker image `penvhq/cli`, and the npm name `@penvhq/cli` as a downloader shim. Release builds run only in CI; this development machine runs `cargo check` and `cargo test` with two jobs.

## 11. Crate layout

```text
crates/penv           the binary: clap commands, output, exit codes, manifest
crates/penv-schema    .env.schema parser, IR, JSON, validation (no I/O)
crates/penv-dotenv    the safe-subset .env reader and writer
crates/penv-agent     detection and policy (pure functions over an environment map)
crates/penv-mask      streaming scrubber
crates/penv-targets   target loading and rendering
crates/penv-guards    harness guard loading and merging
crates/penv-cloud     HTTP client, credential kinds, cache, keychain
```

Each crate depends only on `penv-schema` and the standard library unless the brief for that crate says otherwise. Keep the dependency set small: clap, serde, serde_json, toml, minijinja, thiserror, and for the cloud crate ureq with rustls, keyring, and a small AEAD. No async runtime.

## 12. Phases

1. Local mode: schema, dotenv, `init`, `check`, `ls`, `run` with detection and masking, `gen` (ts, py), `guard`, `help --json`, bare `penv`, CI on GitHub Actions.
2. Cloud mode: `login`, `push`, `pull`, `set`, `unset`, `reveal`, `machine enroll`, cache, environment refusal, audit stamping. penv-cloud is the first project through it, by hand.
3. Distribution: release workflow, installers, npm shim, `upgrade`, `completions`.
