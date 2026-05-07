import { useEffect, useState } from "react";
import { Play } from "lucide-react";
import { useDemoStore } from "@/stores/useDemoStore";
import { DEMO_SCRIPTS, HERO_SCRIPT } from "@/lib/demoScripts";

// v1.5.0 — Demo overlay.
//
// Two visible parts during playback:
//   1. Subtitle banner — narration. Lives at BOTTOM of the screen, above
//      the chat pane, so it doesn't cover the section being toured. Auto-
//      clears after each subtitle step's duration so it never overlaps with
//      typing animations.
//   2. Highlight pulses — when the runner emits a "highlight" or implicit
//      flash (navigate / setRuntime / openWizard / setSubTab), HOTSPOTS
//      with matching `data-demo-id` attributes get a glowing cyan ring so
//      viewers see what's being "clicked".
//
// Esc stops a running demo. ⌘⇧D toggles the hero script.

export default function DemoOverlay() {
  const isPlaying = useDemoStore((s) => s.isPlaying);
  const caption = useDemoStore((s) => s.caption);
  const play = useDemoStore((s) => s.play);
  const stop = useDemoStore((s) => s.stop);

  // Cmd+Shift+D toggles the hero script. Esc stops a running demo.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const playing = useDemoStore.getState().isPlaying;
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === "d") {
        e.preventDefault();
        if (playing) stop();
        else void play(HERO_SCRIPT);
        return;
      }
      if (e.key === "Escape" && playing) {
        e.preventDefault();
        stop();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [play, stop]);

  return (
    <>
      {/* Subtitle banner — bottom-center, above the chat pane.
          Caption auto-clears between subtitle steps so it never crowds
          the screen during typing animations. */}
      {isPlaying && caption && (
        <div className="fixed bottom-24 left-1/2 -translate-x-1/2 z-[80] pointer-events-none px-4">
          <div className="rounded-lg border border-cs-accent/40 bg-cs-bg/95 backdrop-blur-md px-5 py-2.5 shadow-2xl max-w-2xl">
            <p className="text-sm font-medium text-cs-accent text-center leading-snug">
              {caption}
            </p>
          </div>
        </div>
      )}

      {/* Highlight pulses on data-demo-id elements. */}
      {isPlaying && <HighlightHotspots />}

      {/* Floating picker (bottom-right). Hidden during playback. */}
      {!isPlaying && <DemoPicker />}
    </>
  );
}

/** Renders an invisible scanner that finds DOM elements with
 *  `data-demo-id` matching the active highlightIds, and toggles a CSS
 *  class on them for the pulse effect. */
function HighlightHotspots() {
  const ids = useDemoStore((s) => s.highlightIds);
  const [tick, setTick] = useState(0);

  // Re-poll the DOM occasionally — elements appear/disappear as the demo
  // navigates. Cheap (one querySelectorAll per highlight per tick) and
  // only runs while a demo is playing.
  useEffect(() => {
    const t = setInterval(() => setTick((x) => x + 1), 250);
    return () => clearInterval(t);
  }, []);

  useEffect(() => {
    const allTagged = document.querySelectorAll<HTMLElement>("[data-demo-id]");
    allTagged.forEach((el) => {
      const id = el.getAttribute("data-demo-id") ?? "";
      const shouldHighlight = ids.includes(id);
      el.classList.toggle("demo-highlight", shouldHighlight);
    });
    return () => {
      const allTagged = document.querySelectorAll<HTMLElement>("[data-demo-id]");
      allTagged.forEach((el) => el.classList.remove("demo-highlight"));
    };
  }, [ids, tick]);

  return null;
}

function DemoPicker() {
  const play = useDemoStore((s) => s.play);
  return (
    <div className="fixed bottom-3 right-3 z-[60]">
      <details className="group">
        <summary className="cursor-pointer list-none inline-flex items-center gap-1.5 rounded-full border border-cs-border bg-cs-card/80 backdrop-blur-sm px-2.5 py-1 text-[10px] font-mono text-cs-muted hover:text-cs-accent hover:border-cs-accent/40 transition-colors">
          <Play size={9} />
          demo
        </summary>
        <div className="absolute bottom-full right-0 mb-2 w-64 rounded-lg border border-cs-border bg-cs-card shadow-2xl overflow-hidden">
          <div className="border-b border-cs-border px-3 py-2">
            <p className="text-[10px] uppercase tracking-wide text-cs-muted">Demo scripts</p>
            <p className="text-[10px] text-cs-muted/70 mt-0.5">⌘⇧D plays the hero script</p>
          </div>
          {DEMO_SCRIPTS.map((script) => (
            <button
              key={script.id}
              type="button"
              onClick={() => void play(script)}
              className="w-full text-left px-3 py-2 text-xs hover:bg-cs-accent/5 transition-colors border-b border-cs-border/40 last:border-b-0"
            >
              <div className="text-cs-text font-medium">{script.label}</div>
              <div className="text-[10px] text-cs-muted mt-0.5">{script.shortDescription}</div>
            </button>
          ))}
        </div>
      </details>
    </div>
  );
}

// DemoStopButton removed — Esc stops a running demo, no on-screen chrome.
