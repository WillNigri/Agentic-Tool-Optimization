# MCP Author Guide — Enforcing per-user data access with ATO

This guide is for **MCP authors** building a server that's going
to be called from inside ATO. It shows you how to enforce
row/column-level access control at the data layer using the
identity ATO passes through on every call.

ATO is the identity provider. Your MCP server is the source of
truth for what each user can read. The two layers are intentionally
separated: ATO doesn't know your schema; you don't have to write
auth code.

---

## Quick start (60 seconds)

When ATO calls your MCP, it passes the identity of the user who
initiated the call via **two channels** (use whichever fits your
stack):

### Channel 1: Spawn environment variables (stdio MCPs)

ATO sets these env vars on your MCP process when it spawns:

```
ATO_USER_ID=alice@acme.com
ATO_WORKSPACE_ID=workspace_xyz
ATO_ROOM_ID=room_customer_research
ATO_SESSION_ID=01H8…
ATO_AGENT_SLUG=customer-insight-agent
```

Any of them can be missing — read with a safe default:

```python
# Python MCP server
import os
user_id = os.environ.get("ATO_USER_ID") or "anonymous"
workspace = os.environ.get("ATO_WORKSPACE_ID")
```

```typescript
// TypeScript MCP server
const userId = process.env.ATO_USER_ID ?? "anonymous";
const workspace = process.env.ATO_WORKSPACE_ID;
```

### Channel 2: JSON-RPC `params._meta` (per-call)

Every `tools/call` request ATO sends includes a `_meta` object on
the params:

```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "search_customers",
    "arguments": { "q": "churned last 30 days" },
    "_meta": {
      "ato.user_id": "alice@acme.com",
      "ato.workspace_id": "workspace_xyz",
      "ato.room_id": "room_customer_research",
      "ato.session_id": "01H8…",
      "ato.agent_slug": "customer-insight-agent"
    }
  }
}
```

The `_meta` field is reserved by the MCP spec for client-supplied
metadata, so this convention doesn't conflict with anything else
you're doing.

---

## Why two channels?

- **Env vars** are right when your access decision depends on the
  user *at process startup* (e.g. you'll spawn a fresh process per
  user and want each child's identity baked in).
- **`_meta`** is right when the same MCP process serves multiple
  users (long-running process, HTTP MCP, etc) and you need to
  authorize on a per-call basis.

Most MCP authors use `_meta` — it's the more flexible default.
Use env vars if you also want to log identity at process startup
or pre-configure caches per user.

---

## Common ACL patterns

### Row-level Postgres ACLs

If your customer data lives in Postgres, use [Row-Level Security
(RLS)](https://www.postgresql.org/docs/current/ddl-rowsecurity.html)
and pass `ATO_USER_ID` to your session before the query:

```python
# In your MCP's tools/call handler
async def call_tool(name: str, arguments: dict, meta: dict):
    user_id = meta.get("ato.user_id") or "anonymous"

    async with pool.acquire() as conn:
        # Postgres RLS will filter rows where user_id != current_user
        await conn.execute(
            "SET LOCAL app.current_user_id = $1", user_id
        )
        rows = await conn.fetch(
            "SELECT * FROM customer_conversations WHERE q ILIKE $1",
            arguments["q"],
        )
        return rows
```

Then in Postgres:

```sql
CREATE POLICY tenant_isolation ON customer_conversations
  FOR SELECT
  USING (
    -- e.g. only show rows the user owns OR is a member of the team
    -- that owns them
    owner_id = current_setting('app.current_user_id')::text
    OR EXISTS (
      SELECT 1 FROM team_members tm
      WHERE tm.user_id = current_setting('app.current_user_id')::text
        AND tm.team_id = customer_conversations.team_id
    )
  );

ALTER TABLE customer_conversations ENABLE ROW LEVEL SECURITY;
```

### Document-scoping in a vector DB

If you're using pgvector / Pinecone / Weaviate, filter at query
time on a `visible_to` array:

```python
# Pinecone
results = index.query(
    vector=embedding,
    top_k=10,
    filter={
        "visible_to": {"$in": [user_id, f"workspace:{workspace}"]}
    }
)
```

### S3 prefix filtering

If documents live in S3, scope by prefix:

```python
prefix = f"workspace/{workspace}/users/{user_id}/"
objects = s3.list_objects_v2(Bucket="...", Prefix=prefix)
```

### Slack-channel-style room scoping

If your MCP serves a shared team workspace AND specific rooms:

```python
room_id = meta.get("ato.room_id")
if room_id:
    # Restrict to this room's accessible documents only
    where_clause = "WHERE room_id = $1"
    params = [room_id]
else:
    # No room context → fall back to workspace-level access
    where_clause = "WHERE workspace_id = $1"
    params = [workspace_id]
```

---

## What to do when identity is missing

ATO sends identity on every call BUT some installs may not have a
configured user (anonymous OSS install, dev sandbox, etc). Decide
explicitly what your MCP does in that case:

| Strategy | When to use |
|----------|-------------|
| **Deny by default** | High-sensitivity data (financial, PII, regulated) |
| **Read-only public scope** | Public catalogs, docs sites |
| **Allow with audit warning** | Internal-only MCPs where the OS user is implicitly trusted |
| **Refuse with clear error** | When the MCP cannot operate without identity |

Sample "deny by default" pattern:

```python
user_id = meta.get("ato.user_id") or os.environ.get("ATO_USER_ID")
if not user_id or user_id == "anonymous":
    return {
        "isError": True,
        "content": [{
            "type": "text",
            "text": "This MCP requires an authenticated ATO user. "
                    "Set ATO_USER_ID or sign in to a workspace."
        }]
    }
```

---

## Audit logging from your side

ATO already logs every tool call from its side (audit log per
dispatch). For defense-in-depth, log identity on your MCP side too:

```python
logger.info(
    "tool_call",
    extra={
        "user_id": user_id,
        "workspace": workspace,
        "room": room_id,
        "tool": name,
        "argv": json.dumps(arguments)[:500],
    }
)
```

This gives you a forensics trail independent of ATO's own audit log
— useful when investigating "who saw what" incidents.

---

## Testing locally

For a single-user local install, set the identity manually in your
shell before running ATO:

```bash
export ATO_USER_ID="dev@example.com"
export ATO_WORKSPACE_ID="dev-workspace"
ato dispatch claude --agent my-agent "test query"
```

Your MCP will see those values via both env vars and `_meta` on
every call.

---

## What ATO does NOT enforce

ATO passes identity through; it does not validate it.

- **ATO does not check that `ATO_USER_ID` is a real user.** OSS
  Phase 1 trusts the local install. Cloud Team tier replaces this
  with an authenticated workspace session that ATO validates
  server-side.
- **ATO does not enforce that your MCP honors the identity.** If
  your MCP ignores `_meta` and returns everyone's data to
  everyone, ATO can't catch that. The whole *point* of this
  guide is to make it easy for you to honor it.
- **ATO does not encrypt the identity in transit.** stdio is
  process-local; HTTP MCPs should be TLS as a separate concern.

---

## When to upgrade to the Team tier

If you're running into any of these, the OSS env-var flow stops
scaling:

- Multiple humans need their own identity on the same Mac (today
  they'd share `$ATO_USER_ID`)
- You need a central record of which workspace member did what,
  not just per-Mac audit
- You need to revoke a user's access centrally and have ATO stop
  passing them through
- You need SSO / SAML / federated identity

That's what the Team tier provides — same wire format, different
source of truth (authenticated workspace session vs env var).

---

## Reference

| Identity field | Env var | _meta key | When populated |
|---------------|---------|-----------|----------------|
| User | `ATO_USER_ID` | `ato.user_id` | Always (defaults to `$USER`) |
| Workspace | `ATO_WORKSPACE_ID` | `ato.workspace_id` | When in a workspace (Team tier) |
| Room | `ATO_ROOM_ID` | `ato.room_id` | When dispatch is scoped to a room |
| Session | `ATO_SESSION_ID` | `ato.session_id` | When linked to an ato session |
| Agent | `ATO_AGENT_SLUG` | `ato.agent_slug` | When dispatched via `--agent` |

All fields are STRING-typed; missing fields are *omitted* from
both channels, not nulled.

---

## Questions / contributions

Open an issue on the ATO repo or DM us — we'll add patterns for
your stack to this guide. The goal is that every common ACL
pattern has a working snippet here.
