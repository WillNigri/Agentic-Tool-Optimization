/**
 * Skill similarity analysis — detects descriptions/names that are too close
 * and might cause Claude to auto-invoke the wrong skill.
 *
 * Uses tokenized Jaccard similarity + keyword overlap on descriptions.
 */

const STOP_WORDS = new Set([
  "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for", "of",
  "with", "by", "from", "is", "are", "was", "were", "be", "been", "being",
  "have", "has", "had", "do", "does", "did", "will", "would", "could",
  "should", "may", "might", "can", "shall", "this", "that", "these", "those",
  "it", "its", "use", "when", "how", "what", "which", "who", "whom",
  "all", "each", "every", "any", "some", "no", "not", "only", "very",
  "also", "just", "about", "if", "then", "than", "so", "as",
]);

export interface SkillConflict {
  skillA: { id: string; name: string; description: string };
  skillB: { id: string; name: string; description: string };
  similarity: number; // 0-1
  sharedKeywords: string[];
  severity: "high" | "medium" | "low";
  suggestion: string;
}

/** Tokenize and filter a string into meaningful keywords */
function tokenize(text: string): string[] {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9\s-]/g, " ")
    .split(/\s+/)
    .filter((w) => w.length > 2 && !STOP_WORDS.has(w));
}

/** Jaccard similarity between two token sets */
function jaccard(setA: Set<string>, setB: Set<string>): number {
  if (setA.size === 0 && setB.size === 0) return 0;
  let intersection = 0;
  for (const item of setA) {
    if (setB.has(item)) intersection++;
  }
  const union = setA.size + setB.size - intersection;
  return union === 0 ? 0 : intersection / union;
}

/** Extract bigrams for better phrase matching */
function bigrams(tokens: string[]): Set<string> {
  const result = new Set<string>();
  for (let i = 0; i < tokens.length - 1; i++) {
    result.add(`${tokens[i]} ${tokens[i + 1]}`);
  }
  return result;
}

/** Analyze all skills for potential description conflicts */
export function analyzeSkillConflicts(
  skills: { id: string; name: string; description: string }[]
): SkillConflict[] {
  const conflicts: SkillConflict[] = [];

  // Pre-compute tokens for each skill
  const skillTokens = skills.map((s) => {
    const descTokens = tokenize(s.description);
    const nameTokens = tokenize(s.name.replace(/-/g, " "));
    const allTokens = [...nameTokens, ...descTokens];
    return {
      skill: s,
      tokens: allTokens,
      tokenSet: new Set(allTokens),
      bigramSet: bigrams(allTokens),
    };
  });

  // Compare every pair
  for (let i = 0; i < skillTokens.length; i++) {
    for (let j = i + 1; j < skillTokens.length; j++) {
      const a = skillTokens[i];
      const b = skillTokens[j];

      // Jaccard on unigrams
      const unigramSim = jaccard(a.tokenSet, b.tokenSet);

      // Jaccard on bigrams (phrase-level similarity)
      const bigramSim = jaccard(a.bigramSet, b.bigramSet);

      // Combined score: weight unigrams more but bigrams catch phrase overlap
      const combined = unigramSim * 0.6 + bigramSim * 0.4;

      // Find shared keywords (excluding very common ones)
      const sharedKeywords: string[] = [];
      for (const token of a.tokenSet) {
        if (b.tokenSet.has(token)) {
          sharedKeywords.push(token);
        }
      }

      if (combined < 0.15 || sharedKeywords.length < 2) continue;

      let severity: SkillConflict["severity"];
      let suggestion: string;

      if (combined >= 0.5) {
        severity = "high";
        suggestion = `These skills have very similar descriptions. Claude may invoke the wrong one. Differentiate by making descriptions more specific about WHEN each should be used, or merge them.`;
      } else if (combined >= 0.3) {
        severity = "medium";
        suggestion = `Moderate overlap detected. Consider adding unique trigger phrases to each description so Claude can distinguish them clearly.`;
      } else {
        severity = "low";
        suggestion = `Minor keyword overlap. Usually fine, but watch for misfires if both skills activate in similar contexts.`;
      }

      conflicts.push({
        skillA: a.skill,
        skillB: b.skill,
        similarity: Math.round(combined * 100),
        sharedKeywords,
        severity,
        suggestion,
      });
    }
  }

  // Sort by severity (high first) then by similarity
  const severityOrder = { high: 0, medium: 1, low: 2 };
  conflicts.sort((a, b) => {
    const sDiff = severityOrder[a.severity] - severityOrder[b.severity];
    if (sDiff !== 0) return sDiff;
    return b.similarity - a.similarity;
  });

  return conflicts;
}

/** Get conflicts involving a specific skill */
export function getConflictsForSkill(
  skillId: string,
  conflicts: SkillConflict[]
): SkillConflict[] {
  return conflicts.filter(
    (c) => c.skillA.id === skillId || c.skillB.id === skillId
  );
}
