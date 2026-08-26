# Terminal 3 ADK Challenge — Submission

**Agent:** `tenant-notify` — consent-gated transactional notifications
**Author:** *(your name)*
**Date:** 2026-08-26
**Network:** Terminal 3 testnet (`cn-api.sg.testnet.t3n.terminal3.io`)

**Links**
- GitHub repo: *(paste your public repo URL here)*
- This document: *(paste the public Google Doc URL here)*

---

## One-line pitch

An enterprise agent that emails a customer **without the enterprise ever holding
the customer's email address or the email provider's API key** — both secrets
stay inside a Terminal 3 TEE and are injected by the host at the last moment.

## The enterprise problem it solves

Transactional email couples two liabilities into a company's own systems: the
**customer contact list** (everything you can send to, you can leak) and the
**provider API key** (whoever holds it can send as you, to anyone). `tenant-notify`
removes both from the enterprise's blast radius:

- **Recipient address** — the outbound request carries a
  `{{profile.verified_contacts.email.value}}` *placeholder*. The host resolves
  it inside the enclave, against the data owner's **verified** contact, right
  before the request leaves the node. The agent's code never sees the address,
  and the enterprise can only reach a contact the customer actually confirmed.
- **Provider key** — sealed into a confidential KV map only the contract can
  read. The contract uses it to authenticate but never returns or logs it. It's
  never in the app's environment, in `deployment.json`, or in the repo.

A breach of the enterprise app therefore leaks neither the customer list nor the
sending credential.

## How it works (summary)

Two WebAssembly functions run in the TEE:

- `render-notification` — pure, deterministic dry-run: fills in business
  variables, validates the template, and reports which profile (PII) fields a
  send will need and the single host it may reach. No egress, no PII, no
  provider credit.
- `send-notification` — the real send: render → read the sealed key → egress via
  `http-with-placeholders`, which resolves the recipient PII host-side.

The contract's capabilities are exactly the host interfaces it imports in its
WIT world — and it deliberately imports only the placeholder-aware HTTP
interface, so there is no code path that could egress raw profile data. Full
design in `docs/ARCHITECTURE.md`.

## Proof it works (live testnet run)

Captured verbatim in `docs/RUN_LOG.md`. Highlights:

- **Deploy:** 177 KB WASM registered as `contract_id=746`; the `z:<tid>:secrets`
  KV map created with the contract in its ACL; provider key sealed via the
  control plane.
- **`render-notification`** returns deterministically:
  ```json
  { "subject": "Your order AB-10024 is on its way 📦",
    "body": "Hi {{profile.first_name}}, … — The Acme team",
    "profile_fields": ["first_name"],
    "egress_host": "api.resend.com" }
  ```
  Business variables substituted; the PII marker preserved for host resolution;
  the contract self-reports the PII it needs and the one host it will reach.
- **`send-notification`** (with a placeholder key) returns
  `Resend send failed: HTTP 401`. That 401 is the success signal for the
  confidential path: the contract read the sealed key, the host **allowed**
  egress to `api.resend.com` (the grant worked — otherwise the call is denied
  before leaving the node), and Resend received and rejected the *placeholder*
  credential. A real key makes the identical path deliver.

## Build-quality highlights (usefulness + maintainability)

- **Stateless.** No server, no database, no cron. Durable state is just the
  on-network contract, one sealed KV entry, and a committable `deployment.json`.
  If nobody calls it, it costs nothing.
- **Minimal capability surface.** The WIT world imports only what it needs, and
  only the placeholder-aware HTTP interface — the smallest attack surface that
  still does the job.
- **The core is pure and fully tested.** All template/allowlist logic lives in
  `contract/src/render.rs` with **no host dependencies**, covered by 18 unit
  tests + 1 doc-test that run natively (`npm run test:contract`). This repo
  omits the reference template's `.cargo/config.toml` specifically so native
  testing works (see bug T3-2).
- **One-command operations.** `deploy` / `grant` / `render` / `send` / `all`,
  each idempotent; `deploy` auto-bumps the version and reseals the key so you
  never hand-edit version numbers or re-run control calls by memory.
- **Secrets never touch the repo.** `app/.env` is gitignored; the root
  `.gitignore` is verified with `git check-ignore`, and CI fails if a `.env`
  is ever staged.
- **Config, not code, to go live.** A delivering send needs only a real provider
  key and a caller with a verified email — no contract or orchestrator changes
  (`docs/ARCHITECTURE.md → What a production send needs`).

## Documentation

| Doc | Purpose |
|---|---|
| `README.md` | overview, quickstart, repo map |
| `docs/ARCHITECTURE.md` | three identities, self-call, placeholders, KV sealing, egress grant, path to production |
| `docs/MAINTENANCE.md` | prerequisites, routine ops, troubleshooting, **ownership recommendation + handover** |
| `docs/BUGS.md` | issues found, with repro and suggested fixes |
| `docs/RUN_LOG.md` | verbatim live testnet run (the evidence) |

## Ownership decision

**Recommendation: keep running it yourself for now, with a documented handover
to Terminal 3 once the template and provider relationship are frozen.** The
agent is stateless and nearly free to operate, and you retain the provider
billing relationship while iterating. Handing it to T3N later cleanly removes
the last two things you carry — the tenant key and the provider key — and
because the whole system is codified and the release build is deterministic, the
recipient can rebuild and verify the WASM from source rather than trust a binary.
The full handover runbook (either direction, no state migration needed) is in
`docs/MAINTENANCE.md`.

## Bugs submitted

Four Terminal 3 issues with repro + suggested fixes (docs/WIT signature drift;
reference template breaking native `cargo test`; no API to look up a contract's
current `contract_id`; KV default-deny discoverability), plus two environment
notes. Detail in `docs/BUGS.md`.

## Run it yourself

```bash
cd app && npm install
cp .env.example .env      # set T3N_API_KEY, T3N_TENANT_DID (optionally RESEND_API_KEY)
npm run build:contract
npm run all               # deploy → grant → render → send
```

---

### Security note

The tenant API key used for this build was shared in plaintext during
development. It lives only in the gitignored `app/.env` and appears nowhere in
the repo or these docs. **Recommendation: rotate that key after the challenge.**

### Bonus — X post (draft)

> Built a consent-gated notifications agent on @terminal3io ADK: it emails a
> customer without my app ever touching the customer's address *or* the email
> provider's API key — both stay sealed in the TEE and the host injects them at
> egress. Stateless, fully tested, one-command deploy. 🧵 [repo link]
