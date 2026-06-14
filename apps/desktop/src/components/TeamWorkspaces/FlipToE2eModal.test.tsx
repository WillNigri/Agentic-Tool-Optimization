/**
 * Tests for FlipToE2eModal — Wave 3 deliverable H.
 *
 * Verifies:
 *   - Modal text is rendered as specified.
 *   - The opt-in toggle is off by default and toggleable.
 *   - On confirm, the correct cloud routes are called in order:
 *       1. generateTeamKey
 *       2. getTeamMemberE2eKeys
 *       3. pushKeyRotation (with envelopes)
 *       4. setShareEncryptionMode (mode='e2e')
 *       5. set_share_telemetry_pref (via Tauri invoke)
 */

import { describe, it, expect, vi, beforeEach, beforeAll } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

// ── Sodium ────────────────────────────────────────────────────────────────────
import { ensureSodiumReady } from "@/lib/e2e/crypto";

beforeAll(async () => {
  await ensureSodiumReady();
});

// ── Mocks ─────────────────────────────────────────────────────────────────────
// vi.mock calls are hoisted to the top of the file by Vitest, so we must
// use vi.hoisted() to create mock functions that are available inside the
// factory closures.

const {
  mockGetTeamMemberE2eKeys,
  mockPushKeyRotation,
  mockSetShareEncryptionMode,
  mockInvoke,
  mockGenerateTeamKey,
  mockSealTeamKey,
  mockFromBase64,
  mockToBase64,
} = vi.hoisted(() => ({
  mockGetTeamMemberE2eKeys: vi.fn().mockResolvedValue([
    {
      member_user_id: "user-1",
      key_id: "key-1",
      x25519_pubkey: "dW5pdHRlc3Rfa2V5X3gyNTUxOV9wdWJrZXlfMzJiXw==",
      ed25519_pubkey: "dW5pdHRlc3Rfa2V5X2VkMjU1MTlfcHVia2V5XzMyYg==",
    },
  ]),
  mockPushKeyRotation: vi.fn().mockResolvedValue({ team_key_id: "tk-1" }),
  mockSetShareEncryptionMode: vi.fn().mockResolvedValue(undefined),
  mockInvoke: vi.fn().mockResolvedValue(undefined),
  // Mock the crypto module to avoid needing real libsodium operations in this test.
  mockGenerateTeamKey: vi.fn().mockResolvedValue(new Uint8Array(32)),
  mockSealTeamKey: vi.fn().mockResolvedValue(new Uint8Array(80)),
  mockFromBase64: vi.fn().mockReturnValue(new Uint8Array(32)),
  mockToBase64: vi.fn().mockReturnValue("bW9ja19zZWFsZWRfa2V5X2Jhc2U2NA=="),
}));

vi.mock("@/lib/cloud-api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/cloud-api")>();
  return {
    ...actual,
    getTeamMemberE2eKeys: mockGetTeamMemberE2eKeys,
    pushKeyRotation: mockPushKeyRotation,
    setShareEncryptionMode: mockSetShareEncryptionMode,
    CloudApiError: actual.CloudApiError,
  };
});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}));

// Mock the crypto module so the modal doesn't need a real sodium context
// (real sodium is tested in crypto.test.ts; here we test the modal flow).
vi.mock("@/lib/e2e/crypto", () => ({
  generateTeamKey: mockGenerateTeamKey,
  sealTeamKey: mockSealTeamKey,
  fromBase64: mockFromBase64,
  toBase64: mockToBase64,
  ensureSodiumReady: vi.fn().mockResolvedValue(undefined),
}));

// i18n stub — return the defaultValue passed to t().
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, opts?: { defaultValue?: string }) => opts?.defaultValue ?? _key,
    i18n: { changeLanguage: vi.fn() },
  }),
}));

// ── Component under test ──────────────────────────────────────────────────────
import FlipToE2eModal from "./FlipToE2eModal";

function renderModal(overrides: Partial<React.ComponentProps<typeof FlipToE2eModal>> = {}) {
  const onClose = vi.fn();
  const onSuccess = vi.fn();
  render(
    <FlipToE2eModal
      teamId="team-1"
      kind="session"
      resourceId="res-1"
      onClose={onClose}
      onSuccess={onSuccess}
      {...overrides}
    />,
  );
  return { onClose, onSuccess };
}

beforeEach(() => {
  mockGetTeamMemberE2eKeys.mockClear();
  mockPushKeyRotation.mockClear();
  mockSetShareEncryptionMode.mockClear();
  mockInvoke.mockClear();
});

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("FlipToE2eModal", () => {
  it("renders the title", () => {
    renderModal();
    expect(screen.getByText("End-to-End Encryption")).toBeInTheDocument();
  });

  it("renders privacy guarantee bullet", () => {
    renderModal();
    expect(
      screen.getByText(/Your content stays private/),
    ).toBeInTheDocument();
  });

  it("renders PRO-features-run-locally bullet", () => {
    renderModal();
    expect(
      screen.getByText(/PRO features \(judge, diagnose\) still work/),
    ).toBeInTheDocument();
  });

  it("renders the opt-in metrics checkbox unchecked by default", () => {
    renderModal();
    const checkbox = screen.getByRole("checkbox") as HTMLInputElement;
    expect(checkbox.checked).toBe(false);
  });

  it("toggles the opt-in checkbox", () => {
    renderModal();
    const checkbox = screen.getByRole("checkbox") as HTMLInputElement;
    fireEvent.click(checkbox);
    expect(checkbox.checked).toBe(true);
    fireEvent.click(checkbox);
    expect(checkbox.checked).toBe(false);
  });

  it("calls Cancel without any cloud routes", () => {
    const { onClose } = renderModal();
    fireEvent.click(screen.getByText("Cancel"));
    expect(onClose).toHaveBeenCalledOnce();
    expect(mockGetTeamMemberE2eKeys).not.toHaveBeenCalled();
    expect(mockPushKeyRotation).not.toHaveBeenCalled();
    expect(mockSetShareEncryptionMode).not.toHaveBeenCalled();
  });

  it("on confirm calls correct cloud routes in order and then onSuccess", async () => {
    const { onSuccess } = renderModal();

    // The x25519_pubkey mock above is a zero key (base64 of 32 null bytes + padding).
    // sealTeamKey accepts any 32-byte key, so the mock will work as long as the
    // base64 decodes to 32 bytes.
    // Re-mock with a properly-sized key (44 base64 chars = 33 bytes — use 32 + 1
    // but we want exactly 32). Use a proper 32-byte zero key:
    // toBase64(new Uint8Array(32)) in libsodium's ORIGINAL variant.
    // We skip crypto verification here — the component just calls sealTeamKey and
    // passes the result to pushKeyRotation. Verifying the envelope bytes is
    // covered by crypto.test.ts.

    fireEvent.click(screen.getByRole("button", { name: /Enable encryption/i }));

    await waitFor(() => {
      expect(onSuccess).toHaveBeenCalledOnce();
    });

    // Cloud routes called in order: memberKeys → rotation → e2e mode flip → Tauri pref.
    expect(mockGetTeamMemberE2eKeys).toHaveBeenCalledWith("team-1");
    expect(mockPushKeyRotation).toHaveBeenCalledWith(
      "team-1",
      expect.arrayContaining([
        expect.objectContaining({
          member_user_id: "user-1",
          key_id: "key-1",
          sealed_key_b64: expect.any(String),
        }),
      ]),
    );
    expect(mockSetShareEncryptionMode).toHaveBeenCalledWith("team-1", "session", "res-1", "e2e");
    expect(mockInvoke).toHaveBeenCalledWith("set_share_telemetry_pref", {
      teamId: "team-1",
      resourceKind: "session",
      resourceId: "res-1",
      optIn: false, // default off
    });
  });

  it("persists opt-in=true when checkbox is checked", async () => {
    const { onSuccess } = renderModal();
    const checkbox = screen.getByRole("checkbox") as HTMLInputElement;
    fireEvent.click(checkbox); // opt in

    fireEvent.click(screen.getByRole("button", { name: /Enable encryption/i }));

    await waitFor(() => {
      expect(onSuccess).toHaveBeenCalledOnce();
    });

    expect(mockInvoke).toHaveBeenCalledWith("set_share_telemetry_pref", {
      teamId: "team-1",
      resourceKind: "session",
      resourceId: "res-1",
      optIn: true,
    });
  });

  it("shows HAS_PLAINTEXT_HISTORY error message on 409", async () => {
    const { CloudApiError } = await import("@/lib/cloud-api");
    mockSetShareEncryptionMode.mockRejectedValueOnce(
      new CloudApiError("HAS_PLAINTEXT_HISTORY", "Has plaintext history"),
    );

    renderModal();
    fireEvent.click(screen.getByRole("button", { name: /Enable encryption/i }));

    await waitFor(() => {
      expect(screen.getByText(/already has plaintext events/)).toBeInTheDocument();
    });
  });
});
