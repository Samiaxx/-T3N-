import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

// Load app/.env with no dependency. Real secrets live here and ONLY here —
// this file is gitignored. Values already present in the environment win, so
// CI can inject them without a .env file.
const here = dirname(fileURLToPath(import.meta.url));
const repoApp = resolve(here, "..");

try {
  for (const line of readFileSync(resolve(repoApp, ".env"), "utf8").split("\n")) {
    const m = line.match(/^\s*([A-Z0-9_]+)\s*=\s*(.*?)\s*$/);
    if (m && process.env[m[1]] === undefined) process.env[m[1]] = m[2];
  }
} catch {
  /* .env is optional when the vars are already exported */
}

function required(name: string): string {
  const v = process.env[name];
  if (!v) throw new Error(`Missing required env var ${name} — set it in app/.env (see .env.example)`);
  return v;
}

/**
 * All orchestrator configuration in one place. Nothing here is secret except
 * `apiKey` and `resendApiKey`, both sourced from the gitignored .env.
 */
export const config = {
  // --- Identity (Terminal 3) ---
  apiKey: required("T3N_API_KEY"),
  tenantDid: required("T3N_TENANT_DID"),
  env: (process.env.T3N_ENV || "testnet") as "testnet" | "sandbox" | "production",

  // --- Contract coordinates ---
  // Keep the tail short — it is reused in delegation grants downstream.
  contractTail: process.env.CONTRACT_TAIL || "notify",
  contractVersion: process.env.CONTRACT_VERSION || "0.1.0",
  wasmPath:
    process.env.WASM_PATH ||
    resolve(repoApp, "..", "contract", "target", "wasm32-wasip2", "release", "z_tenant_notify.wasm"),

  // --- Provider secret (sealed into z:<tid>:secrets) ---
  secretMapTail: "secrets",
  secretKey: "resend_api_key",
  // Falls back to a clearly-fake sentinel so the send path can be exercised
  // end-to-end (it will get an HTTP 401 from Resend) even without a real key.
  resendApiKey: process.env.RESEND_API_KEY || "re_TEST_PLACEHOLDER_replace_me",

  // --- Egress ---
  // The one host this agent may ever reach. Mirrors EGRESS_HOST in the contract
  // (contract/src/render.rs) and is what we put in the agent-auth grant.
  egressHost: "api.resend.com",

  // Where deploy.ts records the registered contract_id (no secrets; committable).
  deploymentFile: resolve(repoApp, "deployment.json"),
} as const;

/** Build the canonical `z:<tid>:<tail>` script name from a DID. */
export function tenantScript(did: string, tail = config.contractTail): string {
  return `z:${did.slice("did:t3n:".length)}:${tail}`;
}
