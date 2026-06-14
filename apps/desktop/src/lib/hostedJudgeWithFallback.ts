/**
 * hostedJudgeWithFallback.ts — Wave 3 deliverable F.
 *
 * Wraps the cloud hosted-judge / hosted-diagnose call and catches
 * 422 ENCRYPTED_TEAM, falling through to proLocalRun.runProFeatureLocally.
 *
 * Usage (wherever the UI calls hosted-judge for a share):
 *
 *   import { runJudgeWithFallback } from '@/lib/hostedJudgeWithFallback';
 *
 *   const verdict = await runJudgeWithFallback({
 *     teamId, kind, resourceId,
 *     feature: 'hosted-judge',
 *     // For plaintext shares these can be null (cloud path taken):
 *     teamKey, signerEd25519PrivateKey, signerKeyId, decryptor,
 *   });
 */

import { CloudApiError, type SharedResourceKind, type TeamEvent } from "@/lib/cloud-api";
import { runProFeatureLocally, ProRunnerNotInstalledError } from "@/lib/proLocalRun";
import type { DecryptorFn } from "@/lib/teamEventStream";
import type { ProFeature } from "@/lib/e2e/proRunner";

export interface JudgeWithFallbackParams {
  feature: ProFeature;
  teamId: string;
  kind: SharedResourceKind;
  resourceId: string;
  /**
   * Cloud-side call. Implementations should POST to the relevant cloud
   * endpoint and return the verdict event.
   * For plaintext shares this is the only path taken.
   */
  cloudCall: () => Promise<TeamEvent>;
  /** Required for E2E fallback only. */
  teamKey?: Uint8Array;
  signerEd25519PrivateKey?: Uint8Array;
  signerKeyId?: string;
  decryptor?: DecryptorFn;
}

export type JudgeResult =
  | { ok: true; event: TeamEvent }
  | { ok: false; proRunnerNotInstalled: true }
  | { ok: false; proRunnerNotInstalled: false; error: string };

/**
 * Run a cloud PRO feature with a local-run fallback for E2E shares.
 *
 * Returns a discriminated union so callers can handle each case without
 * catching exceptions.
 */
export async function runJudgeWithFallback(
  params: JudgeWithFallbackParams,
): Promise<JudgeResult> {
  const {
    feature,
    teamId,
    kind,
    resourceId,
    cloudCall,
    teamKey,
    signerEd25519PrivateKey,
    signerKeyId,
    decryptor,
  } = params;

  // Try cloud first.
  try {
    const event = await cloudCall();
    return { ok: true, event };
  } catch (cloudErr) {
    // 422 ENCRYPTED_TEAM → the share is E2E; fall through to local runner.
    const isEncryptedTeam =
      cloudErr instanceof CloudApiError && cloudErr.code === "ENCRYPTED_TEAM";

    if (!isEncryptedTeam) {
      // Some other cloud error — surface it.
      return {
        ok: false,
        proRunnerNotInstalled: false,
        error: cloudErr instanceof Error ? cloudErr.message : String(cloudErr),
      };
    }
  }

  // Local-run fallback for ENCRYPTED_TEAM.
  if (!teamKey || !signerEd25519PrivateKey || !signerKeyId || !decryptor) {
    return {
      ok: false,
      proRunnerNotInstalled: false,
      error:
        "E2E share requires teamKey, signerEd25519PrivateKey, signerKeyId, and decryptor for local PRO run.",
    };
  }

  try {
    const event = await runProFeatureLocally(
      feature,
      teamId,
      kind,
      resourceId,
      teamKey,
      signerEd25519PrivateKey,
      signerKeyId,
      decryptor,
    );
    return { ok: true, event };
  } catch (localErr) {
    if (localErr instanceof ProRunnerNotInstalledError) {
      return { ok: false, proRunnerNotInstalled: true };
    }
    return {
      ok: false,
      proRunnerNotInstalled: false,
      error: localErr instanceof Error ? localErr.message : String(localErr),
    };
  }
}

/**
 * React-ready error component factory for the "PRO Runner not installed" case.
 * Returns the CTA message string so callers can render it in their own UI.
 */
export function proRunnerNotInstalledMessage(): string {
  return "PRO Runner not installed. Download from Settings → Account → PRO Runner.";
}
