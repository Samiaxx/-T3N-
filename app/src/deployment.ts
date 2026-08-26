import { readFileSync, writeFileSync } from "node:fs";
import { config } from "./env";

/** On-disk record of what was registered. Contains NO secrets — safe to commit. */
export interface Deployment {
  tail: string;
  version: string;
  contractId: number;
  scriptName: string;
  tenantDid: string;
  node: string;
  updatedAt: string;
}

export function readDeployment(): Deployment | null {
  try {
    return JSON.parse(readFileSync(config.deploymentFile, "utf8")) as Deployment;
  } catch {
    return null;
  }
}

export function writeDeployment(d: Deployment): void {
  writeFileSync(config.deploymentFile, JSON.stringify(d, null, 2) + "\n");
}
