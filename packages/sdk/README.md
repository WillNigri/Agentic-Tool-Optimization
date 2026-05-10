# @ato-sdk/js

Trace forwarder for AI agents you authored in [ATO](https://agentictool.ai) and deployed outside the desktop app. Ships LLM-call traces back to your ATO Insights dashboard so you can compare runs, detect regressions, and replay across runtimes — alongside the dispatches you ran locally in the desktop app.

## When to use this

- You created an agent in the ATO desktop GUI and deployed it via an ATO bundle (Cloudflare Worker / Vercel Edge / Docker / Node).
- You want that deployed agent's traces to flow back to the same Insights dashboard you use locally.
- You want cross-runtime replay to work for prompts your deployed agent sent.

## When NOT to use this

- You have an existing production stack (your own backend, your own LangChain pipeline, your own LLM calls in user-facing services) and want general-purpose LLM observability across it. **This is not the right tool.** Use [Langfuse](https://langfuse.com), [Helicone](https://www.helicone.ai), [LangSmith](https://smith.langchain.com), [Arize Phoenix](https://phoenix.arize.com), or [Braintrust](https://www.braintrust.dev) — they're built for that job, have multi-language SDK surface, and have team-level adoption already.
- You're trying to instrument an agent that wasn't authored in ATO. The SDK is a forwarder, not a general observability product.

ATO is **complementary** to those tools. Most teams running production agents use one from each camp: a Langfuse / Helicone for end-user conversation logging in production, plus ATO for the developer-workflow side (dispatch, replay across runtimes, regressions, file attribution, cost recommendations). The desktop app is where the dev-workflow value lives; this package is the bridge that keeps deployed agents visible in the same dashboard.

## Install

```bash
npm install @ato-sdk/js
```

## Setup

The ATO bundle generator already wires this up for you when you click "Deploy" on an external agent in the desktop app. You usually don't need to touch the SDK directly. The snippets below are for when you've customized a generated bundle or need to wire it into an alternative deployment shape.

```typescript
import { init } from '@ato-sdk/js';

init({
  apiKey: 'your-ato-trace-key',  // Get this from Settings → Cloud → Embed Key
});
```

### Anthropic (Claude API)

```typescript
import Anthropic from '@anthropic-ai/sdk';
import { wrapAnthropic } from '@ato-sdk/js/anthropic';

const client = wrapAnthropic(new Anthropic());

const msg = await client.messages.create({
  model: 'claude-sonnet-4-6',
  max_tokens: 1024,
  messages: [{ role: 'user', content: 'Hello' }],
});
```

### OpenAI

```typescript
import OpenAI from 'openai';
import { wrapOpenAI } from '@ato-sdk/js/openai';

const client = wrapOpenAI(new OpenAI());

const res = await client.chat.completions.create({
  model: 'gpt-4o',
  messages: [{ role: 'user', content: 'Hello' }],
});
```

## What gets forwarded

Every LLM call from an ATO-deployed agent ships back:

- **Model** — which model the bundle used
- **Tokens** — input, output, cached
- **Cost** — calculated from the built-in pricing table (60+ models)
- **Duration** — response time in ms
- **Status** — success or error (with error message)
- **Metadata** — temperature, max_tokens, tool usage, stop reason
- **Agent attribution** — links back to the agent record in your ATO workspace

These show up in **Insights → External** in the desktop app, with the same drill-down (file attribution, replay, regression detection) as your locally-dispatched agents.

## Configuration

```typescript
init({
  apiKey: 'your-key',                     // ATO trace key (per-account)
  endpoint: 'https://api.agentictool.ai', // Custom endpoint if self-hosting
  debug: true,                            // Log forwards to console
  batching: true,                         // Batch traces (default)
  flushInterval: 5000,                    // Flush every 5s (default)
  maxBatchSize: 50,                       // Flush at 50 traces (default)
  sessionId: 'my-session',                // Group traces by session
  userId: 'user-123',                     // Attribute traces to user
  defaultTags: ['production'],            // Tags for all traces
  localOnly: true,                        // Capture without forwarding to cloud
});
```

## Manual forwarding

For deployed bundles that call non-standard LLM providers:

```typescript
import { capture, generateTraceId, calculateCost } from '@ato-sdk/js';

capture({
  id: generateTraceId(),
  provider: 'custom',
  model: 'my-model',
  inputTokens: 100,
  outputTokens: 50,
  cachedTokens: 0,
  totalTokens: 150,
  costUsd: calculateCost('gpt-4o', 100, 50),
  durationMs: 234,
  status: 'success',
  metadata: {},
  timestamp: new Date().toISOString(),
});
```

## Cost calculation helper

The SDK exports a cost calculator that uses ATO's pricing table — useful if your bundle code needs to compute cost outside the wrap helpers:

```typescript
import { calculateCost } from '@ato-sdk/js';

calculateCost('claude-sonnet-4-6', 1000, 500);  // $0.0105
calculateCost('gpt-4o', 1000, 500);             // $0.0075
```

## License

MIT
