//! send_notification: render, then POST the notification to the email provider
//! (Resend) via `host:interfaces/http-with-placeholders`.
//!
//! Two secrets never enter WASM plaintext:
//!   * the recipient address — sent as `{{profile.verified_contacts.email.value}}`
//!     and resolved host-side from the calling user's profile at dispatch time;
//!   * the provider API key — read from `z:<tid>:secrets` and placed in the
//!     `Authorization` header. It is never returned to the caller and never
//!     logged.
//!
//! Ordering is deliberate: [`crate::render::render`] runs first, so a template
//! mistake fails before we read the key or hit the network.

use crate::render::{self, RenderInput};
use serde::Serialize;

const RESEND_URL: &str = "https://api.resend.com/emails";

/// The recipient is ALWAYS the calling user's own verified email — never a
/// caller-supplied address. That is the consent gate: the enterprise can only
/// notify a customer who granted this agent access to their profile, and even
/// then never learns the address. Resolved host-side.
const RECIPIENT_PLACEHOLDER: &str = "{{profile.verified_contacts.email.value}}";

/// KV key under `z:<tid>:secrets` holding the Resend API key.
const SECRET_KEY: &str = "resend_api_key";

#[derive(Serialize)]
struct SendResult {
    provider_message_id: String,
    status: String,
}

/// Entry point for the `send-notification` export. `input` is the raw JSON from
/// `generic-input.input`.
pub fn send_notification(input: &[u8]) -> Result<Vec<u8>, String> {
    let req: RenderInput =
        serde_json::from_slice(input).map_err(|e| format!("send-notification: bad input: {e}"))?;

    // Render + validate FIRST. A bad template fails here — before any secret
    // read or egress, so no credit and no email is spent on a broken message.
    let rendered = render::render(&req)?;

    let from = req
        .from
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or("send-notification: `from` (a verified sender you control) is required")?
        .to_string();

    #[cfg(target_arch = "wasm32")]
    {
        let result = send_wasm(&from, req.reply_to.as_deref(), &rendered)?;
        serde_json::to_vec(&result).map_err(|e| e.to_string())
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        // Egress requires host interfaces that only exist inside the enclave.
        // Everything above (parse, render, validate, `from` check) has already
        // run natively — which is what the native tests below assert on.
        let _ = (from, rendered);
        Err("send_notification egress is only implemented on the wasm32 target".to_string())
    }
}

#[cfg(target_arch = "wasm32")]
use crate::host::{
    interfaces::{http_with_placeholders as hwp, kv_store, logging},
    tenant::tenant_context,
};

#[cfg(target_arch = "wasm32")]
fn send_wasm(
    from: &str,
    reply_to: Option<&str>,
    rendered: &render::RenderedNotification,
) -> Result<SendResult, String> {
    use serde_json::json;

    let api_key = get_api_key()?;

    // The `to` value is the recipient placeholder; the host substitutes the
    // calling user's verified email into the body bytes inside the enclave,
    // after we serialise and before the outbound call. We never hold it.
    let mut payload = json!({
        "from": from,
        "to": [RECIPIENT_PLACEHOLDER],
        "subject": rendered.subject,
        "text": rendered.body,
    });
    if let Some(rt) = reply_to.filter(|s| !s.is_empty()) {
        payload["reply_to"] = json!(rt);
    }

    // Log only non-PII metadata: the fields we asked the host to resolve, never
    // their values.
    let _ = logging::info(&format!(
        "send-notification: POST {RESEND_URL} (host-resolved profile fields: [{}])",
        rendered.profile_fields.join(", ")
    ));

    let resp = hwp::call(&hwp::Request {
        method: hwp::Verb::Post,
        url: RESEND_URL.to_string(),
        headers: Some(resend_headers(&api_key)),
        payload: Some(serde_json::to_vec(&payload).map_err(|e| e.to_string())?),
    })
    .map_err(|e| format!("resend send: {}", format_http_error(e)))?;

    if resp.code != 200 && resp.code != 201 {
        // Body may echo the recipient — log the status only, never the body.
        let _ = logging::error(&format!("Resend create-email HTTP {}", resp.code));
        return Err(format!("Resend send failed: HTTP {}", resp.code));
    }

    let body: serde_json::Value =
        serde_json::from_slice(&resp.payload).map_err(|e| e.to_string())?;
    let id = body["id"]
        .as_str()
        .ok_or("Resend response missing `id`")?
        .to_string();

    let _ = logging::info(&format!("Resend accepted notification: id={id}"));

    Ok(SendResult {
        provider_message_id: id,
        status: "sent".to_string(),
    })
}

#[cfg(target_arch = "wasm32")]
fn get_api_key() -> Result<String, String> {
    let tid = tenant_context::tenant_did();
    let map_name = format!("z:{}:secrets", hex::encode(&tid));
    let bytes = kv_store::get(&map_name, SECRET_KEY.as_bytes())
        .map_err(|e| format!("kv read: {e}"))?
        .ok_or("resend_api_key not found in z:<tid>:secrets — seed it via the orchestrator (`npm run deploy`) before use")?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
fn resend_headers(api_key: &str) -> Vec<(String, String)> {
    // Content-Type is set host-side (same as the T3N reference contract);
    // sending it here would duplicate the header and some providers reject that.
    vec![
        ("Authorization".to_string(), format!("Bearer {api_key}")),
        ("Accept".to_string(), "application/json".to_string()),
    ]
}

/// Render a typed `http-with-placeholders` error as a caller-facing string.
/// Never includes resolved PII — only field names and host-side reasons.
#[cfg(target_arch = "wasm32")]
fn format_http_error(e: hwp::HttpError) -> String {
    match e {
        hwp::HttpError::EgressDenied(host) => {
            format!("egress denied for host {host} — add it to the agent-auth grant `allowedHosts`")
        }
        hwp::HttpError::PlaceholderDenied(marker) => format!("placeholder not permitted: {marker}"),
        hwp::HttpError::PlaceholderUnknown(field) => {
            format!("recipient profile is missing field: {field}")
        }
        hwp::HttpError::PlaceholderNoUserContext => {
            "no user context bound — send-notification must be invoked by (or on behalf of) the data owner"
                .to_string()
        }
        hwp::HttpError::UpstreamError(reason) => format!("upstream: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json_input(v: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&v).unwrap()
    }

    #[test]
    fn valid_input_reaches_egress_gate_on_native() {
        // Parse + render + `from` check all succeed; only the wasm-only egress
        // is unavailable natively. Proves the whole pre-egress path is sound.
        let err = send_notification(&json_input(serde_json::json!({
            "subject_template": "Order {{var.id}}",
            "body_template": "Hi {{profile.first_name}}, order {{var.id}} shipped.",
            "variables": { "id": "AB-1" },
            "from": "notifications@acme.example"
        })))
        .unwrap_err();
        assert!(
            err.contains("only implemented on the wasm32 target"),
            "{err}"
        );
    }

    #[test]
    fn bad_json_is_rejected() {
        let err = send_notification(b"not json").unwrap_err();
        assert!(err.contains("bad input"), "{err}");
    }

    #[test]
    fn missing_from_is_rejected_before_egress() {
        let err = send_notification(&json_input(serde_json::json!({
            "subject_template": "s",
            "body_template": "b",
        })))
        .unwrap_err();
        assert!(err.contains("`from`"), "{err}");
    }

    #[test]
    fn bad_template_fails_before_from_check_and_egress() {
        // A disallowed placeholder must surface from render() — proving we fail
        // fast, before touching the `from` check, any secret, or the network.
        let err = send_notification(&json_input(serde_json::json!({
            "subject_template": "s",
            "body_template": "{{secrets.resend_api_key}}",
            "from": "notifications@acme.example"
        })))
        .unwrap_err();
        assert!(err.contains("disallowed placeholder namespace"), "{err}");
    }
}
