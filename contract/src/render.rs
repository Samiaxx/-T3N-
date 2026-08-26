//! Pure notification rendering + placeholder validation.
//!
//! This module has NO host dependencies and NO `#[cfg(target_arch)]` gates: it
//! behaves identically on wasm32 and native, which is exactly why the whole
//! rendering + validation surface is exercised by ordinary `cargo test`.
//! [`crate::send`] calls [`render`] *before* it reads any secret or touches the
//! network, so a bad template fails fast — inside the enclave, before a single
//! credit or email is spent.
//!
//! ## Template language (deliberately tiny)
//!
//! Two placeholder namespaces, both written `{{namespace.field}}`:
//!
//! * `{{var.<name>}}`     — a business variable (order number, amount, carrier).
//!                          Substituted HERE, in the enclave, from the caller's
//!                          `variables` map. Non-PII by contract.
//! * `{{profile.<field>}}`— recipient PII (name, verified email). Left VERBATIM
//!                          in the output and resolved by the host at send time
//!                          via `http-with-placeholders`, so it never enters
//!                          WASM plaintext. Only allow-listed fields are legal.
//!
//! Any other namespace (e.g. `{{secrets.*}}`), an unknown profile field, an
//! unresolved variable, or unbalanced `{{`/`}}` is a hard error.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The only host this contract ever talks to. Must appear both in the
/// contract's `http_allow_list` and in the user's agent-auth grant
/// `allowedHosts`, or the send egress is denied.
pub const EGRESS_HOST: &str = "api.resend.com";

/// Profile fields a template may reference via `{{profile.<field>}}`.
///
/// Defence-in-depth: the host enforces the `profile`-namespace gate too, but
/// validating here turns a runtime egress failure into a friendly, credit-free
/// error ("unknown profile field: ssn") AND documents, in exactly one place,
/// the complete set of PII this agent can ever touch. To touch a new field,
/// add it here — the diff is the audit trail.
pub const ALLOWED_PROFILE_FIELDS: &[&str] = &[
    "first_name",
    "last_name",
    "full_name",
    "date_of_birth",
    "verified_contacts.email.value",
];

/// Request payload shared by `render-notification` and `send-notification`.
/// `from` / `reply_to` are send-time envelope fields; `render` accepts and
/// ignores them so one JSON shape drives both functions.
#[derive(Debug, Deserialize)]
pub struct RenderInput {
    pub subject_template: String,
    pub body_template: String,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub reply_to: Option<String>,
}

/// Result of a dry-run render. `body`/`subject` still contain any
/// `{{profile.*}}` markers — those are resolved host-side at send time.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct RenderedNotification {
    pub subject: String,
    pub body: String,
    /// PII fields still present as `{{profile.*}}` markers (sorted, unique).
    pub profile_fields: Vec<String>,
    pub egress_host: String,
}

/// Render both templates and validate every placeholder.
pub fn render(input: &RenderInput) -> Result<RenderedNotification, String> {
    let mut profile_fields = BTreeSet::new();
    let mut unresolved = BTreeSet::new();

    let subject = substitute(
        &input.subject_template,
        &input.variables,
        &mut profile_fields,
        &mut unresolved,
    )?;
    let body = substitute(
        &input.body_template,
        &input.variables,
        &mut profile_fields,
        &mut unresolved,
    )?;

    if !unresolved.is_empty() {
        return Err(format!(
            "unresolved template variables (no value supplied): {}",
            unresolved.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }

    Ok(RenderedNotification {
        subject,
        body,
        profile_fields: profile_fields.into_iter().collect(),
        egress_host: EGRESS_HOST.to_string(),
    })
}

/// Entry point for the `render-notification` export. `input` is the raw JSON
/// from `generic-input.input`. Pure — identical native and on wasm32.
pub fn render_notification(input: &[u8]) -> Result<Vec<u8>, String> {
    let req: RenderInput =
        serde_json::from_slice(input).map_err(|e| format!("render-notification: bad input: {e}"))?;
    let rendered = render(&req)?;
    serde_json::to_vec(&rendered).map_err(|e| e.to_string())
}

/// Walk `template`, replacing `{{var.*}}` from `variables`, keeping allow-listed
/// `{{profile.*}}` markers verbatim, and rejecting anything else. UTF-8 safe:
/// `{`, `}` and `.` are ASCII, so every `find` offset lands on a char boundary.
fn substitute(
    template: &str,
    variables: &BTreeMap<String, String>,
    profile_fields: &mut BTreeSet<String>,
    unresolved: &mut BTreeSet<String>,
) -> Result<String, String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 2..];
        let close = after_open
            .find("}}")
            .ok_or("unbalanced placeholder braces: found `{{` with no matching `}}`")?;
        let inner = after_open[..close].trim();

        let (namespace, field) = inner.split_once('.').ok_or_else(|| {
            format!("malformed placeholder `{{{{{inner}}}}}`: expected `<namespace>.<field>`")
        })?;

        match namespace {
            "var" => match variables.get(field) {
                Some(value) => out.push_str(value),
                None => {
                    unresolved.insert(field.to_string());
                    // Keep a readable marker so a preview still makes sense.
                    out.push_str("{{var.");
                    out.push_str(field);
                    out.push_str("}}");
                }
            },
            "profile" => {
                if !ALLOWED_PROFILE_FIELDS.contains(&field) {
                    return Err(format!(
                        "unknown profile field `{field}` — allowed: {}",
                        ALLOWED_PROFILE_FIELDS.join(", ")
                    ));
                }
                profile_fields.insert(field.to_string());
                // Keep verbatim for host-side resolution.
                out.push_str("{{profile.");
                out.push_str(field);
                out.push_str("}}");
            }
            other => {
                return Err(format!(
                    "disallowed placeholder namespace `{other}` in `{{{{{inner}}}}}` — only `var` and `profile` are permitted"
                ));
            }
        }

        rest = &after_open[close + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(subject: &str, body: &str, vars: &[(&str, &str)]) -> RenderInput {
        RenderInput {
            subject_template: subject.to_string(),
            body_template: body.to_string(),
            variables: vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            from: None,
            reply_to: None,
        }
    }

    #[test]
    fn substitutes_vars_and_keeps_profile_markers() {
        let r = render(&input(
            "Your order {{var.order_number}} has shipped",
            "Hi {{profile.first_name}}, order {{var.order_number}} shipped via {{var.carrier}}.",
            &[("order_number", "AB-10024"), ("carrier", "DHL")],
        ))
        .unwrap();

        assert_eq!(r.subject, "Your order AB-10024 has shipped");
        assert_eq!(
            r.body,
            "Hi {{profile.first_name}}, order AB-10024 shipped via DHL."
        );
        assert_eq!(r.profile_fields, vec!["first_name".to_string()]);
        assert_eq!(r.egress_host, "api.resend.com");
    }

    #[test]
    fn repeated_variable_is_substituted_everywhere() {
        let r = render(&input(
            "{{var.n}}",
            "{{var.n}} and again {{var.n}}",
            &[("n", "X")],
        ))
        .unwrap();
        assert_eq!(r.body, "X and again X");
    }

    #[test]
    fn profile_fields_are_sorted_and_deduped() {
        let r = render(&input(
            "{{profile.last_name}}",
            "{{profile.first_name}} {{profile.last_name}} {{profile.first_name}}",
            &[],
        ))
        .unwrap();
        assert_eq!(
            r.profile_fields,
            vec!["first_name".to_string(), "last_name".to_string()]
        );
    }

    #[test]
    fn dotted_profile_field_is_allowed() {
        let r = render(&input(
            "s",
            "mail: {{profile.verified_contacts.email.value}}",
            &[],
        ))
        .unwrap();
        assert_eq!(
            r.profile_fields,
            vec!["verified_contacts.email.value".to_string()]
        );
    }

    #[test]
    fn unresolved_variable_is_an_error() {
        let err = render(&input("s", "{{var.missing}}", &[])).unwrap_err();
        assert!(err.contains("unresolved template variables"), "{err}");
        assert!(err.contains("missing"), "{err}");
    }

    #[test]
    fn unknown_profile_field_is_rejected() {
        let err = render(&input("s", "{{profile.ssn}}", &[])).unwrap_err();
        assert!(err.contains("unknown profile field"), "{err}");
        assert!(err.contains("ssn"), "{err}");
    }

    #[test]
    fn secrets_namespace_is_rejected() {
        let err = render(&input("s", "{{secrets.resend_api_key}}", &[])).unwrap_err();
        assert!(err.contains("disallowed placeholder namespace"), "{err}");
        assert!(err.contains("secrets"), "{err}");
    }

    #[test]
    fn placeholder_without_namespace_is_rejected() {
        let err = render(&input("s", "{{first_name}}", &[])).unwrap_err();
        assert!(err.contains("malformed placeholder"), "{err}");
    }

    #[test]
    fn unbalanced_braces_are_rejected() {
        let err = render(&input("s", "hello {{var.x", &[("x", "1")])).unwrap_err();
        assert!(err.contains("unbalanced placeholder braces"), "{err}");
    }

    #[test]
    fn utf8_body_is_preserved() {
        let r = render(&input("s", "Café ☕ {{var.n}} — déjà", &[("n", "5")])).unwrap();
        assert_eq!(r.body, "Café ☕ 5 — déjà");
    }

    #[test]
    fn plain_text_without_markers_passes_through() {
        let r = render(&input("Receipt", "Thank you for your purchase.", &[])).unwrap();
        assert_eq!(r.body, "Thank you for your purchase.");
        assert!(r.profile_fields.is_empty());
    }

    #[test]
    fn render_notification_bytes_roundtrip() {
        let json = serde_json::to_vec(&serde_json::json!({
            "subject_template": "Hi {{profile.first_name}}",
            "body_template": "Order {{var.id}} confirmed.",
            "variables": { "id": "42" }
        }))
        .unwrap();
        let out = render_notification(&json).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["subject"], "Hi {{profile.first_name}}");
        assert_eq!(v["body"], "Order 42 confirmed.");
        assert_eq!(v["profile_fields"][0], "first_name");
    }

    #[test]
    fn render_notification_rejects_bad_json() {
        let err = render_notification(b"not json").unwrap_err();
        assert!(err.contains("bad input"), "{err}");
    }
}
