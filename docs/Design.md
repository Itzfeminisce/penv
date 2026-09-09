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

The generated `.env` (from `pull`, or the developer's own in local mode) is plain: UTF-8 without BOM, LF, `KEY=value`, upper snake case keys, no `export`, no spaces around `=`, quotes only when needed, no `$`, no duplicates, no comments emitted.

Quoting is the subset Node's `util.parseEnv` and dotenv both read back, and no more: **inside double quotes, `\n` is an escape and nothing else is**. That is how a multi-line value such as a PEM key sits on one line. So the writer folds `\r\n` into `\n`, double-quotes a value that breaks lines escaping those newlines, and refuses it outright when it also holds a `"`, a `\` or a carriage return of its own, because none of those has a portable escape. A value holding a `"` or a `\` and no line break is single-quoted, where nothing is an escape, and is refused when it also holds a `'`. A value goes bare only when it holds no whitespace at all, no `#`, no quote and no backslash; anything else in between is double-quoted with nothing to escape. The reader still decodes `\r`, `\t`, `\"` and `\\` so a file another tool wrote is read rather than mangled, and warns once per value, naming the line and the column and never the character.

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
| `init` | Reads `.env` (writing an empty one when there is none), writes `.env.schema`, gitignores `.env`, prints what it inferred. Every key is sensitive and required unless bundler-prefixed. A value is copied into the schema as a default only when the key is bundler-prefixed, or the value is a boolean, an integer, a lowercase word of letters, a lowercase slug of at most 32 characters whose segments are joined by `-`, `_` or `.` and where one segment is letters only and no segment carrying a digit runs past four characters (`us-east-1`, `gpt-4o`, `api.internal`, never `a3f9c2d4e5b6`), or a localhost URL with no userinfo and no query. A key named for what it holds keeps its value out however dull it reads and whatever prefix it carries, so `NEXT_PUBLIC_SUPABASE_ANON_KEY` is not copied either: the words are `AUTH`, `KEY`, `SECRET`, `TOKEN`, `PASSWORD`, `PASSWD`, `PASSPHRASE`, `PASS`, `PWD`, `PW`, `CREDENTIAL`, `CRED`, `DSN`, `SALT`, `SEED` and `SIGNATURE`, each matching the word itself or a plural of it in `S` or `ES`; sensitivity still follows the prefix, since a bundler-prefixed value reaches the browser either way. At a terminal, with no agent, it offers a picker over every harness penv knows with the installed ones already chosen; `--guards <NAMES>` (trimmed, deduped, and none when it names nothing) and `--no-guards` decide it without a prompt, and every other run guards the installed set. It then generates for every target this repository uses, resolving the output the way `gen` does; `--output <PATH>` names the file instead, and only when one target applies | `run` finds a `.env` with no schema |
| `run [--env E] -- cmd` | Validates, injects into the child only, masks child output when an agent is present | never |
| `push` | Moves local values to the cloud, deletes `.env` | `init` when logged in, as an offer |
| `pull` | Writes a plain `.env` | never |
| `login` / `logout` | Device-code sign in; credential in the OS keychain | `run`/`push` lack a credential and a human is at a TTY |
| `set KEY` / `unset KEY` | Prompted or piped write, never echoed | never |
| `ls` | Names, types, presence; values masked; JSON when stdout is not a TTY | never |
| `check [KEY]` | Schema validity, missing values, why a key fails, guard status | `run` before exec; `init` after import |
| `gen <target>` | Writes the typed file for a language target and prints how to import it. `--out <PATH>` says where; without it penv asks at a terminal and skips anywhere else. Either answer is remembered. No target lists them with the directories they were detected in | `init`/`push` when a directory holds a target's detect file |
| `guard [--check]` | Writes every recognised harness config; `--check` reports coverage | `init`; any command that detects a new harness |
| `reveal KEY` | Prints one value after console approval | never; refused outright in an agent session |
| `machine enroll <secret>` | Binds a server keypair from a one-time secret | never |
| `upgrade` | Replaces this binary with the raw asset for its target from the latest GitHub release, after checking the digest the release publishes. `--check` reports what the release carries and changes nothing | never on its own; a schema this build cannot read names the command in its refusal |
| `completions <shell>` | Writes the completion script for bash, zsh, fish, powershell or elvish, generated from the manifest | never |
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
`penv help --json` emits every command with args, flags (`env` var alias, `default`, `human: true` for flags stripped under agent policy), the `values` an argument accepts where the list is fixed and the `completes` hint where the system answers instead, `revealsValues`, `requiresApproval`, and per-command exit codes, plus `schemaVersion`. The docs generator, completions and the agent skill consume it. Nothing about commands is hand-written twice.

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
targets/<name>/target.toml   name, output path, detect files, [types] map, [options] table,
                             [[suggest]] knobs, [[layout]] shapes, import line,
                             optional [check] command
targets/<name>/env.tmpl      minijinja template over the schema JSON
```

Lookup order: `.penv/targets/<name>/` in the repo, `~/.penv/targets/<name>/`, built in (ts, py). Same layout in all three, and the order is read through rather than winner-takes-all: a folder holding only a `target.toml` inherits the template from the next place, and a field that file does not set is inherited the same way, key by key inside a table as well, so an override naming one `output` or one `[options]` knob keeps the `[types]` map, the other knobs, the `[check]` command and the template it was going to use anyway. A folder holding an `env.tmpl` and no `target.toml` overrides nothing and is refused as the typo it is. `Target.source` is where the `target.toml` came from, and `Target.output_source` is where the `output` field came from, which is not always the same folder.

### Ask, never guess

Two things decide where a generated file goes, and nothing else ever does:

1. an explicit `--out` (`gen`) or `--output` (`init`), relative to the repository root;
2. the remembered repo override, but only when it names `output`.

Detection has two jobs and no third. It says whether a target is relevant to this repository at all, and it offers directories to choose from. A directory holding **any one** of the names in `detect` counts, walking three levels deep and skipping `node_modules`, `dist`, `build`, `target` and every directory whose name starts with a dot, which is how `.git` and `.next` are skipped without naming them. `ts` detects `package.json` or `tsconfig.json`; `py` detects `pyproject.toml`, `requirements.txt`, `setup.py` or `Pipfile`. A workspace root is a suggestion like any other, never a verdict about anything, and it is offered last: the rest come shallowest first, by name, so Enter in a monorepo lands on a package.

With neither source, at a terminal with no agent, penv asks once per relevant target:

```text
#  PATH
1  apps/web/src/env.ts
2  apps/api/src/env.ts

where should env.ts go? [apps/web/src/env.ts] (Enter, number, path, none):
```

The table is shown only when there is more than one suggestion. The default in brackets is the first suggestion joined with the target's `output`; Enter takes it, a number takes another off the list, a typed path is used as written relative to the repository root, and `none` skips the target. With neither source and nobody to ask, penv writes nothing for that target and reports it skipped, with the reason `pass --output <PATH>` (`--out` under `gen`). `gen --check` never asks either, and reports the same skip with exit 0.

The answer is remembered in `.penv/targets/<name>/target.toml` as the smallest override that says it — `name`, `output`, and an `[options]` block only for a knob that differs from the target's default — so a repository is asked once, ever. That holds for an explicit flag too, and for an answer that happens to equal the built-in default, because it was still an answer. A folder that says more than that, or that carries a `#` comment line, was written by hand and is never rewritten. `.penv/targets` is committed. Only the generated file moves: `.env.schema` and the gitignore stay at the root. A path outside the repository is refused rather than written, whether it arrived as an absolute path or as a `..`, and `.` and `..` are taken out of a path before anything downstream sees it.

A `[[layout]]` block shapes the output inside a chosen package: `when` is a path under the package holding one `/*/`, where the `*` stands for a directory name, `output` reads the same name back, and `root` is the directory the language imports from. A `when` with no `/*/` in it matches nothing and is refused when the folder loads. `py` uses one for src layouts, so a package holding `src/billing/__init__.py` is offered `src/billing/penv_env.py` and imports `from billing.penv_env import env`.

penv never edits a `tsconfig.json`, a `package.json` or any other build config. It prints the import line instead, from the `import` the target names: `{specifier}` is the path the language imports by and `{module}` its dotted form. When the target names a `paths_from` file, penv follows its relative `extends` chain, resolves each `paths` target against the effective `baseUrl`, and prints the alias when one is proven to reach the output; otherwise it prints the relative import.

Targets receive `penv schema --json` and nothing else: no values, no network. Whatever `[options]` holds reaches the template as `options`, unread by Rust, so a folder names its own knobs. A `[[suggest]]` block asks penv to work one out from the chosen package: `option` names the `[options]` key (its entry there is the default), and each `[[suggest.rule]]` carries the `value` it sets outright, from the `files` — optionally with the text they must `contain` — that imply it. A rule naming neither `files` nor `contains` is refused when the folder loads. `contains` reads only the lines of a file that are not its own comments, so a dependency somebody commented out is not one. The prompt offers the default first and then each rule's value once. penv asks only where a rule and the default disagree, and takes the default silently when nobody is there to ask; a `contains` rule never settles a knob unasked, so a non-interactive run remembers the path alone:

```text
which runtime reads the env? [vite] (node, vite, deno):
use pydantic types? [true] (false, true):
```

`ts` takes `key_case = "upper" | "camel"` for the property names it exports, and `runtime = "node" | "vite" | "deno"` for the one accessor the whole file reads through (`process.env[key]`, `import.meta.env[key]`, `Deno.env.get(key)`); a `vite.config.*` or a `deno.json` in the chosen package suggests the runtime. A key with a default in the schema reads through the accessor (`read("PORT") ?? "3000"`) instead of widening to `| undefined`, and the Standard Schema validator returns the typed `env`. `py` renders on the standard library alone, and `pydantic = true` swaps `HttpUrl` and `SecretStr` back in; a `pyproject.toml` listing pydantic suggests it.

One fixture schema is snapshot-rendered through every target in CI. `gen --check` compiles the output when the toolchain is present, and says which tool it looked for when it is not. `[check] command` and `probe` may offer alternatives in their first element as `python3|python|py`, and the first that both resolves and answers the probe wins; `[check] bin` names directories under the chosen package looked in before PATH, which is how `ts` finds a project's own `node_modules/.bin/tsc`. Resolution is PATHEXT-aware, so `tsc` finds `tsc.cmd` on Windows and not the shell script beside it. `[check.files]` carries whatever globals the output needs to compile.

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

GitHub Releases are the source of truth: archives for linux x86_64/aarch64 (musl), macOS x86_64/aarch64, windows x86_64, and beside each archive the raw binary `penv-<tag>-<target>`, which is what `upgrade` downloads because the binary carries no decompressor. One `penv-<tag>-<target>.sha256` covers both. `upgrade` verifies that digest; signing the checksum file is pending, and the verification site is marked for it. Install paths: `curl -fsSL https://penv.cloud/install | sh`, Homebrew tap, winget, a Docker image `penvhq/cli`, and the npm name `@penvhq/cli` as a downloader shim. Release builds run only in CI; this development machine runs `cargo check` and `cargo test` with two jobs.

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
