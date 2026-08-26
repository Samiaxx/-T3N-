# tenant-notify — consent-gated notifications on Terminal 3

Send a transactional email to a customer **without your servers ever holding
the customer's email address or the email provider's API key.** Both secrets
stay inside a Terminal 3 TEE; a breach of the enterprise app leaks neither.

- **Provider key** is sealed into a confidential KV map that only the contract
  can read. The WASM uses it to authenticate to the provider but never returns
  it — it is never in the app's environment, logs, or this repo.
- **Recipient address** is resolved from the data-owner's *verified* profile
  **inside the enclave**, through a `{{profile.…}}` placeholder the host fills
  in at egress. It never enters the agent's code or the enterprise app.

That is the enterprise value: **consent-gated, minimal-trust notifications.**
The customer's contact detail is used, not exposed; the credential is used, not
distributed.

---

## How it works (30-second version)

```
 enterprise app                Terminal 3 node (TEE)                 provider
 ──────────────                ─────────────────────                 ────────
 business data  ──execute──▶   ┌───────────────────────────┐
 {{var.order_no}}              │ render-notification (WASM) │
 {{profile.first_name}}        │  • substitute {{var.*}}    │
                               │  • validate template       │
                               │  • keep {{profile.*}} as-is │
                               └───────────────────────────┘
                               ┌───────────────────────────┐
                               │ send-notification (WASM)   │
                               │  • read sealed provider key │──┐
                               │    from z:<tid>:secrets     │  │ Authorization
                               │  • build request w/          │  │ Bearer <key>
                               │    {{profile.…email}}        │  ▼
                               └──────────────┬──────────────┘  host resolves
                                              │                 {{profile.*}}
                                              └───http-with-placeholders──▶  api.resend.com
```

The agent's WASM sees the email **body** and business variables. It never sees
the recipient address or the provider key in cleartext — the host injects both
at the enclave boundary.

Full design: **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**.

---

## Two functions

| Function | What it does | Egress? | PII? | Provider credit? |
|---|---|---|---|---|
| `render-notification` | Dry-run: substitute business vars, validate the template, report which profile fields the send will need. | No | No | No |
| `send-notification` | The real send: render → read sealed key → egress through `http-with-placeholders` (host resolves recipient PII). | Yes | Host-side only | Yes |

`render-notification` is a deterministic preview — ideal for tests, CI, and
UI previews. `send-notification` is the production path.

---

## Quickstart

**Prerequisites:** Node ≥ 20, Rust with the `wasm32-wasip2` target
(`rustup target add wasm32-wasip2`), and a Terminal 3 tenant API key.
See [docs/MAINTENANCE.md](docs/MAINTENANCE.md#prerequisites) for exact versions.

```bash
# 1. install the orchestrator's deps
cd app && npm install

# 2. configure your identity (never committed)
cp .env.example .env      # then edit T3N_API_KEY, T3N_TENANT_DID
                          # (optionally add a real RESEND_API_KEY)

# 3. build the confidential contract → WASM
npm run build:contract

# 4. deploy, authorize, and run the demo
npm run deploy            # register WASM, create the secrets map, seal the key
npm run grant             # authorize the caller for the contract + provider host
npm run render            # headline demo: pure, deterministic, no egress
npm run send              # the real send (needs a real RESEND_API_KEY)
```

`npm run all` chains deploy → grant → render → send.

A real end-to-end run against testnet is captured verbatim in
**[docs/RUN_LOG.md](docs/RUN_LOG.md)**.

---

## What the demo proves

`npm run render` returns, deterministically:

```json
{
  "subject": "Your order AB-10024 is on its way 📦",
  "body": "Hi {{profile.first_name}},\n\nGood news — order AB-10024 shipped via DHL Express.\n…",
  "profile_fields": ["first_name"],
  "egress_host": "api.resend.com"
}
```

Business variables are substituted; the `{{profile.*}}` marker is left intact
for the host to resolve at send; and the contract self-reports which PII fields
it will need and the one host it will ever reach.

`npm run send` (with the placeholder key) returns `Resend send failed: HTTP 401`.
That 401 is the proof the whole confidential path works: the contract read the
sealed key from KV, the host **allowed** egress to `api.resend.com` (the grant
worked — otherwise the call is denied before it leaves the enclave), and Resend
received and rejected the *placeholder* credential. Swap in a real key and the
same path delivers. See [docs/ARCHITECTURE.md → What a production send needs](docs/ARCHITECTURE.md#what-a-production-send-needs).

---

## Repo layout

```
contract/            Rust → WASM confidential contract (the TEE logic)
  wit/world.wit        capability surface (imports) + exported functions
  src/render.rs        pure template engine — no host deps, fully unit-tested
  src/send.rs          egress via http-with-placeholders + sealed-key read
  src/lib.rs           WIT bindings + Guest impl
app/                 TypeScript orchestrator (deploy / grant / invoke)
  src/env.ts           config + zero-dependency .env loader
  src/session.ts       authenticate once, cache the session
  src/deploy.ts        register WASM, create + seal the secrets map
  src/grant.ts         agent-auth-update authorization
  src/invoke.ts        the render / send demo calls
  src/cli.ts           `tsx src/cli.ts <command>`
docs/                ARCHITECTURE · MAINTENANCE · BUGS · SUBMISSION · RUN_LOG · T3N-ADK-NOTES
.github/workflows/   CI: cargo test + tsc typecheck
```

---

## Security

- **Never commit `app/.env`** — it holds the tenant private key. It is
  gitignored, and CI enforces that (see [docs/BUGS.md](docs/BUGS.md) for the
  verification step). If a key is ever exposed, rotate it.
- The provider API key lives only in the sealed KV map. `deployment.json`,
  logs, and this repo contain no secrets.
- The contract declares exactly one egress host (`api.resend.com`) in both the
  code (`contract/src/render.rs`) and the grant (`app/src/grant.ts`).

## License

MIT — see [LICENSE](LICENSE).
