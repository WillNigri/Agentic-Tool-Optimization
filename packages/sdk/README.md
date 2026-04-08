# @agentic-tool-optimization/sdk

Auto-capture LLM traces for [ATO](https://agentictool.ai). Works with Anthropic, OpenAI, and any LLM provider.

## Install

```bash
npm install @agentic-tool-optimization/sdk
```

## Quick Start

```typescript
import { init } from '@agentic-tool-optimization/sdk';

// Initialize with your ATO API key
init({ apiKey: 'your-ato-api-key' });
```

### Anthropic

```typescript
import Anthropic from '@anthropic-ai/sdk';
import { wrapAnthropic } from '@agentic-tool-optimization/sdk/anthropic';

const client = wrapAnthropic(new Anthropic());

// All calls are now auto-traced — no other changes needed
const msg = await client.messages.create({
  model: 'claude-sonnet-4-6',
  max_tokens: 1024,
  messages: [{ role: 'user', content: 'Hello' }],
});
```

### OpenAI

```typescript
import OpenAI from 'openai';
import { wrapOpenAI } from '@agentic-tool-optimization/sdk/openai';

const client = wrapOpenAI(new OpenAI());

// All calls are now auto-traced
const res = await client.chat.completions.create({
  model: 'gpt-4o',
  messages: [{ role: 'user', content: 'Hello' }],
});
```

## What Gets Captured

Every LLM call automatically records:

- **Model** — which model was used
- **Tokens** — input, output, cached
- **Cost** — calculated from built-in pricing table (60+ models)
- **Duration** — response time in ms
- **Status** — success or error (with error message)
- **Metadata** — temperature, max_tokens, tool usage, stop reason

## Configuration

```typescript
init({
  apiKey: 'your-key',                    // ATO Cloud API key
  endpoint: 'https://api.agentictool.ai', // Custom endpoint
  debug: true,                            // Log traces to console
  batching: true,                         // Batch traces (default)
  flushInterval: 5000,                    // Flush every 5s (default)
  maxBatchSize: 50,                       // Flush at 50 traces (default)
  sessionId: 'my-session',               // Group traces by session
  userId: 'user-123',                    // Attribute traces to user
  defaultTags: ['production'],           // Tags for all traces
  localOnly: true,                       // Don't send to cloud
});
```

## Manual Traces

For custom LLM providers:

```typescript
import { capture, generateTraceId } from '@agentic-tool-optimization/sdk';
import { calculateCost } from '@agentic-tool-optimization/sdk';

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

## Cost Calculation

Built-in pricing for 60+ models:

```typescript
import { calculateCost } from '@agentic-tool-optimization/sdk';

calculateCost('claude-sonnet-4-6', 1000, 500);  // $0.0105
calculateCost('gpt-4o', 1000, 500);             // $0.0075
```

## License

MIT
