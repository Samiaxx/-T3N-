import {
  T3nClient,
  TenantClient,
  setEnvironment,
  loadWasmComponent,
  eth_get_address,
  metamask_sign,
  createEthAuthInput,
  fetchTrustedManifest,
  getNodeUrl,
} from "@terminal3/t3n-sdk";
import { config } from "./env";

export interface Session {
  /** Low-level authenticated client — used for execute / executeAndDecode. */
  t3n: T3nClient;
  /** Tenant-plane client — register contracts, create maps, seed secrets. */
  tenant: TenantClient;
  /** The authenticated DID (must equal config.tenantDid). */
  did: string;
  /** Active node URL from setEnvironment(). */
  nodeUrl: string;
}

let cached: Session | null = null;

/**
 * Authenticate once and return a ready session.
 *
 * This project uses the **self-call** model: a single identity (your
 * T3N_API_KEY → tenant DID) plays all three roles — the tenant that owns and
 * deploys the contract, the data owner who grants access, and the caller who
 * invokes it. That keeps the demo runnable with one credential. In production
 * the caller is a separate agent DID and the grant is signed by the real data
 * owner; see docs/ARCHITECTURE.md.
 */
export async function connect(): Promise<Session> {
  if (cached) return cached;

  setEnvironment(config.env);
  const nodeUrl = getNodeUrl();
  const wasmComponent = await loadWasmComponent();
  const address = eth_get_address(config.apiKey);

  const t3n = new T3nClient({
    trustAnchor: await fetchTrustedManifest(config.env),
    wasmComponent,
    handlers: { EthSign: metamask_sign(address, undefined, config.apiKey) },
  });

  await t3n.handshake();
  const auth = await t3n.authenticate(createEthAuthInput(address));
  const did = auth.value;

  if (did !== config.tenantDid) {
    throw new Error(
      `Authenticated DID (${did}) does not match T3N_TENANT_DID (${config.tenantDid}). ` +
        `Check that T3N_API_KEY and T3N_TENANT_DID in app/.env belong to the same tenant.`,
    );
  }

  const tenant = new TenantClient({ t3n, baseUrl: nodeUrl, tenantDid: did });
  await tenant.tenant.me(); // throws if the tenant session is not usable

  cached = { t3n, tenant, did, nodeUrl };
  return cached;
}
