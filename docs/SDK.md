# @ato/sdk — Developer Documentation

Auto-capture LLM traces for [ATO](https://agentictool.ai). Works with Anthropic, OpenAI, and any LLM provider.

---

## Installation

```bash
npm install @ato/sdk
```

The SDK has **optional peer dependencies** — install only what you use:

```bash
# If using Anthropic
npm install @anthropic-ai/sdk

# If using OpenAI
npm install openai
```

---

## Quick Start

### 1. Initialize

```typescript
import { init } from '@ato/sdk';

init({
  apiKey: 'ato_your_api_key',  // Get from app.agentictool.ai/settings
});
```

### 2. Wrap Your LLM Client

#### Anthropic

```typescript
import Anthropic from '@anthropic-ai/sdk';
import { wrapAnthropic } from '@ato/sdk/anthropic';

const anthropic = wrapAnthropic(new Anthropic());

// Use normally — traces are captured automatically
const message = await anthropic.messages.create({
  model: 'claude-sonnet-4-6',
  max_tokens: 1024,
  messages: [{ role: 'user', content: 'Explain quantum computing in one paragraph.' }],
});
```

#### OpenAI

```typescript
import OpenAI from 'openai';
import { wrapOpenAI } from '@ato/sdk/openai';

const openai = wrapOpenAI(new OpenAI());

// Use normally — traces are captured automatically
const completion = await openai.chat.completions.create({
  model: 'gpt-4o',
  messages: [{ role: 'user', content: 'Explain quantum computing in one paragraph.' }],
});
```

#### Streaming

Both wrappers support streaming automatically:

```typescript
// Anthropic streaming
const stream = anthropic.messages.stream({
  model: 'claude-sonnet-4-6',
  max_tokens: 1024,
  messages: [{ role: 'user', content: 'Write a haiku.' }],
});

// OpenAI streaming
const stream = await openai.chat.completions.create({
  model: 'gpt-4o',
  messages: [{ role: 'user', content: 'Write a haiku.' }],
  stream: true,
  stream_options: { include_usage: true },  // Required for token counting
});
```

---

## Configuration

```typescript
import { init } from '@ato/sdk';

const client = init({
  // Required for cloud sync
  apiKey: 'ato_your_api_key',

  // Cloud endpoint (default: https://api.agentictool.ai)
  endpoint: 'https://api.agentictool.ai',

  // Batching (default: true, improves performance)
  batching: true,
  flushInterval: 5000,   // Flush every 5 seconds
  maxBatchSize: 50,       // Or when 50 traces queued

  // Debugging
  debug: true,            // Log every trace to console

  // Context
  sessionId: 'session-123',  // Group traces by session
  userId: 'user-456',        // Attribute to user

  // Tags applied to all traces
  defaultTags: ['production', 'api-server'],

  // Metadata applied to all traces
  defaultMetadata: {
    service: 'my-api',
    version: '1.2.0',
  },

  // Local only (no cloud sync)
  localOnly: false,
});
```

### Environment Variables

You can also configure via environment:

```bash
ATO_API_KEY=ato_your_api_key
ATO_ENDPOINT=https://api.agentictool.ai
```

---

## What Gets Captured

Every LLM call automatically records:

| Field | Description | Example |
|-------|-------------|---------|
| `id` | Unique trace ID | `ato_1712345678_abc123` |
| `provider` | LLM provider | `anthropic`, `openai` |
| `model` | Model used | `claude-sonnet-4-6` |
| `inputTokens` | Input tokens | `1,234` |
| `outputTokens` | Output tokens | `567` |
| `cachedTokens` | Cached input tokens | `890` |
| `costUsd` | Calculated cost in USD | `$0.0234` |
| `durationMs` | Response time | `1,234ms` |
| `status` | Success or error | `success` |
| `error` | Error message (if failed) | `Rate limit exceeded` |
| `metadata` | Provider-specific data | `{ stopReason, temperature, toolUse }` |

---

## Cost Calculation

Built-in pricing for **60+ models** across 7 providers:

```typescript
import { calculateCost } from '@ato/sdk';

// Returns cost in USD
calculateCost('claude-sonnet-4-6', 1000, 500);           // $0.0105
calculateCost('gpt-4o', 1000, 500);                      // $0.0075
calculateCost('claude-opus-4-6', 1000, 500, 200);        // With cached tokens
calculateCost('gemini-2.5-pro', 1000, 500);              // Google
calculateCost('mistral-large', 1000, 500);               // Mistral
```

### Supported Providers & Models

| Provider | Models |
|----------|--------|
| **Anthropic** | Claude Opus 4.6, Sonnet 4.6, Haiku 4.5, 3.5 Sonnet/Haiku, 3 Opus/Sonnet/Haiku |
| **OpenAI** | GPT-4o, 4o-mini, 4-turbo, 4, 3.5-turbo, o1, o1-mini, o1-pro, o3, o3-mini, o4-mini, 4.1, 4.1-mini, 4.1-nano |
| **Google** | Gemini 2.5 Pro/Flash, 2.0 Flash, 1.5 Pro/Flash |
| **Mistral** | Large, Small, Codestral |
| **Groq** | Llama 3.3 70B, 3.1 8B, Mixtral 8x7B |
| **Cohere** | Command R+, Command R |

Pricing is updated regularly. If a model isn't found, cost returns `0`.

---

## Manual Traces

For custom providers or non-standard integrations:

```typescript
import { capture, generateTraceId } from '@ato/sdk';
import { calculateCost } from '@ato/sdk';

capture({
  id: generateTraceId(),
  provider: 'together',
  model: 'meta-llama/Llama-3.3-70B',
  inputTokens: 500,
  outputTokens: 200,
  cachedTokens: 0,
  totalTokens: 700,
  costUsd: 0.0004,
  durationMs: 890,
  status: 'success',
  metadata: { region: 'us-east-1' },
  timestamp: new Date().toISOString(),
  sessionId: 'my-session',
  tags: ['inference', 'production'],
});
```

---

## Lifecycle

```typescript
import { init, flush, shutdown } from '@ato/sdk';

// Initialize at app startup
init({ apiKey: 'ato_key' });

// ... your app runs, traces are auto-captured ...

// Force flush (e.g., before a Lambda exits)
await flush();

// Full shutdown (flush + cleanup timers)
await shutdown();
```

### Serverless / Lambda

For serverless environments, flush before the function exits:

```typescript
import { init, flush } from '@ato/sdk';
import { wrapAnthropic } from '@ato/sdk/anthropic';
import Anthropic from '@anthropic-ai/sdk';

init({ apiKey: process.env.ATO_API_KEY });
const anthropic = wrapAnthropic(new Anthropic());

export async function handler(event) {
  const msg = await anthropic.messages.create({ ... });

  // Flush before Lambda freezes
  await flush();

  return { statusCode: 200, body: msg.content[0].text };
}
```

---

## Cloud Dashboard

Traces sent to ATO Cloud are visible at **[app.agentictool.ai](https://app.agentictool.ai)**:

- **Cost Dashboard** — per-model, per-provider, daily timeline
- **Team Cost** — aggregate spend across team members
- **Agent Monitor** — real-time sessions, token rates, alerts
- **Audit Log** — full history of all actions

---

## API Reference

### Exports from `@ato/sdk`

| Export | Type | Description |
|--------|------|-------------|
| `init(config)` | Function | Initialize the global client |
| `capture(trace)` | Function | Record a trace manually |
| `flush()` | Function | Flush pending traces |
| `shutdown()` | Function | Flush + cleanup |
| `getClient()` | Function | Get the global client instance |
| `generateTraceId()` | Function | Generate a unique trace ID |
| `calculateCost(model, input, output, cached?)` | Function | Calculate cost in USD |
| `MODEL_PRICING` | Object | Full pricing table |
| `AtoClient` | Class | Client class for advanced usage |

### Exports from `@ato/sdk/anthropic`

| Export | Type | Description |
|--------|------|-------------|
| `wrapAnthropic(client)` | Function | Wrap an Anthropic client |

### Exports from `@ato/sdk/openai`

| Export | Type | Description |
|--------|------|-------------|
| `wrapOpenAI(client)` | Function | Wrap an OpenAI client |

---

## Troubleshooting

### Traces not appearing in dashboard

1. Check your API key is correct
2. Enable debug mode: `init({ debug: true })` — traces will log to console
3. Call `await flush()` to force-send immediately
4. Check network — can your server reach `api.agentictool.ai`?

### Cost shows $0.00

The model name must match the pricing table. Check `MODEL_PRICING` for exact names. If using a custom/fine-tuned model, cost won't be calculated automatically — set `costUsd` manually in your trace.

### Streaming tokens show 0

For OpenAI streaming, you must pass `stream_options: { include_usage: true }` to get token counts in the stream. Without this, OpenAI doesn't report usage for streaming responses.

---

## License

MIT — [github.com/WillNigri/Agentic-Tool-Optimization](https://github.com/WillNigri/Agentic-Tool-Optimization)
