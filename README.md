<h1 align="center">Penv CLI (Official CLI of the penv.cloud platform)</h1>

<p align="center">
  <strong>Your <code>.env</code>, validated, typed, and kept out of your coding agent's reach.</strong><br>
  One static binary. Works before you have an account. The cloud is the upgrade.
</p>

<p align="center">
  <a href="#the-first-minute">First minute</a> ·
  <a href="#with-a-team">With a team</a> ·
  <a href="#coding-agents">Coding agents</a> ·
  <a href="#commands">Commands</a> ·
  <a href="./docs/Design.md">Design</a>
</p>

---

penv reads the `.env` you already have, writes a small committed schema next to it, validates every value before your process starts, generates types for your language, and configures your coding agent's harness so it cannot read the file. When you are ready for a team, one command moves the values to [penv.cloud](https://penv.cloud) and deletes the file.

## The first minute

No account, no prompts.

```bash
curl -fsSL https://penv.cloud/install | sh    # one binary, no Node

penv init                                     # reads .env, writes .env.schema, gitignores .env
penv run -- pnpm dev                          # validates, injects, masks
```

`init` writes this, and only this, into your repository:

```dotenv
# @schema=1

# @type=url
DATABASE_URL=

# @type=string
STRIPE_SECRET_KEY=

# @type=port @sensitive=false
PORT=3000

# @type=boolean @sensitive=false
DEBUG=true
```

Every key is sensitive and required unless a bundler prefix like `NEXT_PUBLIC_` or a dull value like `3000` says otherwise. No value that could be a secret is ever copied into the schema. Edit the file if a guess is wrong; the decorators follow the [@env-spec](https://varlock.dev) vocabulary, so a varlock user reads it on sight.

## With a team

```bash
penv login          # device code in the browser
penv push           # values go to the cloud, .env is deleted
```

From then on the cloud is the store and `.env` is a view you can regenerate with `penv pull`. A teammate clones the repo and types `penv run -- pnpm dev`; that is the whole onboarding. CI presents its OIDC token and gets a fifteen minute credential. A server with nothing to present enrols a keypair once.

## Typed access

```bash
penv gen ts        # src/env.ts, a typed cast over process.env, no runtime
penv gen py        # penv_env.py, pydantic with SecretStr for sensitive keys
```

A language target is a folder holding a `target.toml` and a template. Drop one into `.penv/targets/go/` and `penv gen go` works. The binary knows no language by name.

## Coding agents

An agent runs as you, so it can read what you can read. penv narrows that:

- Nothing at rest once pushed. There is no `.env` to `cat`.
- `penv run` injects into the child process only, and scrubs every sensitive value, in raw, hex, base64 and URL-encoded forms, from the child's output whenever an agent session is detected.
- `penv guard` writes what each harness actually enforces, from the schema: deny rules and a sandbox block for Claude Code, a permission profile for Codex, deny rules and fail-closed hooks for Cursor, and the equivalents for Copilot, Gemini, Cline, Windsurf and Amp. The hook is the penv binary itself, never a script that fails open.
- `reveal` needs a person to approve in the console. An agent can ask; a human clicks.

The claim penv makes, printed by `penv guard --check`, is only what is true: it keeps secrets out of the files, the repo, the shell history and the captured output an agent reads. It cannot stop a process running as you from looking, so every value the cloud issues is short-lived, scoped and attributable to the session that used it.

## Commands

| Command | Does |
|---|---|
| `penv` | State and the one next command |
| `init` | `.env` to `.env.schema`, never prompts |
| `run -- cmd` | Validate, inject, mask |
| `check [KEY]` | Schema, values, drift, guard coverage |
| `ls` | Names and types, values masked |
| `gen <target>` | Typed file for a language |
| `guard` | Harness configs from the schema |
| `push` / `pull` | Values to and from the cloud |
| `set` / `unset` | Write a value, never echoed |
| `reveal KEY` | One value, after console approval |
| `login` / `logout` | Device code, credential in the OS keychain |
| `machine enroll` | Bind a server keypair |
| `help --json` | The command manifest |

JSON when stdout is not a terminal. Exit codes: 0 ok, 1 error, 2 auth, 3 validation, 4 confirmation required, 5 no credential, 6 environment refused.

## Status

Phase 1, local mode, is complete on the `v1-rust` branch. Phase 2, the cloud commands, is in progress. Phase 3 is distribution. The previous TypeScript CLI on `main` is retired and does not migrate.

## Contributing

Read [AGENTS.md](./AGENTS.md) and [docs/Design.md](./docs/Design.md). Locally run only `cargo check`, `cargo test`, `cargo clippy` and `cargo fmt`; release builds happen in CI.

MIT.
