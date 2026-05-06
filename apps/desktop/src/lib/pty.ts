import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// v1.3.0 — Frontend wrappers for the embedded terminal Tauri commands (T5).
// PTY data streams arrive on `pty://data/<ptyId>` events; exit fires on
// `pty://exit/<ptyId>`. Frontend should call `dispose()` on the returned handle
// when the xterm instance is unmounted.

export type PtyDataEvent = { data: string };
export type PtyExitEvent = { ptyId: string; code: number | null };

export interface PtyHandle {
  ptyId: string;
  /** Send keyboard input (raw chars). */
  write: (data: string) => Promise<void>;
  /** Tell the kernel about new TTY dimensions. */
  resize: (rows: number, cols: number) => Promise<void>;
  /** Force kill the child process and detach event listeners. */
  kill: () => Promise<void>;
  /** Detach event listeners without killing (e.g. on remount). */
  dispose: () => Promise<void>;
}

export interface SpawnOptions {
  cwd?: string;
  shell?: string;
  rows?: number;
  cols?: number;
  onData: (data: string) => void;
  onExit?: (code: number | null) => void;
}

export async function spawnPty(opts: SpawnOptions): Promise<PtyHandle> {
  const ptyId = await invoke<string>("pty_spawn", {
    cwd: opts.cwd ?? null,
    shell: opts.shell ?? null,
    rows: opts.rows ?? 30,
    cols: opts.cols ?? 100,
  });

  const unlisteners: UnlistenFn[] = [];
  const dataUn = await listen<PtyDataEvent>(`pty://data/${ptyId}`, (e) => {
    opts.onData(e.payload.data);
  });
  unlisteners.push(dataUn);

  if (opts.onExit) {
    const exitUn = await listen<{ pty_id: string; code: number | null }>(
      `pty://exit/${ptyId}`,
      (e) => {
        opts.onExit?.(e.payload.code);
      }
    );
    unlisteners.push(exitUn);
  }

  const dispose = async () => {
    for (const un of unlisteners.splice(0)) {
      try {
        un();
      } catch {
        // ignore double-dispose
      }
    }
  };

  return {
    ptyId,
    write: (data: string) => invoke("pty_write", { ptyId, data }),
    resize: (rows: number, cols: number) => invoke("pty_resize", { ptyId, rows, cols }),
    kill: async () => {
      await dispose();
      try {
        await invoke("pty_kill", { ptyId });
      } catch {
        // already gone
      }
    },
    dispose,
  };
}

export async function listPtys(): Promise<string[]> {
  return invoke<string[]>("pty_list");
}
