# Contributing to ATO

Thanks for considering a contribution. ATO is MIT-licensed and developed
in the open. Pull requests, issues, and discussions are all welcome.

## Required for every non-trivial PR: an LLM review

We dogfood our own multi-LLM review on every meaningful commit before it
lands on `main`. PRs against this repo follow the same standard.

**What counts as non-trivial**: anything that touches dispatch paths,
auth/credentials, billing/credits, security-sensitive code, database
schema, the WebSocket protocol, the mesh pairing flow, or roughly 50+
changed lines.

**What's trivial**: typo fixes, comment-only changes, a single-line doc
tweak, a renamed variable. Maintainers may still ask for a review.

### How to run it

```bash
# From the repo root, with ato installed (`brew install --cask ato`) or
# the release binary built (`cargo build -p ato --release`):

./scripts/llm-review.sh              # diff vs origin/main, default reviewers
./scripts/llm-review.sh --consensus  # add a consensus round that surfaces disagreements
```

The script prints a ready-to-paste `<details>` block. Pipe through
`pbcopy` (macOS) or `xclip` (Linux) and paste into the PR description
where the template says so.

### What the review should produce

The PR description should include:
1. The review transcript inside a `<details>` block (so the description
   stays scannable for reviewers).
2. A short "Tier 1 fixes applied" section listing every HIGH or MEDIUM
   finding with `APPLIED / DEFERRED / FALSE-POSITIVE — rationale`.

Deferring a HIGH finding is allowed but should explain why (e.g., "fixes
a pre-existing issue outside this PR's scope — tracked in #NNN").

### Why we do this

ATO's product pitch is *"war room for humans and LLMs."* The review
isn't theater — it's the same workflow we ship to users, on our own
diffs, against our own codebase. If our review process can't catch a
real bug on our own code, that's a signal we should improve the
process before pitching it to anyone else.

## Local development

See [README.md](README.md) for setup. Key commands:

- `npm run dev:desktop` — Tauri desktop in dev mode
- `cargo build -p ato --release` — Build the CLI binary
- `cargo test` — Run the test suite (both apps + packages)
- `npx tsc --noEmit` — TypeScript type-check the frontend

## Commit style

- Use [conventional commits](https://www.conventionalcommits.org/) where
  it fits (`feat:`, `fix:`, `chore:`, `refactor:`).
- Keep the first line under 72 chars when you can.
- Co-authored-by trailers are encouraged when an LLM helped:
  `Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>`.

## Reporting issues

Open a GitHub issue with:
- What you ran / clicked
- What you expected
- What actually happened (stdout, stderr, screenshots)
- Your platform (macOS / Linux / Windows), ATO version (`ato --version`),
  and the runtime CLIs involved.
