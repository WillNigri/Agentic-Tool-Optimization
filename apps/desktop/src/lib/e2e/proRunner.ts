// Wave 3 stub. The actual PRO runner is a closed-source sidecar binary
// (ato-pro-runner) compiled from ato-cloud, downloaded on first PRO-on-E2E
// use. This file is just the contract — never include PRO logic here.
//
// Why a stub? So Wave 2 (event log + sync) can already call into this
// shape; Wave 3 swaps the body to actually shell out to the sidecar.

export type ProFeature = "hosted-judge" | "hosted-diagnose" | "analytics-aggregate";

export class ProRunnerNotInstalledError extends Error {
  constructor() {
    super("PRO Runner sidecar not installed. This will be implemented in v2.15 Wave 3.");
    this.name = "ProRunnerNotInstalledError";
  }
}

export async function invokeLocally(
  _feature: ProFeature,
  _decryptedInput: unknown,
): Promise<never> {
  throw new ProRunnerNotInstalledError();
}

export async function isProRunnerInstalled(): Promise<boolean> {
  return false;
}
