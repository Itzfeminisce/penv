# Cloud API for the CLI

The surface `penv` speaks to penv.cloud. It is new in v1 and lives beside the existing machine-only `/api/v1/secrets` routes, which the retired TypeScript provider used and which nothing else needs. Every route is under `/api/v1`, answers JSON, and is authenticated with `Authorization: Bearer <credential>` unless stated. Error bodies are `{ "error": "<code>" }` with the status codes below.

## Principals and credentials

| Prefix | Who | How obtained | Lifetime |
|---|---|---|---|
| `pcu_` | a person | device-code login | 30 days from last use (every authenticated request extends `expiresAt`), revoked by `logout` |
| `pck_` | a machine identity | console-issued token, or exchanged from OIDC, AWS SigV4 or a bound keypair | as today; exchanges mint 15 minutes for the CLI |

Both resolve through one verifier to the same claims shape and one RBAC evaluator. A user credential carries the user's own role assignments and no fixed scope. A machine credential is bound to one project and environment; a request whose address is another environment is `403 forbidden`.

Claims: `{ principal: "user" | "machine", principalId, credentialId, orgId, projectId?, environmentId?, grants, plan }`.

## Device-code login

```
POST /api/v1/auth/device            unauthenticated, IP limited
  -> 201 { deviceCode, userCode, verificationUri, expiresIn: 600, interval: 5 }

POST /api/v1/auth/device/token      body { deviceCode }
  -> 428 { error: "authorization_pending" }
  -> 429 { error: "slow_down" }
  -> 410 { error: "expired" }
  -> 403 { error: "denied" }
  -> 201 { credential: "pcu_...", expiresAt, user: { email }, orgs: [{ slug, name }] }

console page /device                signed-in person enters userCode, sees the device name and IP, approves or denies; step-up MFA applies
POST /api/v1/auth/revoke            Bearer pcu_ or pck_, revokes itself; idempotent
```

`POST /auth/device` accepts `{ "device": "<host name>" }`, shown on the approval page. The user code is eight characters in two groups, `XXXX-XXXX`, case-insensitive, normalised server-side. Approval marks the row; the credential is minted by the first successful poll after approval, once. While polling, any 429 (the IP ceiling's `rate_limited` as well as `slow_down`) means back off, honouring `retry-after`. `user.email` may be null.

## Machine exchanges

Unauthenticated, IP limited. Each returns `201 { credential: "pck_...", expiresAt }` with a 15 minute lifetime capped by the trust's own expiry, or `401 { error: "expired" | "unauthorized" }`, or `503 { error: "unavailable" }` which means retry once.

```
POST /api/v1/auth/oidc               { token }                       audience is the org id
POST /api/v1/auth/aws                { method, url, body, headers }  a SigV4-signed STS GetCallerIdentity request
POST /api/v1/auth/keypair/enroll     { secret: "pce_...", publicKey }  SPKI DER base64, Ed25519 -> 201 { credentialId, generation: 1 }
POST /api/v1/auth/keypair/challenge  { credentialId } -> 200 { nonce }   valid 120 s
POST /api/v1/auth/keypair            { credentialId, nonce, generation, signature } -> 201 { credential, expiresAt, generation }
```

The keypair signs the UTF-8 bytes of `penv-cloud:keypair:v1\n{credentialId}\n{nonce}\n{generation}` (four lines joined by newline); the signature is base64. The client persists the returned `generation` before using the credential. A generation mismatch that is not a replay answers `409 { error: "cloned" }` and revokes the keypair and everything it minted.

The CLI stores the credential in the OS keychain under the API base URL. `PENV_TOKEN` in the environment takes precedence over the keychain and is how CI and servers pass a `pck_`.

## Environments

An address is `{org}/{project}/{environment}`; org and project are slugs, environment is the free-form name the console knows.

```
HEAD /api/v1/envs/{org}/{project}/{environment}     If-None-Match honoured
  -> 304
  -> 200 with ETag: "<hash>"    hash over every (parameter id, latest version, meta) in the environment; changes when any value or decorator changes
  -> 404 not_found | 403 forbidden

GET  /api/v1/envs/{org}/{project}/{environment}     If-None-Match honoured
  -> 304
  -> 200 ETag + {
       "keys": [ { "path": "", "name": "DATABASE_URL", "kind": "static", "version": 3,
                   "schema": { <one key of the .env.schema JSON IR> },
                   "value": "..." } ],
       "skipped": [ "path/name" ]      dynamic keys and keys with no version; absent when empty
     }
  requires secret:reveal; `?values=false` lists with schema only and requires secret:read

PUT  /api/v1/envs/{org}/{project}/{environment}     push
  body { "keys": [ { "path", "name", "schema", "value"? } ], "prune": false }
  -> 200 { "written": n, "unchanged": n, "pruned": n, "etag": "..." }
  a key with no value updates the schema only; prune=true deletes keys not in the body; requires secret:write (and secret:delete when pruning)

PATCH  /api/v1/envs/{org}/{project}/{environment}/keys/{path...}/{name}   set
  body { "value"?: string, "schema"?: {...} }
  -> 200 { "version": n, "etag": "..." }

DELETE /api/v1/envs/{org}/{project}/{environment}/keys/{path...}/{name}   unset
  -> 200 { "etag": "..." }
```

Every address and key segment is percent-encoded by the client; environment names are free-form. The per-key schema is stored in `parameters.meta` as the same JSON object the CLI emits for that key in `penv schema --json`, minus `name`: `type {name, raw, members, constraints}`, `required`, `sensitive`, `default`, `description`, `example`, `docs`, `since`, `deprecated` (a string note), `rotate`, `dynamic` (boolean), `dynamicFrom`. The client omits absent fields; the server treats `null` as absent. Anything else is `400 schema_invalid`. Writes to a dynamic key answer `409 dynamic`. The console renders and edits it. `must_encrypt` follows `sensitive`.

## Projects

```
GET  /api/v1/orgs                                   -> { orgs: [{ slug, name }] }
GET  /api/v1/orgs/{org}/projects                    -> { projects: [{ slug, name, environments: [name] }] }
POST /api/v1/orgs/{org}/projects                    body { name, environments: ["development"] } -> 201 ; requires project:create
```

Slugs are derived from names server-side; an ambiguous address is refused, never guessed. `penv push` on a schema with no `@penv=` header creates the project from the directory name after printing what it will do, and writes the `slug` the 201 body returns into the header. Project creation over the plan limit answers `409 quota_exceeded`.

## Errors

| Status | Codes |
|---|---|
| 400 | `schema_invalid`, `name_required`, `keys_required`, `value_must_be_a_string`, `token_required` |
| 401 | `expired` (say so: run `penv login` again), `unauthorized` |
| 403 | `forbidden`, `denied` |
| 404 | `not_found` |
| 409 | `dynamic`, `cloned`, `quota_exceeded`, `ambiguous` |
| 429 | `rate_limited`, `slow_down`, both with `retry-after` seconds |
| 503 | `unavailable`, retry once |
| other 5xx | one retry after one second, then exit 1 naming the status |

## Rate limits and audit

As today: IP ceiling, then per-principal per-op plan limits. A user credential shares the identity bucket keyed by `principalId`. Every request may carry `X-Penv-Agent: <name>` and `X-Penv-Session: <id>` from the CLI's agent detection; every route that writes an audit row, including orgs, projects and the exchanges, stamps both into the row's metadata, truncated to 128 characters each and treated as data, so the console can answer "what did the agent session touch".

## Out of scope for phase 2

Console approval for `reveal` under an agent session (the exit-4 contract) is phase 2b. In phase 2, `reveal` under an agent session is refused outright by the CLI, and for a person it is a plain read gated by `secret:reveal`.
