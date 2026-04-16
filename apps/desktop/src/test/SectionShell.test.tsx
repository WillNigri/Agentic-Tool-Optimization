import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { Sparkles } from "lucide-react";
import SectionShell, { ScopeBadge, EmptyRow } from "@/components/Projects/sections/SectionShell";

describe("SectionShell", () => {
  it("renders title and subtitle", () => {
    render(
      <SectionShell icon={Sparkles} title="Test Section" subtitle="A test subtitle">
        <p>Content</p>
      </SectionShell>
    );
    expect(screen.getByText("Test Section")).toBeInTheDocument();
    expect(screen.getByText("A test subtitle")).toBeInTheDocument();
    expect(screen.getByText("Content")).toBeInTheDocument();
  });

  it("renders count badge when provided", () => {
    render(
      <SectionShell icon={Sparkles} title="Skills" count={5}>
        <p>Items</p>
      </SectionShell>
    );
    expect(screen.getByText("5")).toBeInTheDocument();
  });

  it("has aria-label from title", () => {
    render(
      <SectionShell icon={Sparkles} title="Memory">
        <p>Files</p>
      </SectionShell>
    );
    expect(screen.getByRole("region", { name: "Memory" }) || screen.getByLabelText("Memory")).toBeTruthy();
  });
});

describe("ScopeBadge", () => {
  it("renders scope text", () => {
    render(<ScopeBadge scope="user" />);
    expect(screen.getByText("user")).toBeInTheDocument();
  });

  it("renders project scope", () => {
    render(<ScopeBadge scope="project" />);
    expect(screen.getByText("project")).toBeInTheDocument();
  });
});

describe("EmptyRow", () => {
  it("renders message", () => {
    render(<EmptyRow message="Nothing here yet." />);
    expect(screen.getByText("Nothing here yet.")).toBeInTheDocument();
  });

  it("renders action button when provided", () => {
    const onAction = vi.fn();
    render(<EmptyRow message="Empty" actionLabel="Create" onAction={onAction} />);
    const btn = screen.getByText("Create");
    expect(btn).toBeInTheDocument();
    btn.click();
    expect(onAction).toHaveBeenCalledOnce();
  });

  it("hides action button when no onAction", () => {
    render(<EmptyRow message="Empty" actionLabel="Create" />);
    expect(screen.queryByText("Create")).not.toBeInTheDocument();
  });
});
