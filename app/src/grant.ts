import { getContractVersion } from "@terminal3/t3n-sdk";
import { connect } from "./session";
import { config } from "./env";
import { readDeployment } from "./deployment";

/**
 * Self-grant: authorize the caller (here, our own DID) to invoke the contract's
 * functions and reach the email provider host.
 *
 * A grant is scoped three ways at once — which contract, which functions, which
 * external hosts. Without it the contract still runs, but any outbound call is
 * denied with `host/http.egress_denied`. In production this update is signed by
 * the real data owner, authorizing a separate agent DID; the mechanism is
 * identical (see docs/ARCHITECTURE.md).
 */
export async function grant(): Promise<void> {
  const { t3n, did, nodeUrl } = await connect();

  const dep = readDeployment();
  if (!dep) throw new Error("No deployment.json — run `npm run deploy` first.");

  const scriptVersion = await getContractVersion(nodeUrl, dep.scriptName);
  const userContractVersion = await getContractVersion(nodeUrl, "tee:user/contracts");

  console.log(`Self-grant  ${did}`);
  console.log(`  → ${dep.scriptName} v${scriptVersion}`);
  console.log(`  functions:    render-notification, send-notification`);
  console.log(`  allowedHosts: ${config.egressHost}`);

  await t3n.execute({
    contract_id: "tee:user/contracts",
    contract_version: userContractVersion,
    function_name: "agent-auth-update",
    input: {
      agents: [
        {
          agentDid: did, // self-grant — the caller stands in for the data owner
          scripts: [
            {
              scriptName: dep.scriptName,
              versionReq: scriptVersion,
              functions: ["render-notification", "send-notification"],
              allowedHosts: [config.egressHost],
            },
          ],
        },
      ],
    },
  });

  console.log("Grant applied → next: npm run render  (then: npm run send)");
}
