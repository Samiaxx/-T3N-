# Run log — live testnet deployment

A verbatim, end-to-end run against the Terminal 3 **testnet** node
`https://cn-api.sg.testnet.t3n.terminal3.io` on 2026-08-26. Nothing here is
edited except the redaction of no secrets (there are none in this output).

Reproduce it yourself with `npm run all` after configuring `app/.env`.

---

## 1. `npm run whoami` — authenticate

```
env=testnet
node=https://cn-api.sg.testnet.t3n.terminal3.io
tenant DID=did:t3n:6184228ea2fb5c0bfec58436e5d8e003dcfcfc2e
```

Handshake + Ethereum-signature authentication succeed; the authenticated DID
matches the configured tenant DID (the session guard passes).

## 2. `npm run deploy` — register, create map, seal key

```
WASM: C:\Users\owner\docv2\contract\target\wasm32-wasip2\release\z_tenant_notify.wasm (181339 bytes)
Registered z:6184228ea2fb5c0bfec58436e5d8e003dcfcfc2e:notify v0.1.0 → contract_id=746
Created map z:6184228ea2fb5c0bfec58436e5d8e003dcfcfc2e:secrets (readers/writers = [746])
Sealed resend_api_key into z:6184228ea2fb5c0bfec58436e5d8e003dcfcfc2e:secrets — PLACEHOLDER (set RESEND_API_KEY in app/.env for a real send)
Wrote C:\Users\owner\docv2\app\deployment.json
Deploy complete → next: npm run grant
```

The 177 KB WASM registers as `contract_id=746`; the `secrets` KV map is created
with the contract in its readers/writers ACL (KV governance defaults to deny —
this ACL is mandatory); the provider key is sealed via the control plane.

## 3. `npm run grant` — authorize the caller

```
Self-grant  did:t3n:6184228ea2fb5c0bfec58436e5d8e003dcfcfc2e
  → z:6184228ea2fb5c0bfec58436e5d8e003dcfcfc2e:notify v0.1.0
  functions:    render-notification, send-notification
  allowedHosts: api.resend.com
Grant applied → next: npm run render  (then: npm run send)
```

`agent-auth-update` scopes the authorization three ways: which contract, which
functions, and which egress host.

## 4. `npm run render` — headline demo (pure, deterministic)

```
render-notification  z:6184228ea2fb5c0bfec58436e5d8e003dcfcfc2e:notify v0.1.0
RESULT:
{
  "subject": "Your order AB-10024 is on its way 📦",
  "body": "Hi {{profile.first_name}},\n\nGood news — order AB-10024 shipped via DHL Express.\nTrack it here: https://track.example.com/AB-10024\n\nWe'll email your verified address again the moment it's delivered.\n\n— The Acme team",
  "profile_fields": [
    "first_name"
  ],
  "egress_host": "api.resend.com"
}
```

Business variables (`order_number`, `carrier`, …) are substituted in-enclave;
the `{{profile.first_name}}` marker is preserved for host-side resolution; the
contract reports exactly which PII field it will need and the single host it may
reach. No egress, no PII, no provider credit consumed.

## 5. `npm run send` — the real send (placeholder key)

```
send-notification  z:6184228ea2fb5c0bfec58436e5d8e003dcfcfc2e:notify v0.1.0
send-notification returned an error:
  RPC Error: contract error: Resend send failed: HTTP 401 [70e0207b-2700-4e9f-bb7e-0066f4b15757]
```

**This 401 is the success signal for the confidential path.** For Resend to
return 401, the request had to leave the enclave and reach `api.resend.com` —
which means:

1. the contract read the sealed `resend_api_key` from the KV map (a missing key
   would fail earlier, inside the enclave);
2. the host **allowed** egress to `api.resend.com` — the grant from step 3
   worked (without it the call is rejected with `host/http.egress_denied`
   before leaving the node);
3. Resend received the request and rejected the **placeholder** credential.

Swap the placeholder for a real `RESEND_API_KEY` in `app/.env`, redeploy, and
the identical path delivers a real email. The bracketed value is the T3N
request id, useful for support.

---

## Contract tests (local)

```
cargo test --release --offline
```

18 unit tests + 1 doc-test pass. The pure template engine
(`contract/src/render.rs`) is exhaustively covered — variable substitution,
profile-field allowlisting, secret-namespace rejection, malformed placeholders,
and UTF-8 handling. See [BUGS.md](BUGS.md) for why this project runs the native
tests in `--release` mode on this Windows host.
