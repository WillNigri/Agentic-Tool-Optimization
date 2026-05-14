## Summary
<!-- One paragraph: what changed, why. Link the issue / discussion. -->

## Multi-LLM review
<!--
For non-trivial PRs (anything touching dispatch paths, auth, billing,
security, schema, or > ~50 lines), paste the `ato review` output here.
Generate with:

  ato review --against origin/main \
    --reviewer "@security-specialist" --reviewer "@perf-reviewer" \
    --reviewer claude --reviewer minimax \
    --out review.md --human

For trivial PRs (typo fix, comment cleanup, single-line doc tweak),
write "n/a — trivial" instead. Reviewers may still ask for one.

The transcript goes inside the <details> block so it doesn't dominate
the PR description; reviewers expand it to read.
-->

<details>
<summary>Reviewer transcript</summary>

```markdown
PASTE THE CONTENTS OF review.md HERE
```

</details>

### Tier 1 fixes applied
<!--
For each finding the review flagged HIGH or MEDIUM, record:
- Finding N (reviewer X): APPLIED / DEFERRED / FALSE-POSITIVE — one-line rationale

Linking the actual fix commit is even better.
-->

## Testing
<!-- How did you verify this works? `cargo test`? Manual GUI run? -->

## Screenshots / GIFs
<!-- UI changes: a before/after screenshot or a vhs GIF beats a paragraph. -->
