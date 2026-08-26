# Architecture

How `tenant-notify` keeps two secrets out of reach while still sending a real
email, and how the demo's self-call model maps to a production deployment.

---

## The problem

An enterprise wants to email its customers — order updates, receipts, alerts.
The naïve way couples two liabilities into the enterprise's own systems:

1. **The customer contact list.** Every address the enterprise can send to is
   an address it can leak.
2. **The email provider's API key.** Whoever holds it can send as the
   enterprise, to anyone.

`tenant-notify` removes both from the enterprise's blast radius by running the
send inside a Terminal 3 TEE and letting the **host** — not the agent code —
supply each secret at the last moment.

---

## Three identities

Terminal 3 separates three roles. Understanding them is the key to the whole
design.

| Role | Who | Holds | In this project |
|---|---|---|---|
| **Tenant** | the developer/enterprise | `T3N_API_KEY` (a private key) → a tenant DID | you; deploys the contract, seals the provider key |
| **Agent** | the caller that invokes the contract | its own DID | the identity that runs `render` / `send` |
| **Data owner / user** | owns the profile data (e.g. the customer) | a verified profile, and the power to grant access | supplies the recipient email via a verified contact |

An agent may authenticate, but it cannot reach a user's data or an external
host until the **data owner grants** it that scope. Authentication ≠
authorization.

### Self-call (what the demo uses)

To keep the demo runnable with a single credential, this project uses the
**self-call** model: one identity (`T3N_API_KEY` → tenant DID) plays all three
roles. The tenant deploys the contract, the "data owner" (the same DID) grants
access to itself, and the "agent" (again the same DID) invokes it.

```
                     ┌──────────────────────────────┐
   T3N_API_KEY ─────▶│  one DID plays all three roles │
                     │  tenant · data owner · agent   │
                     └──────────────────────────────┘
```

This is why `app/src/grant.ts` sets `agentDid` to our own DID. Nothing about
the **contract** changes between self-call and production — only who signs the
grant and whose profile is read. See [Going to production](#going-to-production).

---

## Confidential contract = WASM with a declared capability surface

The contract is Rust compiled to a WebAssembly **component**
(`wasm32-wasip2`, `crate-type = ["cdylib", "lib"]`). It runs inside the node's
TEE. There is no imperative "manifest" of permissions — **the contract's
capabilities are exactly the host interfaces it imports in its WIT world**
([`contract/wit/world.wit`](../contract/wit/world.wit)):

| Import | Why this contract needs it |
|---|---|
| `host:tenant/tenant-context` | read the tenant DID to build the secrets map name |
| `host:interfaces/logging` | structured, **non-PII** logs |
| `host:interfaces/kv-store` | read the sealed provider key |
| `host:interfaces/http-with-placeholders` | the **only** egress path — and the one that can resolve `{{profile.*}}` |

Deliberately **not** imported: the plain `host:interfaces/http` interface. This
contract can only make outbound calls through the placeholder-aware variant, so
there is no code path that could egress raw profile data. Minimizing the import
set minimizes what the contract can ever do — the capability surface *is* the
security boundary.

Each exported function takes a `generic-input` record (input / user-profile /
context, each `option<list<u8>>`) and returns `result<list<u8>, string>`. JSON
in, JSON out.

---

## Secret #1 — the recipient address (never enters WASM)

The outbound request body contains a **placeholder**, not an address:

```json
{ "to": ["{{profile.verified_contacts.email.value}}"], "subject": "…", "text": "…" }
```

`http-with-placeholders.call(...)` resolves `{{profile.*}}` markers **host-side,
inside the enclave, against the calling user's verified profile**, immediately
before the bytes leave the node. The WASM builds the request with the marker and
never sees the resolved value. Key properties:

- Only the `profile` namespace is resolvable. A `{{secrets.*}}` marker is
  rejected by the host with `placeholder-denied` — you cannot smuggle a sealed
  secret into a URL or body this way.
- The address is read from the data owner's **verified** contacts, so the
  enterprise cannot send to an arbitrary address — only to a contact the user
  actually confirmed. That is the "consent-gated" property.
- `render-notification` validates every `{{profile.*}}` field against an
  allowlist (`ALLOWED_PROFILE_FIELDS` in
  [`contract/src/render.rs`](../contract/src/render.rs)) and reports which
  fields a send will require, so a preview can't silently reference PII the
  template author didn't intend.

## Secret #2 — the provider API key (read but never returned)

The Resend key is sealed into a private KV map named `z:<tid>:secrets`
(`<tid>` = the tenant DID hex). The contract reads it at send time:

```rust
let tid = tenant_context::tenant_did();
let map_name = format!("z:{}:secrets", hex::encode(&tid));
kv_store::get(&map_name, b"resend_api_key")
```

and uses it only to build the `Authorization: Bearer …` header for the egress
call. It is never logged, never returned from the function, and never placed in
a `{{…}}` marker. The value reaches the map through the **control plane**
(`tenant.executeControl("map-entry-set", …)`), which bypasses the writers ACL —
so seeding a secret doesn't require granting anyone write access to the map.

### KV governance defaults to deny

A freshly created KV map denies all reads and writes until its ACL names
principals. `app/src/deploy.ts` therefore creates the map with the contract's
`contract_id` in **both** `readers` and `writers`:

```ts
tenant.maps.create({ tail: "secrets", visibility: "private",
                     writers: { only: [contractId] }, readers: { only: [contractId] } });
```

If the contract is ever re-registered and its `contract_id` changes, the map's
ACL must be updated (or a fresh map tail used) or the read will be denied — the
deploy script logs a reminder when it finds the map already exists.

---

## Egress authorization — the grant

Even with the code and secrets in place, the contract cannot call out until the
data owner authorizes it. `app/src/grant.ts` submits an `agent-auth-update` to
the built-in `tee:user/contracts` contract, scoping three things at once:

```ts
{ agentDid,                                   // who may call
  scripts: [{ scriptName,                     // which contract
              versionReq,
              functions: ["render-notification", "send-notification"],  // which functions
              allowedHosts: ["api.resend.com"] }] }                     // which egress hosts
```

Without a matching grant, `send-notification`'s outbound call fails with
`host/http.egress_denied` **before any bytes leave the node**. The allowed host
list is the runtime enforcement of the same `EGRESS_HOST` constant the contract
declares in code — defense from both sides.

---

## End-to-end data flow

```
render-notification(input)
  └─ substitute {{var.*}} from input.variables         (in enclave)
     validate {{profile.*}} against the allowlist
     → { subject, body(with {{profile.*}} intact), profile_fields, egress_host }

send-notification(input)
  └─ render(input)                                       (fail-fast: bad template errors here)
     require a verified `from`
     kv_store.get("z:<tid>:secrets", "resend_api_key")   (Secret #2, in enclave)
     build { to: ["{{profile.…email}}"], subject, text } (Secret #1 as a marker)
     http-with-placeholders.call(POST api.resend.com)    ── host resolves {{profile.*}} ──▶ provider
     → { provider_message_id, status }
```

The agent's WASM handles the message **content**. It never handles the recipient
address or the provider key in cleartext — the host injects both at the enclave
boundary.

---

## What a production send needs

The demo's `send` returns HTTP 401 with the placeholder key. A delivering send
needs exactly two changes, **both configuration, not code**:

1. **A real provider key.** Put a real `RESEND_API_KEY` in `app/.env` and
   re-run `npm run deploy` to reseal the map.
2. **A caller whose verified profile has an email.** In self-call, that means
   the tenant DID's own profile must carry a verified
   `verified_contacts.email.value`. If it doesn't, the host cannot resolve the
   recipient placeholder and returns `placeholder-unknown` /
   `placeholder-no-user-context` instead of reaching the provider.

Neither touches the contract or the orchestrator logic.

---

## Going to production

Moving from the self-call demo to a real multi-party deployment changes **who
signs what**, not the contract:

| | Demo (self-call) | Production |
|---|---|---|
| Caller | tenant DID | a distinct **agent DID** |
| Grant signed by | tenant DID (itself) | the **real data owner** (the customer) |
| Recipient profile | tenant's own | the customer's verified contact |
| Provider key | tenant seals it | tenant seals it (unchanged) |

Concretely: register a separate agent DID, have each customer sign an
`agent-auth-update` authorizing that agent for `send-notification` +
`api.resend.com`, and invoke as the agent. `render-notification` needs no user
context at all, so previews and CI work with any identity.

See [MAINTENANCE.md](MAINTENANCE.md) for the operational runbook and the
ownership recommendation (keep running it yourself vs. hand it to Terminal 3).
