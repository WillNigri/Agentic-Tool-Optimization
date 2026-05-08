import { useState, useMemo, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Loader2,
  AlertCircle,
  FileText,
  Trash2,
  Search,
  Sparkles,
  ClipboardPaste,
} from "lucide-react";
import type { Agent } from "@/lib/agents";
import {
  ingestKnowledgeText,
  listAgentKnowledge,
  deleteKnowledgeSource,
  retrieveKnowledge,
  groupBySource,
  totalTokens,
  type KnowledgeChunk,
  type RetrievalHit,
} from "@/lib/agentKnowledge";
import { cn } from "@/lib/utils";

// v2.0.0 Wave 2 — Knowledge tab.
//
// Visible only on external agents. Three sections, top to bottom:
//   1. Add — paste text or drop a .md / .txt file → embeds via OpenAI →
//      chunks land in local SQLite.
//   2. Sources — ingested files grouped by source, with chunk count +
//      token total. Delete-by-source lets the user remove a whole file.
//   3. Test retrieval — type a query, see top-K matching chunks with
//      cosine score so the user can sanity-check their RAG before deploy.
//
// PDFs / URLs are not supported in alpha — only .md / .txt / paste. The
// chunker is char-window with paragraph-boundary backoff; embeddings are
// OpenAI text-embedding-3-small (1536 dims).

interface Props {
  agent: Agent;
}

export default function KnowledgeTab({ agent }: Props) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const isExternal = agent.kind === "external";

  const [pasted, setPasted] = useState("");
  const [pasteName, setPasteName] = useState("");
  const [ingestState, setIngestState] = useState<"idle" | "embedding" | "error">("idle");
  const [ingestError, setIngestError] = useState<string | null>(null);

  const [retrieveQuery, setRetrieveQuery] = useState("");
  const [retrieveState, setRetrieveState] = useState<"idle" | "running" | "error">("idle");
  const [retrieveError, setRetrieveError] = useState<string | null>(null);
  const [retrieveResults, setRetrieveResults] = useState<RetrievalHit[]>([]);

  const { data: chunks = [], isLoading } = useQuery({
    queryKey: ["agent-knowledge", agent.id],
    queryFn: () => listAgentKnowledge(agent.id, false),
    enabled: isExternal,
    staleTime: 5_000,
  });

  const grouped = useMemo(() => groupBySource(chunks), [chunks]);
  const totalToks = totalTokens(chunks);

  const onIngest = useCallback(
    async (source: string, content: string) => {
      if (!content.trim()) return;
      setIngestState("embedding");
      setIngestError(null);
      try {
        await ingestKnowledgeText({
          agentId: agent.id,
          source: source.trim() || "pasted-text.md",
          content,
        });
        setPasted("");
        setPasteName("");
        setIngestState("idle");
        queryClient.invalidateQueries({ queryKey: ["agent-knowledge", agent.id] });
      } catch (err) {
        setIngestState("error");
        setIngestError(err instanceof Error ? err.message : String(err));
      }
    },
    [agent.id, queryClient],
  );

  const onDrop = useCallback(
    async (e: React.DragEvent<HTMLDivElement>) => {
      e.preventDefault();
      e.stopPropagation();
      const files = Array.from(e.dataTransfer.files);
      for (const f of files) {
        // Reject non-text files up front so the user gets a clear message
        // instead of a binary blob being embedded as garbage tokens.
        const lc = f.name.toLowerCase();
        const ok = lc.endsWith(".md") || lc.endsWith(".txt") || lc.endsWith(".markdown");
        if (!ok) {
          setIngestState("error");
          setIngestError(
            `Skipped ${f.name} — only .md / .txt / .markdown supported in v2.0 alpha.`,
          );
          continue;
        }
        const text = await f.text();
        await onIngest(f.name, text);
      }
    },
    [onIngest],
  );

  const onDeleteSource = useCallback(
    async (source: string) => {
      if (!confirm(`Delete all chunks from "${source}"?`)) return;
      await deleteKnowledgeSource(agent.id, source);
      queryClient.invalidateQueries({ queryKey: ["agent-knowledge", agent.id] });
    },
    [agent.id, queryClient],
  );

  const onRetrieve = useCallback(async () => {
    if (!retrieveQuery.trim()) return;
    setRetrieveState("running");
    setRetrieveError(null);
    try {
      const hits = await retrieveKnowledge({
        agentId: agent.id,
        query: retrieveQuery.trim(),
        k: 5,
      });
      setRetrieveResults(hits);
      setRetrieveState("idle");
    } catch (err) {
      setRetrieveState("error");
      setRetrieveError(err instanceof Error ? err.message : String(err));
    }
  }, [agent.id, retrieveQuery]);

  if (!isExternal) {
    return (
      <div className="rounded-lg border border-cs-border bg-cs-bg/40 p-6 text-sm text-cs-muted">
        {t(
          "agentDetail.knowledge.internalOnly",
          "Knowledge is only available for external agents. Flip this agent to External in the header to unlock RAG-style retrieval.",
        )}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Cross-reference to Context tab for live data sources. Beatriz
          feedback (2026-05-08): "Knowledge" sounded like the catch-all
          for everything the agent knows, but it only handles static text.
          Per-customer DB queries / CRM lookups / live API calls belong in
          Context (hooks) — which fires on every turn, not at deploy time. */}
      <section className="rounded-lg border border-cs-border bg-cs-bg-raised/40 p-3 text-[11px] text-cs-muted">
        <div className="text-xs font-semibold text-cs-text mb-1">
          {t("agentDetail.knowledge.staticVsLive", "Static knowledge vs live data")}
        </div>
        <p>
          {t(
            "agentDetail.knowledge.staticVsLiveBody",
            "Knowledge ingests STATIC text (FAQs, policies, docs) — embedded once and baked into the deploy bundle. For LIVE data per customer (DB queries, CRM lookups, API calls, MCP tools), use the Context tab — hooks fire per-turn with optional keyword / LLM gating.",
          )}
        </p>
      </section>

      {/* ── Add ────────────────────────────────────────────────────────── */}
      <section className="space-y-3">
        <SectionHeader
          title={t("agentDetail.knowledge.addTitle", "Add knowledge")}
          hint={t(
            "agentDetail.knowledge.addHint",
            "Drop a .md / .txt file or paste text. Each chunk is embedded via your active embedding provider (OpenAI / Voyage / Gemini / Cohere / Ollama) and stored locally. Bundle generation inlines the chunks into the deployed agent.",
          )}
        />

        <div
          onDrop={onDrop}
          onDragOver={(e) => e.preventDefault()}
          onDragEnter={(e) => e.preventDefault()}
          className={cn(
            "rounded-lg border-2 border-dashed border-cs-border bg-cs-bg/40 p-6 text-center text-xs text-cs-muted",
            ingestState === "embedding" && "opacity-50",
          )}
        >
          {ingestState === "embedding" ? (
            <span className="inline-flex items-center gap-2">
              <Loader2 size={14} className="animate-spin" />
              {t("agentDetail.knowledge.embedding", "Chunking + embedding…")}
            </span>
          ) : (
            <>
              <FileText size={16} className="mx-auto mb-2 text-cs-muted" />
              {t(
                "agentDetail.knowledge.dropHint",
                "Drag a .md or .txt file here, or paste below.",
              )}
            </>
          )}
        </div>

        <div className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_auto]">
          <input
            type="text"
            value={pasteName}
            onChange={(e) => setPasteName(e.target.value)}
            placeholder={t("agentDetail.knowledge.sourceName", "Source name (e.g. policies.md)")}
            className="rounded-md border border-cs-border bg-cs-bg px-3 py-2 text-xs text-cs-text font-mono"
          />
          <button
            type="button"
            onClick={() => onIngest(pasteName || "pasted-text.md", pasted)}
            disabled={!pasted.trim() || ingestState === "embedding"}
            className="inline-flex items-center justify-center gap-1.5 rounded-md bg-cs-accent px-3 py-2 text-xs font-medium text-cs-bg hover:bg-cs-accent-hover disabled:opacity-50"
          >
            <ClipboardPaste size={11} />
            {t("agentDetail.knowledge.ingest", "Ingest paste")}
          </button>
        </div>

        <textarea
          rows={5}
          value={pasted}
          onChange={(e) => setPasted(e.target.value)}
          placeholder={t(
            "agentDetail.knowledge.pastePlaceholder",
            "Paste your FAQ / policy / docs content here.",
          )}
          className="w-full rounded-md border border-cs-border bg-cs-bg px-3 py-2 text-xs text-cs-text font-mono"
        />

        {ingestError && (
          <div className="flex items-start gap-2 rounded-md border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
            <AlertCircle size={12} className="text-cs-danger shrink-0 mt-0.5" />
            <span>{ingestError}</span>
          </div>
        )}
      </section>

      {/* ── Sources ────────────────────────────────────────────────────── */}
      <section>
        <SectionHeader
          title={t("agentDetail.knowledge.sourcesTitle", "Sources")}
          hint={
            chunks.length === 0
              ? t(
                  "agentDetail.knowledge.empty",
                  "No knowledge yet. Add a source above to enable RAG.",
                )
              : t("agentDetail.knowledge.sourcesSummary", "{{c}} chunks · ~{{t}} tokens total", {
                  c: chunks.length,
                  t: totalToks.toLocaleString(),
                })
          }
        />

        {isLoading && (
          <div className="text-xs text-cs-muted">
            <Loader2 size={12} className="inline animate-spin mr-1" />
            Loading…
          </div>
        )}

        {!isLoading && chunks.length > 0 && (
          <ul className="space-y-2">
            {Array.from(grouped.entries()).map(([source, sourceChunks]) => (
              <li
                key={source}
                className="flex items-center justify-between rounded-md border border-cs-border bg-cs-bg/40 px-3 py-2"
              >
                <div className="min-w-0">
                  <code className="font-mono text-xs text-cs-text truncate block">{source}</code>
                  <span className="text-[11px] text-cs-muted">
                    {sourceChunks.length} chunks ·{" "}
                    {sourceChunks.reduce((s, c) => s + c.tokens, 0).toLocaleString()} tokens
                  </span>
                </div>
                <button
                  type="button"
                  onClick={() => onDeleteSource(source)}
                  className="text-cs-muted hover:text-cs-danger"
                  aria-label={`Delete ${source}`}
                >
                  <Trash2 size={12} />
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      {/* ── Test retrieval ─────────────────────────────────────────────── */}
      {chunks.length > 0 && (
        <section className="space-y-3">
          <SectionHeader
            title={t("agentDetail.knowledge.testTitle", "Test retrieval")}
            hint={t(
              "agentDetail.knowledge.testHint",
              "Type the kind of question your customers will ask. ATO embeds it and shows the top-5 chunks the agent would see — so you can sanity-check your knowledge before deploy.",
            )}
          />

          <div className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_auto]">
            <input
              type="text"
              value={retrieveQuery}
              onChange={(e) => setRetrieveQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && onRetrieve()}
              placeholder={t(
                "agentDetail.knowledge.queryPlaceholder",
                "How do I cancel my subscription?",
              )}
              className="rounded-md border border-cs-border bg-cs-bg px-3 py-2 text-xs text-cs-text"
            />
            <button
              type="button"
              onClick={onRetrieve}
              disabled={!retrieveQuery.trim() || retrieveState === "running"}
              className="inline-flex items-center justify-center gap-1.5 rounded-md border border-cs-border bg-cs-bg px-3 py-2 text-xs font-medium text-cs-text hover:bg-cs-border/50 disabled:opacity-50"
            >
              {retrieveState === "running" ? (
                <Loader2 size={11} className="animate-spin" />
              ) : (
                <Search size={11} />
              )}
              {t("agentDetail.knowledge.search", "Retrieve")}
            </button>
          </div>

          {retrieveError && (
            <div className="flex items-start gap-2 rounded-md border border-cs-danger/40 bg-cs-danger/10 p-3 text-xs text-cs-text">
              <AlertCircle size={12} className="text-cs-danger shrink-0 mt-0.5" />
              <span>{retrieveError}</span>
            </div>
          )}

          {retrieveResults.length > 0 && (
            <ul className="space-y-2">
              {retrieveResults.map((hit) => (
                <RetrievalHitCard key={hit.chunk.id} hit={hit} />
              ))}
            </ul>
          )}
        </section>
      )}
    </div>
  );
}

function RetrievalHitCard({ hit }: { hit: RetrievalHit }) {
  const score = (hit.score * 100).toFixed(1);
  return (
    <li className="rounded-md border border-cs-border bg-cs-bg/40 p-3">
      <div className="mb-1 flex items-center justify-between gap-2 text-[11px]">
        <code className="font-mono text-cs-muted truncate">
          {hit.chunk.source} · #{hit.chunk.position + 1}
        </code>
        <span className="inline-flex items-center gap-1 rounded bg-cs-accent/10 px-1.5 py-0.5 font-medium text-cs-accent">
          <Sparkles size={9} />
          {score}%
        </span>
      </div>
      <pre className="text-[11px] text-cs-text font-mono whitespace-pre-wrap line-clamp-6">
        {hit.chunk.content}
      </pre>
    </li>
  );
}

function SectionHeader({ title, hint }: { title: string; hint?: string }) {
  return (
    <div>
      <div className="text-[11px] font-semibold uppercase tracking-wide text-cs-muted">{title}</div>
      {hint && <p className="mt-1 text-[11px] text-cs-muted">{hint}</p>}
    </div>
  );
}
