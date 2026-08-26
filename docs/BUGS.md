# Bugs & friction encountered

Reported constructively, with repro and suggested fixes. Split into **Terminal 3
issues** (actionable by T3N) and **environment notes** (local to this build,
recorded so the next developer isn't surprised). Severity is from a builder's
perspective: how much it slowed a first-time integration.

---

## Terminal 3 issues

### T3-1 · `http-with-placeholders` documented signature didn't match the shipped WIT — Medium

**Where:** docs *Tips → Placeholders in outbound calls* vs. the actual
`host:interfaces@2.1.0` WIT resolved for the contract.

**What:** The prose/example implied a return/error shape that differs from the
WIT the toolchain actually binds. The shipped interface is:

```wit
call: func(request: request) -> result<response, http-error>;
variant http-error {
  egress-denied(string),
  placeholder-denied(string),
  placeholder-unknown(string),
  placeholder-no-user-context,
  upstream-error(string),
}
```

**Impact:** Following the doc, the error handling in `send.rs` didn't compile
until I matched the real variant set (five variants, one of them field-less).
Cost ~30 min of reconciling doc vs. generated bindings.

**Suggested fix:** Generate the tips page's signature block directly from the
published WIT, or link to the canonical `.wit` so the two can't drift.

### T3-2 · Reference/quickstart ships `.cargo/config.toml` that breaks native `cargo test` — Medium

**Where:** the reference contract template includes:

```toml
# .cargo/config.toml
[build]
target = "wasm32-wasip2"
```

**What:** Setting a global default target means **every** cargo invocation —
including `cargo test` — targets WASM. Unit tests then can't run on the host
(there's no native test runner for `wasm32-wasip2` out of the box), so a
newcomer copying the template finds `cargo test` mysteriously trying to
build/run WASM.

**Impact:** Pure logic (template substitution, allowlisting) is the most
valuable thing to unit-test, and the template makes that hard by default.

**Suggested fix:** Don't pin a default target in the template. Instead document
`cargo build --target wasm32-wasip2 --release` for the build step (this project
does exactly that — see `app/package.json` → `build:contract`), leaving
`cargo test` to run natively. This repo deliberately **omits** the
`.cargo/config.toml` for this reason, and its logic is 100% host-testable.

### T3-3 · No SDK call to look up a contract's current `contract_id` by tail — Low/Medium

**Where:** `TenantClient.contracts` (SDK v5.1.0).

**What:** `contracts.register({ tail, version, wasm })` returns a `contract_id`,
and re-registering a tail requires a strictly-higher version and mints a **new**
`contract_id`. But there is no `contracts.get({ tail })` (or similar) to
*retrieve* the current id/version later.

**Impact:** The `contract_id` matters beyond registration — it's what a KV map's
reader/writer ACL is keyed on. If you lose the value returned at register time
(this project persists it in `deployment.json` precisely to avoid that), you
can't cleanly rediscover it, and a stale ACL causes silent KV read denials after
a redeploy.

**Suggested fix:** Add a read API returning the current `contract_id` (and
version) for a tail, so tooling can reconcile KV ACLs after a re-register
without bookkeeping.

### T3-4 · KV maps default-deny is easy to miss — Low

**Where:** map creation (`tenant.maps.create`) + first read from a contract.

**What:** A newly created KV map denies all reads until its ACL names the
reading `contract_id`. It's documented in *Tips → Create KV maps*, but the
failure mode (a silent `kv-store.get` denial from inside the enclave) is far
from the place you'd look, so it's easy to burn time on.

**Impact:** ~20 min the first time, until the ACL was set on both `readers` and
`writers`.

**Suggested fix:** Make the default-deny behavior prominent in the KV quickstart
(a one-line "⚠️ readers/writers must include your contract_id or reads fail"),
and consider a clearer host-side error string for an ACL denial vs. a
missing key.

---

## Environment notes (not Terminal 3 bugs)

Recorded for reproducibility; these are local to the build machine/tooling.

### E-1 · Windows WDAC blocked freshly-linked debug build-scripts (`os error 4551`)

On this Windows 11 host, `cargo test` (debug profile) failed while executing a
freshly-linked build-script binary under `target/debug/build/…` with
*"An Application Control policy has blocked this file (os error 4551)"* — a
WDAC/AppLocker policy, not anything in the code. **Workaround:** run the native
tests in release mode (`cargo test --release`), which reuses the
already-validated release build-script binaries produced during the WASM build.
CI on Linux runs plain `cargo test` with no issue. `app/package.json`'s
`test:contract` uses `--release` for this reason.

### E-2 · Doc-fetching tool stripped fenced code blocks

While researching, fetching the docs pages through an HTML-to-markdown path
dropped the contents of fenced code blocks, which made API signatures hard to
read remotely. **Workaround:** mirrored the docs locally (kept out of this repo)
and read the raw pages. Not a T3N product issue — noted only to explain why some
signatures above were verified against the shipped WIT rather than the rendered
docs.

---

## Not bugs (expected behavior, verified)

- `{{secrets.*}}` placeholders are rejected by the host (`placeholder-denied`).
  This is correct and important — it's what stops a sealed secret being
  smuggled into an outbound URL/body. Verified, not a defect.
- Re-registering a tail requires a strictly-higher version. Sensible; the
  deploy script auto-bumps the patch level to accommodate it.
- `send` returning HTTP 401 with a placeholder key is the provider rejecting a
  bad credential, i.e. proof the egress path works end-to-end (see
  [RUN_LOG.md](RUN_LOG.md)).
