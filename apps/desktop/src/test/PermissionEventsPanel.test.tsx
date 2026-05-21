// PR-5 UI tests — parsing + categorization of the tool_calls_summary
// JSON column. The panel itself is presentational; the load-bearing
// logic is the categorizer (denied vs advisory vs allowed) and the
// graceful handling of malformed JSON. Both are unit-tested here.

import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import PermissionEventsPanel, {
  parsePermissionEvents,
  categorizeToolCalls,
} from "@/components/SessionsList/PermissionEventsPanel";

describe("parsePermissionEvents", () => {
  it("parses a well-formed tool_calls_summary JSON array", () => {
    const raw = JSON.stringify([
      { name: "read_file", args_brief: "path=src/lib.rs", is_error: false },
      { name: "grep", args_brief: "pattern=TODO", is_error: false },
    ]);
    const events = parsePermissionEvents(raw);
    expect(events.allowed).toHaveLength(2);
    expect(events.denied).toHaveLength(0);
    expect(events.advisory).toHaveLength(0);
    expect(events.allowed[0].name).toBe("read_file");
    expect(events.allowed[0].argsBrief).toBe("path=src/lib.rs");
    expect(events.allowed[1].name).toBe("grep");
  });

  it("returns empty buckets when given malformed JSON", () => {
    const events = parsePermissionEvents("not json {{{{");
    expect(events.allowed).toEqual([]);
    expect(events.denied).toEqual([]);
    expect(events.advisory).toEqual([]);
  });

  it("returns empty buckets when given null or empty input", () => {
    expect(parsePermissionEvents(null).allowed).toEqual([]);
    expect(parsePermissionEvents(undefined).denied).toEqual([]);
    expect(parsePermissionEvents("").advisory).toEqual([]);
  });

  it("categorizes a denied event into the denied bucket", () => {
    const raw = JSON.stringify([
      { name: "read_file", args_brief: "path=ok.rs", is_error: false },
      {
        name: "exec",
        args_brief: "rm -rf / (blocked by agent policy)",
        is_error: true,
      },
    ]);
    const events = parsePermissionEvents(raw);
    expect(events.allowed).toHaveLength(1);
    expect(events.denied).toHaveLength(1);
    expect(events.denied[0].name).toBe("exec");
    expect(events.denied[0].isError).toBe(true);
  });
});

describe("categorizeToolCalls", () => {
  it("places generic errors in allowed (not denied) without the policy marker", () => {
    const events = categorizeToolCalls([
      { name: "grep", argsBrief: "pattern=x (no matches)", isError: true },
    ]);
    expect(events.denied).toHaveLength(0);
    expect(events.allowed).toHaveLength(1);
  });

  it("routes the advisory_only marker to advisory regardless of isError", () => {
    const events = categorizeToolCalls([
      { name: "shell", argsBrief: "ls -la (advisory_only)", isError: false },
    ]);
    expect(events.advisory).toHaveLength(1);
    expect(events.allowed).toHaveLength(0);
  });

  // War-room pin (claude+google, 2026-05-20 review round): the denied
  // category must trip on the marker alone. Requiring isError too means a
  // backend regression that emits the marker without the flag silently
  // demotes the denial to "allowed" — the worst miscategorization.
  it("treats the denied marker as sufficient even when isError is false", () => {
    const events = categorizeToolCalls([
      {
        name: "exec",
        argsBrief: "rm -rf / (blocked by agent policy)",
        isError: false,
      },
    ]);
    expect(events.denied).toHaveLength(1);
    expect(events.allowed).toHaveLength(0);
  });

  // Pins the advisory-wins-over-denied precedence so a future refactor
  // can't reorder the branches without a failing test.
  it("prefers advisory over denied when both markers appear", () => {
    const events = categorizeToolCalls([
      {
        name: "shell",
        argsBrief: "rm -rf / (blocked by agent policy, advisory_only)",
        isError: true,
      },
    ]);
    expect(events.advisory).toHaveLength(1);
    expect(events.denied).toHaveLength(0);
  });

  it("handles an explicitly empty array", () => {
    const events = parsePermissionEvents("[]");
    expect(events.allowed).toEqual([]);
    expect(events.denied).toEqual([]);
    expect(events.advisory).toEqual([]);
  });
});

describe("<PermissionEventsPanel />", () => {
  it("renders nothing when there are no events", () => {
    const { container } = render(
      <PermissionEventsPanel toolCallsSummary={null} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing for malformed JSON", () => {
    const { container } = render(
      <PermissionEventsPanel toolCallsSummary={"not json"} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders the Denied bucket label when a policy denial is present", () => {
    const raw = JSON.stringify([
      {
        name: "exec",
        args_brief: "rm -rf / (blocked by agent policy)",
        is_error: true,
      },
    ]);
    const { getByText } = render(
      <PermissionEventsPanel toolCallsSummary={raw} />,
    );
    expect(getByText("Denied")).toBeInTheDocument();
    expect(getByText("exec")).toBeInTheDocument();
  });

  it("omits empty Denied / Advisory buckets when only allowed events exist", () => {
    const raw = JSON.stringify([
      { name: "read_file", args_brief: "path=ok.rs", is_error: false },
    ]);
    const { queryByText, getByText } = render(
      <PermissionEventsPanel toolCallsSummary={raw} />,
    );
    expect(getByText("Allowed")).toBeInTheDocument();
    expect(queryByText("Denied")).toBeNull();
    expect(queryByText("Advisory")).toBeNull();
  });
});
