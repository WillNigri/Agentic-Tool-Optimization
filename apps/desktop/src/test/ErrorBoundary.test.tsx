import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import ErrorBoundary from "@/components/ErrorBoundary";

function ThrowingComponent() {
  throw new Error("Test error");
}

function GoodComponent() {
  return <p>All good</p>;
}

describe("ErrorBoundary", () => {
  it("renders children when no error", () => {
    render(
      <ErrorBoundary>
        <GoodComponent />
      </ErrorBoundary>
    );
    expect(screen.getByText("All good")).toBeInTheDocument();
  });

  it("catches errors and shows fallback", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <ErrorBoundary>
        <ThrowingComponent />
      </ErrorBoundary>
    );
    expect(screen.getByText("Something went wrong")).toBeInTheDocument();
    expect(screen.getByText("Try again")).toBeInTheDocument();
    vi.restoreAllMocks();
  });

  it("detects Tauri-specific errors", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    function TauriError() {
      throw new Error("invoke is not defined __TAURI__");
    }
    render(
      <ErrorBoundary>
        <TauriError />
      </ErrorBoundary>
    );
    expect(screen.getByText("Desktop app required")).toBeInTheDocument();
    vi.restoreAllMocks();
  });
});
