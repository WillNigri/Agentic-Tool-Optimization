# Product Hunt launch packet — ato v2.4.6

## Strategic notes before launching

**Timing**: PH ranks by upvotes accumulated in a **24-hour window starting at 00:01 Pacific Time** on launch day. Tuesday–Thursday are the highest-traffic days (Monday is dead, Friday loses weekend support). Submit the listing **24 hours in advance** (PH lets you schedule); the listing goes live at 00:01 PT.

**Concrete recommendation**: schedule for **Tuesday or Wednesday next week** (2026-05-19 or 2026-05-20). Reasons:
1. Brew tap update has time to fully propagate in CDN caches.
2. You can solicit ~10 "hunters" (friends who'll upvote in the first 2 hours) without rushing.
3. The Twitter launch (above) precedes PH by 5–7 days so PH gets traffic from the warm Twitter audience.

**Do NOT** launch HN and PH the same day. They cannibalize attention and you'll have to be on both threads simultaneously. Sequence: Twitter → PH → HN.

---

## Product info (what PH asks for)

### Name
**ATO** (display name: `ATO — Agentic Tool Optimization`)

### Tagline (max 60 chars)

Pick one (in order of strength, my pick first):

1. ✅ **Local war room for humans and LLMs. With receipts.** *(51 chars)*
2. **Multi-LLM debate sessions with tool calls + audit trail** *(56 chars)*
3. **Drop N LLMs into one session. Walk out with receipts.** *(54 chars)*
4. **The maintained LLM Council. With tool calls.** *(45 chars — minimal, but PH likes punch)*

PH frowns on third-party brand names in the tagline. Option 4 borderline OK because "LLM Council" is generic enough; option 1 is the safe play.

### Description (max 260 chars)

```
Drop any of your LLMs (Claude, GPT, Gemini, Grok, MiniMax, +15 more) into one shared session. They see each other's findings, call tools (read_file, grep, git_log) to verify claims in your repo, and you walk out with a signed audit trail. Local-first. MIT.
```

— 258 chars, fits.

### Topics (PH wants 3–4)

- **Artificial Intelligence** (primary)
- **Developer Tools**
- **Open Source**
- **Productivity**

Optional 5th: **Coding Tools** if PH offers it as a sub-topic under Developer Tools.

### Makers list

- Will Nigri (Guilherme) — primary
- (Beatriz Nigri — co-maker / brand owner, if listed as such publicly)

### First comment (from maker, post within 5 min of launch going live)

```
Hey Product Hunt — Will here, maker of ato.

The 30-second pitch: drop multiple LLMs into one shared conversation.
They see each other's prior turns. They call real tools (read_file,
grep, git_log) to verify claims against your repo. You moderate, push
back, walk out with a markdown audit trail to paste into your PR.

Why I built it: a friend sent me Karpathy's llm-council (18.7k stars,
then explicitly abandoned — "Code is ephemeral, ask your LLM to change
it"). The primitive was right. What was missing for daily use:

• Tool calls — LLMs actually read your files / grep your repo /
  check what they're claiming. Karpathy's repo shuffles text.
• Multi-provider — use the Claude Max or Codex CLI subscriptions you
  already have. Not OpenRouter-locked.
• Per-turn audit log — which LLM made which tool call with what args,
  rendered as "verified via N tool calls" vs "prompt-only" badges.
• Specialist agents — define @security-specialist on Gemini,
  @perf-reviewer on MiniMax once, compose into reviews per PR.

Use cases: code review, strategy debates, pre-mortems, architecture
decisions, security audits. Same primitive, different problem.

Best dogfood story: I used ato to decide ato's own positioning. Three
LLMs, five rounds of structured debate, me moderating. They converged
on a hybrid. The headline now on the website was produced by the
product, on camera.

Install:
  brew tap WillNigri/ato
  brew install --cask ato

Or DMG / AppImage / Windows from GitHub releases. Bring your own keys.
Local-first SQLite. MIT.

Honest tradeoffs:
- Higher install friction than a web hack (Tauri desktop + CLI)
- No Chairman LLM (yet) — human moderates by default
- Not a ChatGPT-clone web UI — the CLI + GUI + MCP triad is the wedge

This is the first PH launch, ato has shipped 87 releases in 60 days,
and I'm here for the brutal feedback. Tell me what's confusing, what's
broken, what you'd want next. Thanks!
```

— 1,756 chars. PH allows up to ~5,000 chars in the first comment; this fits with breathing room.

---

## Gallery — what you need to prepare

PH requires a **featured image** (1270×760, the thumbnail) and 4–6 **gallery images** (1270×760 each). Optionally a **video** (max 60s for the gallery slot — the recorded war-room session is ideal here, cut to 30–45s).

### Required: featured image (1270×760)

The image PH shows in feeds, search, and the home page. Make this **the most clickable single image you have**.

**Recommended composition**: the war-room session view in the desktop app — multiple speaker bubbles labeled `@security-specialist · claude`, `@perf-reviewer · minimax`, with tool-call badges visible (`2×tools`, `prompt-only`). Title overlay in the corner: **"Multi-LLM debate sessions, with receipts."**

If you don't want to design — just a clean screenshot of the desktop home screen (the new war-room hero) works as a backup.

### Gallery images (4–6 total)

1. **Hero / brand shot** — same as featured image OR the home screen
2. **The session tab** — the actual recorded session showing each LLM's turn, attribution badges, tool-call audit
3. **The CLI in action** — terminal screenshot of `ato review --reviewer @security-specialist --reviewer @perf-reviewer` mid-run, with tool calls firing in stderr
4. **The audit trail** — close-up of the LogViewer with `verified via 3 tool calls` badge + the per-call list (read_file, grep) expanded
5. **(Optional) Architecture / what-supports-what** — a diagram showing the 20+ runtimes feeding into ato → one session → audit trail. Skip if you don't have time; the other 4 cover the demo.
6. **(Optional) Use cases strip** — the 5 use cases (strategy / pre-mortem / architecture / code review / security) as a labeled grid. Helps the "what is this for" answer.

### Video (optional but high-impact)

Cut the recorded war-room positioning session to **30–45 seconds**. The clip should show:
- Opening frame: the desktop home screen with the war-room hero (5s)
- One round of the actual session — a Gemini turn, then MiniMax pushing back, then your input (15s)
- The tool-call audit badge appearing in the GUI (5s)
- Closing frame: a generated transcript markdown (or the final headline) (5s)

Soundtrack: silent or extremely-soft instrumental. **No voiceover** — voiceover dates fast and creates production overhead.

If you can produce this video, it goes in the gallery AND on the Twitter thread (tweet 1 attachment). One asset, two channels.

---

## Day-of execution checklist

Schedule the listing for 00:01 PT. Day-of:

1. **05:00 PT** — listing goes live. PH bots scan it. Verify nothing is broken (links, install command, screenshots load).
2. **05:01 PT** — post the maker's first comment. Critical for the algorithm; engages your own listing early.
3. **05:15 PT** — DM 5–10 friends with the link and ask for **honest engagement** (read it, click through, comment if it resonates). Do NOT ask for upvotes explicitly — PH detects upvote rings.
4. **06:00 PT** — tweet from your own X account with the PH link. Don't make this the same launch tweet from above; this is a follow-up. Format: *"ATO is on Product Hunt today. The brutal-feedback request: tell me what's confusing. [link]"*
5. **08:00 PT–18:00 PT** — be present on the PH page. Reply to every comment within 30 min. The PH algorithm weights time-to-first-reply and reply depth.
6. **18:00 PT** — quick check on rank. Top 10? Top 20? Just present? Doesn't matter for outcome but worth knowing.
7. **23:59 PT** — listing closes for the day. Don't be on it. Sleep.

## Day-after followup

- **Pin** a "we launched on PH" badge to the README + landing page (PH provides the embed code).
- **Thank** every commenter individually within 24 hours, even the negative ones (especially the negative ones — they often install).
- **Wait at least 14 days** before doing anything HN-related. PH and HN audiences overlap; back-to-back launches read as desperate.

## What NOT to do on PH

- **Don't pay for upvotes / "hunters for hire."** Detected, banned, listing nuked.
- **Don't make every reply a CTA**. Some comments deserve a substantive "you're right, that's a known limitation" answer.
- **Don't relaunch on PH later under a different name** if this one underperforms. PH tracks duplicates and the second listing usually gets shadow-deprioritized.
- **Don't tag PH staff in tweets** asking them to "feature" you. They feature based on launch quality and engagement, not on tags.

## Realistic expectation-setting

At your current scale (28 GitHub stars, ~10 active users), a realistic PH outcome is:

- **Top 20 product of the day** (very achievable with a well-prepared launch)
- **Top 10** (achievable if Karpathy or another high-signal account boosts you on launch day)
- **Top 5** (would require either a coordinated push from a paid hunter or genuinely viral content; unlikely without one of those)

Conversion estimate: a top-20 PH placement typically yields 50–200 signups for a tool like ato, 5–20 GitHub stars, and 1–3 substantive feature requests in the comments. That's the realistic value of the launch — not a hockey-stick growth event, but a 2x of your current install base + qualified feedback from people who chose to engage.
