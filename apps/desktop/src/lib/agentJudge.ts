import { useAuthStore } from "@/hooks/useAuth";
import { tierMeetsRequirement } from "@/lib/tier";

// v1.4.0 Wave 4.5 — Pro hosted LLM-as-judge.
//
// The Rust evaluator runs heuristic kinds locally (contains, length-range,
// tool-called, etc.). LLM-judge needs a real model call so it runs in the
// cloud, gated by requireTier('pro') on the server. This wrapper handles
// auth + tier check on the client and surfaces a typed result.

const CLOUD_API_URL =
  (import.meta.env.VITE_CLOUD_API_URL as string | undefined) || "https://api.ato.dev";

export interface JudgeInput {
  judgePrompt: string;
  userMessage: string;
  agentResponse: string;
  agentGoal?: string;
}

export interface JudgeResult {
  verdict: "pass" | "fail" | "partial" | "unknown";
  score: number;
  reason: string;
  inputTokens: number;
  outputTokens: number;
  model: string;
}

export class JudgeError extends Error {
  constructor(message: string, public code: string) {
    super(message);
    this.name = "JudgeError";
  }
}

export async function runLlmJudge(input: JudgeInput): Promise<JudgeResult> {
  const { isCloudUser, accessToken, tier } = useAuthStore.getState();

  if (!isCloudUser || !accessToken) {
    throw new JudgeError(
      "LLM-as-judge requires a cloud account. Sign in to enable.",
      "NOT_SIGNED_IN"
    );
  }
  if (!tierMeetsRequirement(tier, "pro")) {
    throw new JudgeError(
      "LLM-as-judge is a Pro feature. Upgrade to enable.",
      "TIER_TOO_LOW"
    );
  }

  const response = await fetch(`${CLOUD_API_URL}/api/agent-evaluators/judge`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${accessToken}`,
    },
    body: JSON.stringify(input),
  });

  if (!response.ok) {
    let code = "JUDGE_FAILED";
    let message = `Judge request failed (${response.status})`;
    try {
      const body = await response.json();
      if (body?.error?.code) code = body.error.code;
      if (body?.error?.message) message = body.error.message;
    } catch {
      // Ignore — keep the default message.
    }
    throw new JudgeError(message, code);
  }

  const body = await response.json();
  if (!body?.success || !body?.data) {
    throw new JudgeError("Malformed judge response", "BAD_RESPONSE");
  }
  return body.data as JudgeResult;
}
