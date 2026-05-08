import type { Agent } from "@/lib/agents";
import type { DeployBundleConfig } from "./shared";

// v2.0.0 Wave 4 — Embed widget code generator.
//
// Emits two files alongside every deploy bundle:
//   - embed.html — a self-contained test page the customer can open locally
//                  in their browser to verify the deployed agent works.
//   - embed.js   — the chat-bubble widget itself. Vanilla JS, no framework.
//                  Customer drops a single `<script>` tag into their site;
//                  the bubble appears, opens on click, talks to the
//                  deployed endpoint via fetch.
//
// The widget reads its config from the script tag's data-* attributes:
//   <script src="embed.js"
//           data-endpoint="https://acme-support.your-team.workers.dev"
//           data-brand="Acme Support"
//           data-color="#00FFB2"
//           data-greeting="Hi! How can I help you today?"></script>
//
// State (conversation history) lives in localStorage keyed by agent slug,
// so customers don't lose their thread on page refresh.

export interface EmbedBundleFiles {
  /** Test page — open locally to verify the agent before going live. */
  "embed.html": string;
  /** The actual widget JS. Customer hosts this from their CDN. */
  "embed.js": string;
}

export function generateEmbedFiles(
  agent: Agent,
  config: DeployBundleConfig,
): EmbedBundleFiles {
  return {
    "embed.html": renderTestPage(agent, config),
    "embed.js": renderEmbedScript(agent),
  };
}

function renderTestPage(agent: Agent, config: DeployBundleConfig): string {
  // Greeting and branding pulled from config — these are placeholders the
  // customer can edit before hosting publicly.
  const brandName = config.brandName || agent.displayName;
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>${escapeHtml(brandName)} — Test Page</title>
  <style>
    body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; padding: 4rem 2rem; max-width: 720px; margin: 0 auto; color: #1a1a1a; line-height: 1.55; }
    h1 { font-size: 1.4rem; margin: 0 0 .5rem; }
    p { color: #555; }
    code { background: #f5f5f7; padding: 2px 6px; border-radius: 4px; font-size: .9em; }
    .note { margin-top: 2rem; padding: 1rem; background: #f5f5f7; border-radius: 8px; font-size: .9rem; }
  </style>
</head>
<body>
  <h1>${escapeHtml(brandName)} — Test Page</h1>
  <p>This is a local test page for the <strong>${escapeHtml(agent.displayName)}</strong> agent. The chat bubble in the bottom-right is live.</p>
  <p>Try sending a message. If the agent doesn't respond, double-check:</p>
  <ul>
    <li>The deployed endpoint URL is correct (see <code>data-endpoint</code> below).</li>
    <li>Your origin (where this HTML is served from) is in the deployed agent's allowlist. For local file:// testing, the agent should also accept <code>null</code> or <code>file://</code> origins.</li>
    <li><code>PROVIDER_API_KEY</code> is set on the deployed bundle.</li>
  </ul>

  <div class="note">
    <strong>Before going live:</strong> replace <code>data-endpoint</code> with your actual deployed URL. Host <code>embed.js</code> on your own CDN or copy this file's <code>&lt;script&gt;</code> tag into your site.
  </div>

  <!-- THE EMBED. Replace data-endpoint with your deployed agent URL. -->
  <script src="./embed.js"
          data-endpoint="https://your-deployed-agent.example.com"
          data-brand=${JSON.stringify(brandName)}
          data-color="#00FFB2"
          data-greeting=${JSON.stringify(`Hi! I'm ${brandName}. How can I help?`)}
          data-agent-slug=${JSON.stringify(agent.slug)}></script>
</body>
</html>
`;
}

function renderEmbedScript(agent: Agent): string {
  // The widget itself. Vanilla JS, IIFE-wrapped to keep globals clean.
  // Storage key is namespaced by agent slug so multiple agents on one
  // site don't collide. The widget POSTs `{message, history}` and
  // expects `{message, latencyMs}` back — matches what our deploy bundle
  // generators emit.
  return `// ATO embed widget v1 — auto-generated for "${agent.displayName}".
// Inject by adding a single <script> tag to your site. See embed.html
// in this bundle for a working example.
(function () {
  "use strict";

  // Read config from the script tag's data-* attributes.
  var script = document.currentScript || (function () {
    var scripts = document.getElementsByTagName("script");
    return scripts[scripts.length - 1];
  })();
  var ENDPOINT = script.getAttribute("data-endpoint");
  if (!ENDPOINT) {
    console.error("[ATO embed] missing data-endpoint attribute on <script> tag");
    return;
  }
  var BRAND = script.getAttribute("data-brand") || "Support";
  var COLOR = script.getAttribute("data-color") || "#00FFB2";
  var GREETING = script.getAttribute("data-greeting") || "Hi! How can I help?";
  var SLUG = script.getAttribute("data-agent-slug") || "default";
  var STORAGE_KEY = "ato-embed:" + SLUG;
  var MAX_HISTORY = 20;

  // ── State ───────────────────────────────────────────────────────────
  var state = {
    open: false,
    sending: false,
    messages: loadHistory(),
  };

  function loadHistory() {
    try {
      var raw = localStorage.getItem(STORAGE_KEY);
      if (!raw) return [];
      var parsed = JSON.parse(raw);
      if (!Array.isArray(parsed)) return [];
      return parsed.slice(-MAX_HISTORY);
    } catch (e) { return []; }
  }
  function saveHistory() {
    try { localStorage.setItem(STORAGE_KEY, JSON.stringify(state.messages.slice(-MAX_HISTORY))); } catch (e) {}
  }

  // ── DOM ─────────────────────────────────────────────────────────────
  var root = document.createElement("div");
  root.setAttribute("data-ato-embed", "");
  root.style.cssText = "position:fixed;bottom:20px;right:20px;z-index:2147483646;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;";

  // The bubble (closed state)
  var bubble = document.createElement("button");
  bubble.type = "button";
  bubble.setAttribute("aria-label", "Open " + BRAND + " chat");
  bubble.style.cssText = "width:60px;height:60px;border-radius:50%;border:none;background:" + COLOR + ";color:#0a0a0f;cursor:pointer;box-shadow:0 4px 16px rgba(0,0,0,.15);display:flex;align-items:center;justify-content:center;transition:transform .15s ease;";
  bubble.innerHTML = '<svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"></path></svg>';
  bubble.addEventListener("mouseenter", function () { bubble.style.transform = "scale(1.05)"; });
  bubble.addEventListener("mouseleave", function () { bubble.style.transform = "scale(1)"; });
  bubble.addEventListener("click", function () { setOpen(true); });

  // The panel (open state)
  var panel = document.createElement("div");
  panel.style.cssText = "display:none;position:absolute;bottom:0;right:0;width:360px;height:520px;max-height:calc(100vh - 40px);background:#fff;border-radius:12px;box-shadow:0 8px 32px rgba(0,0,0,.18);overflow:hidden;flex-direction:column;color:#1a1a1a;";

  var header = document.createElement("div");
  header.style.cssText = "padding:12px 16px;background:" + COLOR + ";color:#0a0a0f;font-weight:600;display:flex;align-items:center;justify-content:space-between;";
  header.innerHTML = '<span></span><button type="button" aria-label="Close" style="background:none;border:none;color:inherit;cursor:pointer;font-size:18px;line-height:1;padding:4px 8px;">×</button>';
  header.firstChild.textContent = BRAND;
  header.lastChild.addEventListener("click", function () { setOpen(false); });

  var messages = document.createElement("div");
  messages.style.cssText = "flex:1;overflow-y:auto;padding:12px;background:#fafafa;";

  var form = document.createElement("form");
  form.style.cssText = "display:flex;gap:8px;padding:12px;border-top:1px solid #eee;background:#fff;";
  var input = document.createElement("input");
  input.type = "text";
  input.placeholder = "Type a message…";
  input.style.cssText = "flex:1;padding:10px 12px;border:1px solid #ddd;border-radius:8px;font:inherit;outline:none;";
  input.addEventListener("focus", function () { input.style.borderColor = COLOR; });
  input.addEventListener("blur", function () { input.style.borderColor = "#ddd"; });
  var send = document.createElement("button");
  send.type = "submit";
  send.textContent = "Send";
  send.style.cssText = "padding:10px 16px;background:" + COLOR + ";color:#0a0a0f;border:none;border-radius:8px;font:inherit;font-weight:600;cursor:pointer;";
  form.appendChild(input);
  form.appendChild(send);
  form.addEventListener("submit", function (e) {
    e.preventDefault();
    var text = input.value.trim();
    if (!text || state.sending) return;
    input.value = "";
    sendMessage(text);
  });

  panel.appendChild(header);
  panel.appendChild(messages);
  panel.appendChild(form);
  root.appendChild(panel);
  root.appendChild(bubble);
  document.body.appendChild(root);

  // ── Render ──────────────────────────────────────────────────────────
  function setOpen(open) {
    state.open = open;
    panel.style.display = open ? "flex" : "none";
    bubble.style.display = open ? "none" : "flex";
    if (open) {
      if (state.messages.length === 0) {
        appendMessage("assistant", GREETING, false);
      }
      setTimeout(function () { input.focus(); }, 50);
    }
  }

  function renderMessage(role, content) {
    var row = document.createElement("div");
    row.style.cssText = "margin-bottom:10px;display:flex;" + (role === "user" ? "justify-content:flex-end;" : "");
    var bubbleEl = document.createElement("div");
    bubbleEl.style.cssText = "max-width:80%;padding:10px 12px;border-radius:12px;font-size:14px;line-height:1.45;white-space:pre-wrap;word-wrap:break-word;" + (role === "user"
      ? "background:" + COLOR + ";color:#0a0a0f;border-bottom-right-radius:4px;"
      : "background:#fff;color:#1a1a1a;border:1px solid #eee;border-bottom-left-radius:4px;");
    bubbleEl.textContent = content;
    row.appendChild(bubbleEl);
    return row;
  }

  function appendMessage(role, content, persist) {
    if (persist !== false) {
      state.messages.push({ role: role, content: content });
      saveHistory();
    }
    messages.appendChild(renderMessage(role, content));
    messages.scrollTop = messages.scrollHeight;
  }

  function appendThinking() {
    var row = document.createElement("div");
    row.setAttribute("data-thinking", "");
    row.style.cssText = "margin-bottom:10px;";
    var b = document.createElement("div");
    b.style.cssText = "display:inline-block;padding:10px 12px;border-radius:12px;background:#fff;border:1px solid #eee;font-size:14px;color:#999;border-bottom-left-radius:4px;";
    b.textContent = "Thinking…";
    row.appendChild(b);
    messages.appendChild(row);
    messages.scrollTop = messages.scrollHeight;
    return row;
  }

  // ── Send ────────────────────────────────────────────────────────────
  function sendMessage(text) {
    state.sending = true;
    send.disabled = true;
    input.disabled = true;
    appendMessage("user", text);
    var thinking = appendThinking();
    var history = state.messages.slice(0, -1).slice(-MAX_HISTORY);
    fetch(ENDPOINT, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ message: text, history: history }),
    })
      .then(function (r) {
        if (!r.ok) return r.text().then(function (t) { throw new Error("HTTP " + r.status + ": " + t); });
        return r.json();
      })
      .then(function (data) {
        thinking.remove();
        appendMessage("assistant", data.message || "(empty response)");
      })
      .catch(function (err) {
        thinking.remove();
        appendMessage("assistant", "Sorry — couldn't reach the agent. " + (err && err.message ? err.message : ""));
      })
      .then(function () {
        state.sending = false;
        send.disabled = false;
        input.disabled = false;
        input.focus();
      });
  }

  // ── Render initial history if any ───────────────────────────────────
  for (var i = 0; i < state.messages.length; i++) {
    messages.appendChild(renderMessage(state.messages[i].role, state.messages[i].content));
  }

  // Expose a tiny API for power users (e.g. open chat from another button).
  window.AtoEmbed = window.AtoEmbed || {};
  window.AtoEmbed[SLUG] = {
    open: function () { setOpen(true); },
    close: function () { setOpen(false); },
    reset: function () { state.messages = []; saveHistory(); messages.innerHTML = ""; },
  };
})();
`;
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}
