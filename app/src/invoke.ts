import { getContractVersion } from "@terminal3/t3n-sdk";
import { connect } from "./session";
import { readDeployment } from "./deployment";

/**
 * A realistic "your order shipped" notification. Note the two placeholder
 * namespaces:
 *   {{var.*}}      — business data, substituted in-enclave by render-notification
 *   {{profile.*}}  — recipient PII, left intact and resolved host-side at send
 *
 * The recipient address is NOT here: send-notification always targets the
 * calling user's own verified email ({{profile.verified_contacts.email.value}}),
 * so the enterprise never supplies or sees it.
 */
const SAMPLE = {
  subject_template: "Your order {{var.order_number}} is on its way 📦",
  body_template:
    "Hi {{profile.first_name}},\n\n" +
    "Good news — order {{var.order_number}} shipped via {{var.carrier}}.\n" +
    "Track it here: {{var.tracking_url}}\n\n" +
    "We'll email your verified address again the moment it's delivered.\n\n" +
    "— The {{var.company}} team",
  variables: {
    order_number: "AB-10024",
    carrier: "DHL Express",
    tracking_url: "https://track.example.com/AB-10024",
    company: "Acme",
  },
  // A verified sender you control. `onboarding@resend.dev` works out of the box
  // with any Resend account for test sends.
  from: "Acme <onboarding@resend.dev>",
  reply_to: "support@acme.example",
};

async function coords() {
  const { t3n, nodeUrl } = await connect();
  const dep = readDeployment();
  if (!dep) throw new Error("No deployment.json — run `npm run deploy` first.");
  const version = await getContractVersion(nodeUrl, dep.scriptName);
  return { t3n, scriptName: dep.scriptName, version };
}

/** Dry-run: render + validate, no egress, no PII resolution, no credit on the provider. */
export async function render(): Promise<void> {
  const { t3n, scriptName, version } = await coords();
  console.log(`render-notification  ${scriptName} v${version}`);
  const out = await t3n.executeAndDecode({
    contract_id: scriptName,
    contract_version: version,
    function_name: "render-notification",
    input: SAMPLE,
  });
  console.log("RESULT:");
  console.log(JSON.stringify(out, null, 2));
}

/** The real thing: render, read the sealed key, and egress via http-with-placeholders. */
export async function send(): Promise<void> {
  const { t3n, scriptName, version } = await coords();
  console.log(`send-notification  ${scriptName} v${version}`);
  try {
    const out = await t3n.executeAndDecode({
      contract_id: scriptName,
      contract_version: version,
      function_name: "send-notification",
      input: SAMPLE,
    });
    console.log("SENT:");
    console.log(JSON.stringify(out, null, 2));
  } catch (e: any) {
    // A failure here is still informative: it tells us exactly how far the
    // enclave got (secret read → placeholder resolution → egress). Report it
    // verbatim rather than pretending success.
    console.log("send-notification returned an error:");
    console.log("  " + String(e?.message || e));
    console.log(
      "\nExpected when: (a) RESEND_API_KEY is the placeholder → Resend HTTP 401, or\n" +
        "               (b) the calling identity has no verified email on its profile\n" +
        "                   → placeholder-unknown / placeholder-no-user-context.\n" +
        "See docs/ARCHITECTURE.md → 'What a production send needs'.",
    );
  }
}
