/**
 * proLocalRun.ts — Wave 3 local-run router for PRO features on E2E shares.
 *
 * For E2E-encrypted shares the cloud hosted-judge / hosted-diagnose endpoints
 * receive 422 ENCRYPTED_TEAM (they can't read the ciphertext). This module
 * catches that and falls through to the PRO runner sidecar binary (Wave 1 stub
 * in apps/desktop/src/lib/e2e/proRunner.ts). The binary lands separately.
 *
 * Flow:
 *   1. Load + decrypt all events for the resource (backfill + decryptor).
 *   2. Invoke proRunner.invokeLocally(feature, input) → may throw ProRunnerNotInstalledError.
 *   3. On success: encrypt + post the verdict as a new event via appendTeamEventEncrypted.
 *   4. If anon telemetry is opted in for this share, enqueue metrics locally.
 */

import {
  backfillTeamEvents,
  appendTeamEventEncrypted,
  type TeamEvent,
  type SharedResourceKind,
} from "@/lib/cloud-api";
import { invokeLocally, type ProFeature } from "@/lib/e2e/proRunner";
export { ProRunnerNotInstalledError } from "@/lib/e2e/proRunner";
import { invoke } from "@tauri-apps/api/core";

/** Marker returned on the TeamEvent when a PRO run produces a verdict. */
export interface ProRunVerdictEvent extends TeamEvent {
  event_kind: "judge_verdict" | "diagnose_proposal" | "analytics_summary";
}

/**
 * Run a PRO feature locally on a decrypted E2E resource.
 *
 * Callers obtain teamKey + signerEd25519PrivateKey + signerKeyId from the
 * E2E keychain / team-key cache. The decryptor is injected so this function
 * doesn't take a hard dep on the UI's closure.
 *
 * May throw:
 *   - `ProRunnerNotInstalledError` — sidecar not present; caller shows CTA.
 *   - Any network error from appendTeamEventEncrypted.
 */
export async function runProFeatureLocally(
  feature: ProFeature,
  teamId: string,
  kind: SharedResourceKind,
  resourceId: string,
  teamKey: Uint8Array,
  signerEd25519PrivateKey: Uint8Array,
  signerKeyId: string,
  decryptor: (raw: TeamEvent) => Promise<TeamEvent>,
): Promise<ProRunVerdictEvent> {
  // Load all decrypted events (cap 1000 per synthesis Q4).
  const rawEvents = await backfillTeamEvents(teamId, kind, resourceId, 0, 1000);
  const decryptedEvents = await Promise.all(rawEvents.map(decryptor));

  // Map feature → verdict event_kind.
  const verdictKind = featureToEventKind(feature);

  // Invoke the PRO runner sidecar.
  const verdictPayload = await invokeLocally(feature, {
    teamId,
    kind,
    resourceId,
    events: decryptedEvents,
  });

  // Encrypt + post the verdict as a new team event.
  const result = await appendTeamEventEncrypted(
    teamId,
    kind,
    resourceId,
    verdictKind,
    verdictPayload,
    "desktop",
    teamKey,
    signerEd25519PrivateKey,
    signerKeyId,
  );

  // Enqueue anonymized telemetry if opted in.
  const optIn = await getShareTelemetryPref(teamId, kind, resourceId);
  if (optIn) {
    const metrics = extractSafeMetrics(feature, verdictPayload);
    if (metrics) {
      await enqueueTelemetry(metrics);
    }
  }

  // Return a synthetic TeamEvent for the verdict.
  return {
    seq_num: result.seq_num,
    event_kind: verdictKind,
    payload_json: verdictPayload,
    ciphertext_b64: null,
    nonce_b64: null,
    signature_b64: null,
    signer_key_id: signerKeyId,
    initiator_user_id: null,
    initiator_runtime: null,
    initiator_agent_slug: null,
    surface: "desktop",
    created_at: result.created_at,
  } as ProRunVerdictEvent;
}

// ── Internal helpers ──────────────────────────────────────────────────────────

function featureToEventKind(
  feature: ProFeature,
): "judge_verdict" | "diagnose_proposal" | "analytics_summary" {
  switch (feature) {
    case "hosted-judge": return "judge_verdict";
    case "hosted-diagnose": return "diagnose_proposal";
    case "analytics-aggregate": return "analytics_summary";
  }
}

/**
 * Extract the safe-set metrics from a PRO verdict for anon telemetry.
 * Returns null if no safe metrics are extractable.
 */
function extractSafeMetrics(
  feature: ProFeature,
  verdict: unknown,
): Record<string, unknown> | null {
  if (typeof verdict !== "object" || verdict === null) return null;
  const v = verdict as Record<string, unknown>;
  // Only numeric aggregate scores — never raw content.
  if (feature === "hosted-judge" && typeof v.score === "number") {
    return { feature, score: v.score };
  }
  if (feature === "hosted-diagnose" && typeof v.confidence === "number") {
    return { feature, confidence: v.confidence };
  }
  if (feature === "analytics-aggregate" && typeof v.event_count === "number") {
    return { feature, event_count: v.event_count };
  }
  return null;
}

async function enqueueTelemetry(entry: Record<string, unknown>): Promise<void> {
  try {
    await invoke("anon_telemetry_enqueue", { entryJson: JSON.stringify(entry) });
  } catch {
    // Best-effort; don't let telemetry errors affect the main flow.
  }
}

async function getShareTelemetryPref(
  teamId: string,
  kind: SharedResourceKind,
  resourceId: string,
): Promise<boolean> {
  try {
    return await invoke<boolean>("get_share_telemetry_pref", {
      teamId,
      resourceKind: kind,
      resourceId,
    });
  } catch {
    return false;
  }
}
