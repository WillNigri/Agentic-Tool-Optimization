import { invoke } from "@tauri-apps/api/core";

// v2.0.0 Wave 2 — Frontend wrappers for the local knowledge commands.
//
// Embeddings are 1536 floats (OpenAI text-embedding-3-small). Listing
// chunks defaults to omitting the embedding so the UI can render hundreds
// of rows fast; deploy-bundle generation passes `includeEmbedding: true`.

export interface KnowledgeChunk {
  id: string;
  agentId: string;
  source: string;
  content: string;
  tokens: number;
  position: number;
  embedModel: string;
  createdAt: string;
  /** Only populated when caller passes includeEmbedding: true. */
  embedding?: number[] | null;
}

export interface RetrievalHit {
  chunk: KnowledgeChunk;
  score: number;
}

export async function ingestKnowledgeText(input: {
  agentId: string;
  source: string;
  content: string;
}): Promise<KnowledgeChunk[]> {
  return invoke<KnowledgeChunk[]>("ingest_knowledge_text", {
    agentId: input.agentId,
    source: input.source,
    content: input.content,
  });
}

export async function listAgentKnowledge(
  agentId: string,
  includeEmbedding = false,
): Promise<KnowledgeChunk[]> {
  return invoke<KnowledgeChunk[]>("list_agent_knowledge", {
    agentId,
    includeEmbedding,
  });
}

export async function deleteKnowledgeChunk(chunkId: string): Promise<void> {
  return invoke("delete_knowledge_chunk", { chunkId });
}

export async function deleteKnowledgeSource(
  agentId: string,
  source: string,
): Promise<number> {
  return invoke<number>("delete_knowledge_source", { agentId, source });
}

export async function retrieveKnowledge(input: {
  agentId: string;
  query: string;
  k?: number;
}): Promise<RetrievalHit[]> {
  return invoke<RetrievalHit[]>("retrieve_knowledge", {
    agentId: input.agentId,
    query: input.query,
    k: input.k ?? 5,
  });
}

/** Group flat chunks by source filename for the UI list view. */
export function groupBySource(chunks: KnowledgeChunk[]): Map<string, KnowledgeChunk[]> {
  const m = new Map<string, KnowledgeChunk[]>();
  for (const c of chunks) {
    const arr = m.get(c.source) ?? [];
    arr.push(c);
    m.set(c.source, arr);
  }
  return m;
}

/** Total approximate tokens across all chunks — surfaced in the UI so
 *  users see how much of their context budget the knowledge will consume
 *  on each call. */
export function totalTokens(chunks: KnowledgeChunk[]): number {
  return chunks.reduce((sum, c) => sum + c.tokens, 0);
}
