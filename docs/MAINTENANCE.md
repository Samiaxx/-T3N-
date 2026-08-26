# Maintenance & handover

This agent is designed to be cheap to run and easy to hand off. It is
**stateless** — there is no database, no server to keep up, no cron. The only
durable state is:

1. the registered contract (lives on the Terminal 3 network),
2. one sealed KV entry (`resend_api_key` in `z:<tid>:secrets`), and
3. `app/deployment.json` (a committable record with **no secrets**).

Everything else is rebuilt from source on demand.

---

## Prerequisites

| Tool | Version used | Notes |
|---|---|---|
| Node.js | 20+ (tested on 24.18.0) | runs the TypeScript orchestrator via `tsx` |
| Rust | stable (1.80+) | `rustup target add wasm32-wasip2` |
| `@terminal3/t3n-sdk` | ^5.1.0 | pinned in `app/package.json` |

Install once: `cd app && npm install`, then `rustup target add wasm32-wasip2`.

---

## Routine operations

### Ship a new contract version

```bash
# edit contract/src/*.rs
cd app
npm run test:contract     # native unit tests (pure logic)
npm run build:contract    # → contract/target/wasm32-wasip2/release/z_tenant_notify.wasm
npm run deploy            # re-registers with the next patch version, reseals the key
npm run grant             # re-authorize (version bumped → grant references the new version)
npm run render            # smoke test
```

`deploy` auto-bumps the patch version until the network accepts it (a tail
requires a strictly-higher version to re-register), so you never hand-edit
version numbers. **Re-registering yields a new `contract_id`** — see the KV note
below.

### Rotate the provider (Resend) API key

```bash
# put the new key in app/.env  →  RESEND_API_KEY=re_...
cd app && npm run deploy    # reseals z:<tid>:secrets via the control plane
```

No contract rebuild needed — the key is data, not code. (If you only want to
reseed the secret without touching the contract, that single `map-entry-set`
control call is all `deploy` does for the key; re-registering an unchanged WASM
is otherwise a no-op you can skip.)

### Rotate the tenant key (`T3N_API_KEY`)

The tenant key is a private key. If it is ever exposed (e.g. pasted into chat —
see the note in [SUBMISSION.md](SUBMISSION.md)), rotate it: provision a new
tenant credential, update `app/.env`, and redeploy. The contract and its logic
are unaffected; only the owning identity changes.

### Change the email template

The sample template lives in `app/src/invoke.ts`. In production the template and
its `variables` come from the caller's `execute` input — the contract itself is
template-agnostic. To change what a preview shows, edit `SAMPLE`.

### Add a profile (PII) field the template may use

Add it to `ALLOWED_PROFILE_FIELDS` in `contract/src/render.rs`, add a unit test,
then `npm run test:contract && npm run build:contract && npm run deploy`. The
allowlist is intentional: it prevents a template from referencing PII the
contract wasn't reviewed to expose.

---

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `Missing required env var T3N_API_KEY` | no `app/.env` | `cp app/.env.example app/.env`, fill it in |
| `Authenticated DID (…) does not match T3N_TENANT_DID` | key and DID are from different tenants | correct `app/.env` |
| `WASM not found …` | contract not built | `npm run build:contract` |
| `host/http.egress_denied` on send | no/expired grant, or wrong host | `npm run grant` |
| `Resend send failed: HTTP 401` | placeholder or invalid `RESEND_API_KEY` | set a real key, redeploy |
| `placeholder-unknown` / `-no-user-context` | caller's profile has no verified email | use an identity with a verified email contact (see ARCHITECTURE → *What a production send needs*) |
| KV read denied after redeploy | `contract_id` changed, map ACL is stale | re-create the `secrets` map (or use a fresh tail) so its ACL includes the new id |

Every RPC error includes a bracketed **request id** (e.g.
`[70e0207b-…]`) — quote it to Terminal 3 support.

---

## Ownership: recommendation

You asked to keep both options open. Here is the recommendation and both paths.

> **Recommendation: keep running it yourself for now, with a documented
> handover to Terminal 3 once the template and provider relationship are
> frozen.**

**Why keep it (near term).** The agent is stateless and nearly free to operate —
there is no infrastructure, just a contract on the network and one sealed key.
You retain control of the provider (Resend) billing relationship and the tenant
identity while you're still iterating on templates and deciding volume. The
operational burden is a `npm run deploy` when something changes.

**Why hand it to T3N (later).** Once the functionality is stable and you'd
rather not hold the provider credential or field support, handing over removes
the last two things you carry: the tenant key and the Resend key. Because the
whole system is codified and reproducible, the handover is clean.

Neither path requires rewriting anything — the difference is *who holds the two
keys and who signs the grant*.

### Handover process (either direction)

Everything the recipient needs is in this repo. To transfer operation to
Terminal 3 (or anyone):

1. **Hand over the repo** (public already) — source, docs, and `deployment.json`.
2. **New owner provisions their own tenant** identity and puts it in their own
   `app/.env` (the private key never travels).
3. **New owner supplies their own provider key** — a fresh `RESEND_API_KEY`;
   the old one is rotated/revoked at Resend so it stops working immediately.
4. **New owner runs `npm run build:contract && npm run deploy && npm run grant`.**
   The WASM builds from source (the release profile is deterministic:
   `opt-level=s, lto=true, codegen-units=1, strip=true`), so they can review
   exactly what they're running rather than trusting a binary.
5. **Repoint callers** at the new owner's `scriptName` (in the new
   `deployment.json`).

No state migration is required because there is no mutable state to migrate —
only the sealed provider key, which is intentionally re-created by the new owner
rather than copied.

---

## Cost & scaling notes

- `render-notification` consumes no egress and no provider credit — use it
  freely for previews, tests, and CI.
- `send-notification` costs one provider send per call plus the network's
  execution fee. It is stateless and horizontally trivial: the same contract
  serves any number of callers; scaling is a function of your Resend plan, not
  of anything you operate.
- There is nothing to keep running between calls. If no one invokes the
  contract, it costs nothing.
