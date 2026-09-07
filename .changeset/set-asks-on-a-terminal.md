---
"@penvhq/cli": patch
---

**`penv set <key>` with no value asks for it on a terminal, and hides the typing when meta says the parameter is a secret.** It used to drain stdin whatever was on the other end, so on a terminal it sat waiting for an end-of-file nobody was told to send. A pipe still works exactly as before; a blank answer refuses rather than writing an empty file, since an empty value would win the cascade over the one already there. `penv fill` hides secret answers the same way, which its prompt shape had carried a flag for and never used.
