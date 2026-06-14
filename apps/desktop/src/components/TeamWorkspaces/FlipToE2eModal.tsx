/**
 * FlipToE2eModal — Wave 3 warning modal for switching a share to E2E encryption.
 *
 * Shows:
 *   - What E2E means for the user (privacy + PRO features run locally).
 *   - An opt-in toggle for anonymized aggregate metrics (default off).
 *
 * On confirm:
 *   1. Generate a fresh Team Key.
 *   2. Fetch every member's active X25519 pubkey.
 *   3. Seal the Team Key for each member (sealTeamKey).
 *   4. POST /api/teams/:tid/key-rotations → creates a fresh generation.
 *   5. POST /api/teams/:tid/<kind>/:rid/encryption-mode with mode=e2e.
 *   6. Persist the anon-telemetry opt-in preference locally via Tauri.
 *
 * If step 5 returns 409 HAS_PLAINTEXT_HISTORY, shows an error explaining
 * that the resource has existing plaintext events and instructs the user to
 * create a new fresh share and enable E2E from the start.
 */

import { useState } from "react";
import { Loader2, ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";

import { cn } from "@/lib/utils";
import {
  getTeamMemberE2eKeys,
  setShareEncryptionMode,
  pushKeyRotation,
  CloudApiError,
  type SharedResourceKind,
} from "@/lib/cloud-api";
import { generateTeamKey, sealTeamKey, toBase64 } from "@/lib/e2e/crypto";

interface FlipToE2eModalProps {
  teamId: string;
  kind: SharedResourceKind;
  resourceId: string;
  onClose: () => void;
  /** Called after a successful flip so the parent can refresh. */
  onSuccess: () => void;
}

export default function FlipToE2eModal({
  teamId,
  kind,
  resourceId,
  onClose,
  onSuccess,
}: FlipToE2eModalProps) {
  const { t } = useTranslation();
  const [contributeMetrics, setContributeMetrics] = useState(false);
  const [busy, setBusy] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const handleConfirm = async () => {
    setBusy(true);
    setErrorMsg(null);

    try {
      // Step 1: fresh Team Key.
      const teamKey = await generateTeamKey();

      // Step 2: fetch all member pubkeys.
      const memberKeys = await getTeamMemberE2eKeys(teamId);

      // Step 3: seal the Team Key to each member.
      const envelopes = await Promise.all(
        memberKeys.map(async (m) => {
          const { fromBase64 } = await import("@/lib/e2e/crypto");
          const recipientPubkey = fromBase64(m.x25519_pubkey);
          const sealed = await sealTeamKey(teamKey, recipientPubkey);
          return {
            member_user_id: m.member_user_id,
            key_id: m.key_id,
            sealed_key_b64: toBase64(sealed),
          };
        }),
      );

      // Step 4: POST key-rotations to create a fresh Team Key generation.
      await pushKeyRotation(teamId, envelopes);

      // Step 5: flip the share's encryption_mode to 'e2e'.
      await setShareEncryptionMode(teamId, kind, resourceId, "e2e");

      // Step 6: persist telemetry opt-in preference locally.
      await invoke("set_share_telemetry_pref", {
        teamId,
        resourceKind: kind,
        resourceId,
        optIn: contributeMetrics,
      });

      onSuccess();
    } catch (err) {
      if (err instanceof CloudApiError && err.code === "HAS_PLAINTEXT_HISTORY") {
        setErrorMsg(
          t("flipE2e.error.hasPlaintextHistory", {
            defaultValue:
              "This share already has plaintext events. To use E2E encryption, " +
              "create a new share and enable encryption from the start.",
          }),
        );
      } else {
        setErrorMsg(
          err instanceof Error ? err.message : t("flipE2e.error.generic", { defaultValue: "An error occurred." }),
        );
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    // Backdrop
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="relative w-full max-w-md rounded-xl border border-cs-border bg-cs-card p-6 shadow-2xl">
        {/* Header */}
        <div className="flex items-center gap-3 mb-4">
          <span className="flex items-center justify-center w-9 h-9 rounded-full bg-cs-accent/10 border border-cs-accent/30">
            <ShieldCheck size={18} className="text-cs-accent" />
          </span>
          <h2 className="text-base font-semibold text-cs-text">
            {t("flipE2e.title", { defaultValue: "End-to-End Encryption" })}
          </h2>
        </div>

        {/* Body */}
        <div className="space-y-3 text-sm text-cs-text mb-5">
          <p className="text-cs-muted">
            {t("flipE2e.intro", {
              defaultValue: "When you turn this on:",
            })}
          </p>

          <ul className="space-y-1.5">
            <li className="flex items-start gap-2">
              <span className="mt-0.5 text-emerald-400">✓</span>
              <span>
                {t("flipE2e.bullet.privacy", {
                  defaultValue: "Your content stays private — ATO can't read it.",
                })}
              </span>
            </li>
            <li className="flex items-start gap-2">
              <span className="mt-0.5 text-emerald-400">✓</span>
              <span>
                {t("flipE2e.bullet.proLocal", {
                  defaultValue:
                    "PRO features (judge, diagnose) still work — they run locally on your machine using your tokens.",
                })}
              </span>
            </li>
            <li className="flex items-start gap-2">
              {/* Checkbox item */}
              <label className="flex items-start gap-2 cursor-pointer select-none w-full">
                <input
                  type="checkbox"
                  checked={contributeMetrics}
                  onChange={(e) => setContributeMetrics(e.target.checked)}
                  className="mt-0.5 h-4 w-4 rounded border border-cs-border bg-cs-bg accent-cs-accent cursor-pointer"
                />
                <span className="text-cs-muted">
                  {t("flipE2e.bullet.telemetry", {
                    defaultValue:
                      "Optionally share anonymized aggregate metrics to help improve ATO.",
                  })}{" "}
                  <span className="text-[11px] text-cs-muted/70">
                    ({t("flipE2e.defaultOff", { defaultValue: "default off; opt-in" })})
                  </span>
                </span>
              </label>
            </li>
          </ul>

          {errorMsg && (
            <div className="rounded-md border border-cs-danger/40 bg-cs-danger/10 px-3 py-2 text-xs text-cs-text">
              {errorMsg}
            </div>
          )}
        </div>

        {/* Actions */}
        <div className="flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            className="rounded-md border border-cs-border px-3 py-1.5 text-sm text-cs-muted hover:bg-cs-border/30 transition-colors disabled:opacity-50"
          >
            {t("common.cancel", { defaultValue: "Cancel" })}
          </button>
          <button
            type="button"
            onClick={() => void handleConfirm()}
            disabled={busy}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-md bg-cs-accent px-3 py-1.5 text-sm font-medium text-cs-bg",
              "hover:bg-cs-accent/90 transition-colors disabled:opacity-50",
            )}
          >
            {busy && <Loader2 size={14} className="animate-spin" />}
            {t("flipE2e.confirm", { defaultValue: "Enable encryption" })}
          </button>
        </div>
      </div>
    </div>
  );
}
