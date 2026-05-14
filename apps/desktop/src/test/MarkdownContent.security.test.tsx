// Verifies the markdown renderer doesn't allow LLM-controlled output
// to produce dangerous links / raw HTML. react-markdown v10+ filters
// `javascript:` / `data:` URI schemes by default and refuses raw HTML
// in the source unless rehype-raw is added. We don't use rehype-raw.
//
// These tests guard against a future bump to a version that changes
// the defaults, or an unintentional addition of rehype-raw.
//
// Audit V1 — pre-launch markdown render verification.

import { render } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import MarkdownContent from "@/components/MarkdownContent";

describe("MarkdownContent security defaults", () => {
  it("strips javascript: URI from link href", () => {
    const { container } = render(
      <MarkdownContent content="[click me](javascript:alert(1))" />
    );
    const anchor = container.querySelector("a");
    // react-markdown either drops the href entirely or replaces it
    // with a safe value. Either way, the resulting href must not
    // start with "javascript:".
    const href = anchor?.getAttribute("href") ?? "";
    expect(href.toLowerCase()).not.toMatch(/^javascript:/);
  });

  it("strips data:text/html URI from link href", () => {
    const { container } = render(
      <MarkdownContent content="[xss](data:text/html,<script>alert(1)</script>)" />
    );
    const anchor = container.querySelector("a");
    const href = anchor?.getAttribute("href") ?? "";
    expect(href.toLowerCase()).not.toMatch(/^data:/);
  });

  it("does NOT render raw HTML from markdown source", () => {
    // Without rehype-raw, any `<script>` / `<iframe>` / inline event
    // handler in the markdown source comes through as visible text,
    // not as a DOM element. This test fails if rehype-raw is ever
    // added (which would let an LLM reply inject real DOM).
    const { container } = render(
      <MarkdownContent
        content={`Hello <script>window.__pwned = 1</script> world<img src=x onerror="alert(1)" />`}
      />
    );
    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("img[onerror]")).toBeNull();
    // The "<script>" bytes should still appear as escaped text — no
    // execution, just rendering. We don't assert the exact text
    // because the parser may smudge it; we just assert no script tag.
  });

  it("does NOT execute javascript via image onerror", () => {
    // Same as above but with a different XSS vector.
    const { container } = render(
      <MarkdownContent content={`<img src=x onerror=alert(1)>`} />
    );
    const imgs = container.querySelectorAll("img[onerror]");
    expect(imgs.length).toBe(0);
  });
});
