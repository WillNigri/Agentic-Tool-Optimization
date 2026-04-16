import "@testing-library/jest-dom/vitest";

// Mock Tauri API for tests
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockRejectedValue(new Error("Not in Tauri")),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
