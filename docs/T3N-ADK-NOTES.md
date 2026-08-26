# Terminal 3 ADK — developer notes

Concise, **verified** notes for building on Terminal 3, collected while building
this repo. Signatures here were checked against the shipped WIT
(`host:interfaces@2.1.0`, `host:tenant@1.0.0`) and SDK v5.1.0, not just the
rendered docs (which drift — see [BUGS.md](BUGS.md) T3-1).

## Mental model: three identities

- **Tenant** — the developer; holds `T3N_API_KEY` (a secp256k1 private key) →
  a tenant DID. Deploys contracts, seals secrets.
- **Agent** — the caller that invokes a contract; has its own DID.
- **Data owner / user** — owns profile data; must *grant* an agent access.

Authentication ≠ authorization. An agent can authenticate yet still be denied
data/egress until the data owner signs an `agent-auth-update`.

**Self-call** (used in this repo): one DID plays all three roles so the demo
runs with a single credential. Only *who signs the grant* differs in production.

## Contract shape (Rust → WASM component)

- Target `wasm32-wasip2`; `crate-type = ["cdylib", "lib"]`.
- Capabilities = the host interfaces you **import** in `wit/world.wit`. Import
  the minimum. Importing only `http-with-placeholders` (not plain `http`) means
  no code path can egress raw profile data.
- Each exported fn takes `generic-input` (`input`, `user-profile`, `context` —
  each `option<list<u8>>`) and returns `result<list<u8>, string>`. JSON in/out.
- Bindings: `wit_bindgen::generate!({ world, path: "wit", additional_derives:
  [serde::Deserialize, serde::Serialize], generate_all });` then
  `export!(Component);` — gate the `Guest` impl and `export!` on
  `#[cfg(target_arch = "wasm32")]` so native tests still build.
- **Keep the pure logic in a host-free module** (here `render.rs`) so it unit-
  tests natively. Do **not** add `.cargo/config.toml` with a default
  `target = "wasm32-wasip2"` — it breaks `cargo test` (BUGS.md T3-2).

## Verified host interface signatures

```wit
// host:interfaces@2.1.0
http-with-placeholders.call(request) -> result<response, http-error>;
variant http-error { egress-denied(string), placeholder-denied(string),
  placeholder-unknown(string), placeholder-no-user-context, upstream-error(string) }
kv-store.get(map-name: string, key: list<u8>) -> result<option<list<u8>>, string>;
// (+ put / delete / scan / set-claims-digest)
logging.{info,debug,error}(msg: string) -> result<_, string>;
record request  { method: verb, url: string,
                  headers: option<list<tuple<string,string>>>, payload: option<list<u8>> }
record response { code: u16, payload: list<u8> }

// host:tenant@1.0.0
tenant-context.tenant-did() -> list<u8>;
tenant-context.contract-id() -> u32;
tenant-context.calling-user-did() -> option<list<u8>>;
tenant-context.cluster-timestamp-secs() -> u64;
tenant-context.seq-no() -> u64;
```

## Two secrets, two mechanisms

- **Recipient PII → placeholders.** Put `{{profile.<field>}}` in the outbound
  body/URL; the host resolves it inside the enclave against the calling user's
  **verified** profile. Only the `profile` namespace is allowed; `{{secrets.*}}`
  is rejected (`placeholder-denied`). PII never enters WASM.
- **Provider API key → sealed KV.** Read `z:<tid>:secrets` via `kv-store.get`
  (`tid` = `hex(tenant-did())`). Use it for the auth header; never log or return
  it. Seed it from the tenant side with
  `tenant.executeControl("map-entry-set", { map_name, key, value })` — a
  control-plane write that bypasses the writers ACL.

## KV governance — default DENY

A new map denies all reads until its ACL names the reading `contract_id`. Create
with **both** readers and writers set:

```ts
tenant.maps.create({ tail: "secrets", visibility: "private",
  writers: { only: [contractId] }, readers: { only: [contractId] } });
```

Re-registering a contract mints a **new** `contract_id` → the map ACL goes
stale → silent read denials. Persist `contract_id` (we use `deployment.json`);
there's no SDK call to look it up later (BUGS.md T3-3).

## SDK v5.1.0 flow (TypeScript)

```ts
setEnvironment(env); const nodeUrl = getNodeUrl();
const wasm = await loadWasmComponent();
const address = eth_get_address(apiKey);
const t3n = new T3nClient({ trustAnchor: await fetchTrustedManifest(env),
  wasmComponent: wasm, handlers: { EthSign: metamask_sign(address, undefined, apiKey) } });
await t3n.handshake();
const { value: did } = await t3n.authenticate(createEthAuthInput(address));
const tenant = new TenantClient({ t3n, baseUrl: nodeUrl, tenantDid: did });

// deploy
const { contract_id } = await tenant.contracts.register({ tail, version, wasm });
await tenant.maps.create({ tail: "secrets", visibility: "private",
  writers: { only: [contract_id] }, readers: { only: [contract_id] } });
await tenant.executeControl("map-entry-set",
  { map_name: tenant.canonicalName("secrets"), key: "resend_api_key", value });

// grant (data owner authorizes an agent for a contract + functions + hosts)
const v  = await getContractVersion(nodeUrl, scriptName);
const uv = await getContractVersion(nodeUrl, "tee:user/contracts");
await t3n.execute({ contract_id: "tee:user/contracts", contract_version: uv,
  function_name: "agent-auth-update",
  input: { agents: [{ agentDid: did, scripts: [{ scriptName, versionReq: v,
    functions: [...], allowedHosts: ["api.resend.com"] }] }] } });

// invoke
await t3n.executeAndDecode({ contract_id: scriptName, contract_version: v,
  function_name: "render-notification", input });
```

Re-register requires a strictly-higher version — auto-bump the patch on
`/not higher than current/`.

## Failure signals (what each means)

- `host/http.egress_denied` → no/mismatched grant (host or function not
  authorized). Run the grant.
- `placeholder-unknown` / `-no-user-context` → caller's profile lacks the field
  the `{{profile.*}}` marker names. Needs a verified contact.
- Provider `HTTP 401` → egress path fully worked; the credential is bad
  (e.g. placeholder key). Config, not code.
- Every RPC error carries a bracketed request id — quote it to support.

## Build / test / run (this repo)

```bash
cd app && npm install
npm run test:contract     # native unit tests (release mode; see BUGS.md E-1 on Windows)
npm run build:contract    # → contract/target/wasm32-wasip2/release/z_tenant_notify.wasm
npm run deploy && npm run grant && npm run render && npm run send
```

Never commit `app/.env` (holds the private key). It's gitignored and CI enforces
it.
