#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // v2.7.14 — Fedora / Arch / NVIDIA white-screen fix. Per the
    // war-room E2D6ABF5-… (claude vs minimax/google, coordinator
    // picked claude's diagnosis): WebKitGTK ≥ 2.40 has a broken
    // DMA-BUF renderer path that triggers a blank/white window
    // when the AppImage runs under GNOME/Wayland. Setting
    // `WEBKIT_DISABLE_DMABUF_RENDERER=1` BEFORE Tauri builds the
    // webview forces the legacy compositing path and fixes the
    // symptom for every affected cohort at once. Ubuntu still on
    // webkit2gtk 2.38.x is why other Linux contributors don't
    // repro; setting the env var unconditionally on Linux is a
    // no-op on the older WebKitGTK that didn't have the bug.
    //
    // Must run BEFORE `ato_desktop_lib::run()` because Tauri
    // initializes the webview as part of `run()` and the env var
    // is read once at webview-creation time. Held this as a
    // separate line so the comment trail is searchable when the
    // bug fix is eventually obviated by an upstream WebKitGTK
    // patch and the line can be removed.
    #[cfg(target_os = "linux")]
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    ato_desktop_lib::run()
}
