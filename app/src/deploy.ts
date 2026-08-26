import { readFile } from "node:fs/promises";
import { connect } from "./session";
import { config, tenantScript } from "./env";
import { writeDeployment } from "./deployment";

function bumpPatch(version: string): string {
  const [maj, min, patch] = version.split(".").map((n) => parseInt(n, 10) || 0);
  return `${maj}.${min}.${patch + 1}`;
}

/**
 * One-shot, idempotent deployment:
 *   1. read the compiled WASM
 *   2. register it (bumping the patch version until it lands — re-registering
 *      a tail requires a strictly higher version and yields a new contract_id)
 *   3. create the `secrets` KV map with the contract in its readers/writers ACL
 *   4. seal the provider API key into that map via the control plane
 *   5. persist a deployment.json record (no secrets)
 */
export async function deploy(): Promise<void> {
  const { tenant, did, nodeUrl } = await connect();

  // 1. WASM
  let wasm: Buffer;
  try {
    wasm = await readFile(config.wasmPath);
  } catch {
    throw new Error(
      `WASM not found at ${config.wasmPath}\n` +
        `  Build it first:  npm run build:contract`,
    );
  }
  console.log(`WASM: ${config.wasmPath} (${wasm.length} bytes)`);

  // 2. Register
  let version = config.contractVersion;
  let contractId: number | undefined;
  for (let attempt = 0; attempt < 12 && contractId === undefined; attempt++) {
    try {
      const res = await tenant.contracts.register({ tail: config.contractTail, version, wasm });
      contractId = res.contract_id;
      console.log(`Registered ${tenantScript(did)} v${version} → contract_id=${contractId}`);
    } catch (e: any) {
      if (/not higher than current/i.test(String(e?.message || e))) {
        version = bumpPatch(version);
        continue;
      }
      throw e;
    }
  }
  if (contractId === undefined) {
    throw new Error("Could not register a strictly-higher version after several attempts.");
  }

  // 3. secrets map — ACL MUST include the contract id (kv-governor defaults to deny).
  const mapName = tenant.canonicalName(config.secretMapTail);
  try {
    await tenant.maps.create({
      tail: config.secretMapTail,
      visibility: "private",
      writers: { only: [contractId] },
      readers: { only: [contractId] },
    });
    console.log(`Created map ${mapName} (readers/writers = [${contractId}])`);
  } catch (e: any) {
    if (/alreadyexists/i.test(String(e?.message || e))) {
      console.log(`Map ${mapName} already exists (ok).`);
      console.log(
        `  NOTE: if contract_id changed, re-create the map (or use a fresh tail) so its ACL includes ${contractId}.`,
      );
    } else {
      throw e;
    }
  }

  // 4. Seal the provider key (control-plane write bypasses the writers ACL).
  await tenant.executeControl("map-entry-set", {
    map_name: mapName,
    key: config.secretKey,
    value: config.resendApiKey,
  });
  const isPlaceholder = config.resendApiKey === "re_TEST_PLACEHOLDER_replace_me";
  console.log(
    `Sealed ${config.secretKey} into ${mapName} — ` +
      (isPlaceholder
        ? "PLACEHOLDER (set RESEND_API_KEY in app/.env for a real send)"
        : `real key ****${config.resendApiKey.slice(-4)}`),
  );

  // 5. Persist (no secrets).
  writeDeployment({
    tail: config.contractTail,
    version,
    contractId,
    scriptName: tenantScript(did),
    tenantDid: did,
    node: nodeUrl,
    updatedAt: new Date().toISOString(),
  });
  console.log(`Wrote ${config.deploymentFile}`);
  console.log("Deploy complete → next: npm run grant");
}
