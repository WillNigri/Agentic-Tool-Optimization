import { useEffect, useRef } from "react";
import { EditorState, Compartment } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { json, jsonParseLinter } from "@codemirror/lang-json";
import { yaml } from "@codemirror/lang-yaml";
import { oneDark } from "@codemirror/theme-one-dark";
import { syntaxHighlighting, defaultHighlightStyle, bracketMatching, indentOnInput } from "@codemirror/language";
import { lintGutter, linter, type Diagnostic } from "@codemirror/lint";
import { validateSettingsJson } from "@/lib/api";

export type ATOEditorLanguage = "markdown" | "json" | "yaml" | "toml" | "text";

interface ATOEditorProps {
  value: string;
  language?: ATOEditorLanguage;
  filePath?: string;
  readOnly?: boolean;
  onChange: (value: string) => void;
  onSave?: () => void;
  className?: string;
}

function detectLanguage(filePath: string | undefined): ATOEditorLanguage {
  if (!filePath) return "text";
  const lower = filePath.toLowerCase();
  if (lower.endsWith(".md") || lower.endsWith(".markdown")) return "markdown";
  if (lower.endsWith(".json")) return "json";
  if (lower.endsWith(".yaml") || lower.endsWith(".yml")) return "yaml";
  if (lower.endsWith(".toml")) return "toml";
  return "text";
}

function getLanguageExtension(lang: ATOEditorLanguage) {
  switch (lang) {
    case "markdown": return markdown();
    case "json": return json();
    case "yaml":
    case "toml":
      // CodeMirror doesn't ship a TOML parser in the small bundle; YAML is close enough visually
      return yaml();
    default:
      return [];
  }
}

function isSettingsJsonPath(filePath: string | undefined): boolean {
  if (!filePath) return false;
  return filePath.endsWith("/settings.json") || filePath.endsWith("/settings.local.json");
}

/**
 * Async schema linter for Claude Code settings.json.
 * Debounced via CodeMirror's internal delay (default 750ms), then calls our Rust validator.
 */
async function settingsSchemaLinter(view: EditorView): Promise<Diagnostic[]> {
  const text = view.state.doc.toString();
  if (!text.trim()) return [];
  try {
    const result = await validateSettingsJson(text);
    if (result.valid) return [];
    // We only get field-path errors back; CodeMirror wants doc offsets. Best effort:
    // map the first line offset for now so the gutter shows; users see full list in SaveConfirmDialog.
    return result.errors.map((err) => ({
      from: 0,
      to: Math.min(1, view.state.doc.length),
      severity: "error" as const,
      message: `${err.field}: ${err.message}`,
      source: "settings-schema",
    }));
  } catch {
    return [];
  }
}

function buildLinters(lang: ATOEditorLanguage, filePath: string | undefined) {
  const extensions = [];
  if (lang === "json") {
    extensions.push(linter(jsonParseLinter()));
    if (isSettingsJsonPath(filePath)) {
      extensions.push(linter(settingsSchemaLinter, { delay: 800 }));
    }
  }
  return extensions;
}

export default function ATOEditor({
  value,
  language,
  filePath,
  readOnly = false,
  onChange,
  onSave,
  className,
}: ATOEditorProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const langCompartmentRef = useRef(new Compartment());
  const lintCompartmentRef = useRef(new Compartment());
  const readOnlyCompartmentRef = useRef(new Compartment());
  const onChangeRef = useRef(onChange);
  const onSaveRef = useRef(onSave);

  useEffect(() => {
    onChangeRef.current = onChange;
    onSaveRef.current = onSave;
  }, [onChange, onSave]);

  // Initial mount
  useEffect(() => {
    if (!hostRef.current) return;
    const lang = language ?? detectLanguage(filePath);

    const saveShortcut = keymap.of([
      {
        key: "Mod-s",
        preventDefault: true,
        run: () => {
          onSaveRef.current?.();
          return true;
        },
      },
    ]);

    const updateListener = EditorView.updateListener.of((update) => {
      if (update.docChanged) {
        onChangeRef.current(update.state.doc.toString());
      }
    });

    const state = EditorState.create({
      doc: value,
      extensions: [
        lineNumbers(),
        highlightActiveLine(),
        highlightActiveLineGutter(),
        history(),
        bracketMatching(),
        indentOnInput(),
        syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
        oneDark,
        lintGutter(),
        keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
        saveShortcut,
        updateListener,
        EditorView.theme({
          "&": {
            height: "100%",
            fontSize: "13px",
            fontFamily: "ui-monospace, 'SF Mono', Menlo, monospace",
          },
          ".cm-scroller": { fontFamily: "inherit" },
          ".cm-gutters": {
            backgroundColor: "#0a0a0f",
            borderRight: "1px solid rgba(255,255,255,0.06)",
          },
          "&.cm-focused .cm-selectionBackground, ::selection": {
            backgroundColor: "rgba(0, 255, 178, 0.18)",
          },
          ".cm-activeLine": { backgroundColor: "rgba(0, 255, 178, 0.04)" },
          ".cm-activeLineGutter": { backgroundColor: "rgba(0, 255, 178, 0.08)" },
        }),
        langCompartmentRef.current.of(getLanguageExtension(lang)),
        lintCompartmentRef.current.of(buildLinters(lang, filePath)),
        readOnlyCompartmentRef.current.of(EditorState.readOnly.of(readOnly)),
      ],
    });

    const view = new EditorView({ state, parent: hostRef.current });
    viewRef.current = view;

    return () => {
      view.destroy();
      viewRef.current = null;
    };
    // Intentionally mount-only; updates flow through effects below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Keep external value in sync when it changes (e.g., file reload).
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const current = view.state.doc.toString();
    if (current !== value) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: value },
      });
    }
  }, [value]);

  // Swap language extension + linters if language or filePath changes.
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const lang = language ?? detectLanguage(filePath);
    view.dispatch({
      effects: [
        langCompartmentRef.current.reconfigure(getLanguageExtension(lang)),
        lintCompartmentRef.current.reconfigure(buildLinters(lang, filePath)),
      ],
    });
  }, [language, filePath]);

  // Toggle readOnly.
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    view.dispatch({
      effects: readOnlyCompartmentRef.current.reconfigure(EditorState.readOnly.of(readOnly)),
    });
  }, [readOnly]);

  return <div ref={hostRef} className={className ?? "h-full w-full overflow-hidden rounded-lg border border-cs-border bg-cs-bg"} />;
}
