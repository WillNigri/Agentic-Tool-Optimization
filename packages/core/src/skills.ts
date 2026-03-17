// ============================================================
// Skill Parsing Utilities
// Pure functions, no I/O.
// ============================================================

/**
 * Parse YAML-like frontmatter from a markdown skill file.
 * Expects content in the form:
 *
 * ```
 * ---
 * name: My Skill
 * description: Does things
 * key: value
 * ---
 * # Actual content...
 * ```
 */
export function parseSkillFrontmatter(rawContent: string): {
  name: string;
  description: string;
  metadata: Record<string, unknown>;
  body: string;
} {
  const match = rawContent.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/);
  if (!match) {
    return { name: '', description: '', metadata: {}, body: rawContent };
  }

  const frontmatterBlock = match[1]!;
  const body = match[2] ?? '';
  const metadata: Record<string, unknown> = {};

  for (const line of frontmatterBlock.split('\n')) {
    const colonIndex = line.indexOf(':');
    if (colonIndex === -1) continue;
    const key = line.slice(0, colonIndex).trim();
    const value = line.slice(colonIndex + 1).trim();
    if (key) {
      metadata[key] = value;
    }
  }

  return {
    name: (metadata['name'] as string) ?? '',
    description: (metadata['description'] as string) ?? '',
    metadata,
    body,
  };
}

/**
 * Detect keyword overlap between skills using Jaccard similarity.
 * Returns pairs of skills with overlap > 30%.
 */
export function detectSkillConflicts(
  skills: Array<{ name: string; content: string }>,
): Array<{ skillA: string; skillB: string; overlapPercentage: number }> {
  const conflicts: Array<{ skillA: string; skillB: string; overlapPercentage: number }> = [];

  // Extract keyword sets for each skill (words with 4+ characters)
  const keywordSets = skills.map((skill) => {
    const words = skill.content
      .toLowerCase()
      .replace(/[^a-z0-9\s]/g, ' ')
      .split(/\s+/)
      .filter((w) => w.length >= 4);
    return new Set(words);
  });

  for (let i = 0; i < skills.length; i++) {
    for (let j = i + 1; j < skills.length; j++) {
      const setA = keywordSets[i]!;
      const setB = keywordSets[j]!;

      // Compute Jaccard similarity: |A ∩ B| / |A ∪ B|
      let intersection = 0;
      for (const word of setA) {
        if (setB.has(word)) intersection++;
      }

      const union = setA.size + setB.size - intersection;
      if (union === 0) continue;

      const overlapPercentage = Math.round((intersection / union) * 100);
      if (overlapPercentage > 30) {
        conflicts.push({
          skillA: skills[i]!.name,
          skillB: skills[j]!.name,
          overlapPercentage,
        });
      }
    }
  }

  return conflicts;
}

/**
 * Compute a simple hash of the given content.
 * Converts to char codes, multiply/add, returns hex string.
 * No crypto dependency needed.
 */
export function computeContentHash(content: string): string {
  let h1 = 0x811c9dc5;
  let h2 = 0x01000193;
  for (let i = 0; i < content.length; i++) {
    const c = content.charCodeAt(i);
    h1 = Math.imul(h1 ^ c, 0x01000193);
    h2 = Math.imul(h2 ^ c, 0x811c9dc5);
  }
  const hex1 = (h1 >>> 0).toString(16).padStart(8, '0');
  const hex2 = (h2 >>> 0).toString(16).padStart(8, '0');
  return hex1 + hex2;
}
