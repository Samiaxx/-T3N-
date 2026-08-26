//! z-tenant-notify v0.1.0 — consent-gated transactional notifications.
//!
//! A Rust→WASM contract that runs inside the Terminal 3 TEE and sends a
//! transactional email to a customer using the customer's *verified* contact
//! info and the enterprise's *sealed* provider key — where neither the
//! recipient address nor the provider key is ever visible to the calling agent
//! or the orchestrating app.
//!
//! Two exports:
//!   * `render-notification` — dry-run: substitute `{{var.*}}`, validate
//!     `{{profile.*}}`, no egress, no PII, no credit spend. See [`render`].
//!   * `send-notification`   — render, read the sealed provider key from
//!     `z:<tid>:secrets`, and POST via `http-with-placeholders` so recipient
//!     PII is resolved host-side inside the enclave. See [`send`].
//!
//! # Host-capability manifest
//!
//! Access to a user's profile is gated by the on-chain agent-auth grant, not a
//! per-field allowlist. Declare:
//! ```json
//! { "host_capabilities": ["kv_store", "logging", "tenant_context", "http_with_placeholders"] }
//! ```
//!
//! # Setup
//!
//! Before first use, the orchestrator seeds the provider key (see
//! `app/src/deploy.ts`), equivalent to:
//! ```text
//! z:<tid>:secrets["resend_api_key"] = "re_..."
//! ```
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

// wit-bindgen's generated bindings reference the `alloc` crate by path.
extern crate alloc;

/// Bumped in lockstep with the WIT world / `Cargo.toml` version.
pub const CONTRACT_VERSION: &str = "0.1.0";

wit_bindgen::generate!({
    world: "tenant-notify",
    path: "wit",
    additional_derives: [
        serde::Deserialize,
        serde::Serialize,
    ],
    generate_all,
});

mod render;
mod send;

struct Component;

#[cfg(target_arch = "wasm32")]
impl exports::z::tenant_notify::contracts::Guest for Component {
    fn render_notification(
        req: exports::z::tenant_notify::contracts::GenericInput,
    ) -> Result<Vec<u8>, String> {
        let input = req.input.ok_or("render-notification: missing input")?;
        render::render_notification(&input)
    }

    fn send_notification(
        req: exports::z::tenant_notify::contracts::GenericInput,
    ) -> Result<Vec<u8>, String> {
        let input = req.input.ok_or("send-notification: missing input")?;
        send::send_notification(&input)
    }
}

#[cfg(target_arch = "wasm32")]
export!(Component);

#[cfg(test)]
mod tests {
    use super::CONTRACT_VERSION;

    #[test]
    fn contract_version_is_semver() {
        let parts: Vec<&str> = CONTRACT_VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "CONTRACT_VERSION must be MAJOR.MINOR.PATCH");
        for part in parts {
            assert!(part.parse::<u32>().is_ok(), "each part must be numeric");
        }
    }
}
