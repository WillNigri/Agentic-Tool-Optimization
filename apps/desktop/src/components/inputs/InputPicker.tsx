import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { Check, ChevronDown, Search, X } from "lucide-react";

import { cn } from "@/lib/utils";
import { list_inputs } from "@/lib/inputs-api";

// Codex R2: kindFilter is a closed set — types must match. Loose
// string would let unknown kinds pass through and skip the filter.
type InputKind = "markdown" | "json" | "text";

interface InputPickerProps {
  value?: string;
  onSelect: (slug: string) => void;
  kindFilter?: InputKind;
}

export default function InputPicker({
  value,
  onSelect,
  kindFilter,
}: InputPickerProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  // Codex R2 a11y: track keyboard focus separately from mouse hover.
  // Up/Down moves activeIndex; Enter selects the active option; Escape
  // closes the listbox and returns focus to the button.
  const [activeIndex, setActiveIndex] = useState(0);
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  const listboxRef = useRef<HTMLDivElement | null>(null);

  const inputsQuery = useQuery({
    queryKey: ["inputs"],
    queryFn: () => list_inputs(),
    staleTime: 30_000,
  });

  const inputs = useMemo(() => {
    const all = inputsQuery.data ?? [];
    if (!kindFilter) return all;
    return all.filter((input) => input.kind === kindFilter);
  }, [inputsQuery.data, kindFilter]);

  const selected = useMemo(
    () => inputs.find((input) => input.slug === value) ?? null,
    [inputs, value],
  );

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return inputs;
    return inputs.filter((input) => {
      const haystack = [
        input.slug,
        input.name,
        input.kind,
        input.tags.join(" "),
      ]
        .join(" ")
        .toLowerCase();
      return haystack.includes(needle);
    });
  }, [inputs, query]);

  const choose = (slug: string) => {
    onSelect(slug);
    setOpen(false);
    setQuery("");
    setActiveIndex(0);
    // Return focus to the trigger so keyboard flow continues from there.
    buttonRef.current?.focus();
  };

  // Codex R2 a11y: total option list = sentinel "no input" row + filtered.
  // ActiveIndex 0 is the sentinel; >= 1 indexes into filtered.
  const optionCount = filtered.length + 1;
  // Keep activeIndex in range as the filter narrows.
  useEffect(() => {
    if (activeIndex >= optionCount) setActiveIndex(0);
  }, [activeIndex, optionCount]);

  const onListboxKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (!open) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex((i) => (i + 1) % optionCount);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex((i) => (i - 1 + optionCount) % optionCount);
    } else if (e.key === "Home") {
      e.preventDefault();
      setActiveIndex(0);
    } else if (e.key === "End") {
      e.preventDefault();
      setActiveIndex(optionCount - 1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (activeIndex === 0) {
        choose("");
      } else {
        const target = filtered[activeIndex - 1];
        if (target) choose(target.slug);
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
      buttonRef.current?.focus();
    }
  };

  const onButtonKeyDown = (e: React.KeyboardEvent<HTMLButtonElement>) => {
    if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      setOpen(true);
      setActiveIndex(0);
    }
  };

  return (
    <div className="relative">
      <button
        ref={buttonRef}
        type="button"
        role="combobox"
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-controls="input-picker-listbox"
        aria-label={t("inputPicker.buttonTitle", "Pick a saved input")}
        onClick={() => setOpen((v) => !v)}
        onKeyDown={onButtonKeyDown}
        className={cn(
          "w-full rounded-md border px-2 py-1.5 text-left text-xs transition-colors",
          "border-[#2a2a3a] bg-[#16161e] text-[#e8e8f0] hover:border-[#00FFB2]/50",
          "focus:outline-none focus:border-[#00FFB2]",
        )}
        title={t("inputPicker.buttonTitle", "Pick a saved input")}
      >
        <span className="flex items-center justify-between gap-2">
          <span className="min-w-0">
            {selected ? (
              <span className="block truncate font-mono text-[#00FFB2]">
                {selected.slug}
              </span>
            ) : (
              <span className="block truncate text-[#8888a0]">
                {t("inputPicker.placeholder", "Select a context input")}
              </span>
            )}
            {selected && (
              <span className="block truncate text-[10px] text-[#8888a0]">
                {selected.name}
              </span>
            )}
          </span>
          <ChevronDown
            size={12}
            className={cn("shrink-0 text-[#8888a0] transition-transform", open && "rotate-180")}
          />
        </span>
      </button>

      {open && (
        <>
          <div
            className="fixed inset-0 z-30"
            onClick={() => {
              setOpen(false);
              // Codex R3 — match Escape/select paths: return focus
              // to the trigger so screen-reader users land back at
              // the combobox rather than the document root.
              buttonRef.current?.focus();
            }}
          />
          <div
            ref={listboxRef}
            id="input-picker-listbox"
            role="listbox"
            aria-label={t("inputPicker.buttonTitle", "Pick a saved input")}
            tabIndex={-1}
            onKeyDown={onListboxKeyDown}
            className="absolute left-0 top-full z-40 mt-1 w-full min-w-[22rem] overflow-hidden rounded-lg border border-[#2a2a3a] bg-[#16161e] shadow-2xl"
          >
            <div className="border-b border-[#2a2a3a] p-2">
              <div className="relative">
                <Search
                  size={12}
                  className="absolute left-2.5 top-1/2 -translate-y-1/2 text-[#8888a0]"
                />
                <input
                  autoFocus
                  type="text"
                  value={query}
                  onChange={(e) => {
                    setQuery(e.target.value);
                    setActiveIndex(0);
                  }}
                  placeholder={t("inputPicker.searchPlaceholder", "Search inputs…")}
                  className="w-full rounded-md border border-[#2a2a3a] bg-[#0e0e16] py-1.5 pl-8 pr-2 text-xs text-[#e8e8f0] placeholder:text-[#8888a0] focus:border-[#00FFB2] focus:outline-none"
                />
              </div>
            </div>

            <div className="max-h-72 overflow-y-auto py-1">
              <button
                type="button"
                role="option"
                aria-selected={!value}
                onClick={() => choose("")}
                onMouseEnter={() => setActiveIndex(0)}
                className={cn(
                  "flex w-full items-center gap-2 px-3 py-2 text-left text-xs transition-colors",
                  activeIndex === 0 && "ring-1 ring-inset ring-[#00FFB2]/40",
                  !value ? "bg-[#00FFB2]/10 text-[#00FFB2]" : "text-[#c7c7d1] hover:bg-[#0e0e16]",
                )}
              >
                {!value ? <Check size={12} /> : <X size={12} />}
                <span>{t("inputPicker.none", "No context input")}</span>
              </button>

              {inputsQuery.isLoading && (
                <div className="px-3 py-3 text-xs text-[#8888a0]">
                  {t("inputPicker.loading", "Loading inputs…")}
                </div>
              )}

              {inputsQuery.isError && (
                <div className="px-3 py-3 text-xs text-red-400">
                  {t("inputPicker.error", "Could not load inputs")}
                </div>
              )}

              {!inputsQuery.isLoading && !inputsQuery.isError && filtered.length === 0 && (
                <div className="px-3 py-3 text-xs text-[#8888a0]">
                  {t("inputPicker.empty", "No inputs match this search")}
                </div>
              )}

              {filtered.map((input, idx) => {
                const isActive = input.slug === value;
                const isKeyboardActive = activeIndex === idx + 1;
                return (
                  <button
                    key={input.id}
                    type="button"
                    role="option"
                    aria-selected={isActive}
                    onClick={() => choose(input.slug)}
                    onMouseEnter={() => setActiveIndex(idx + 1)}
                    className={cn(
                      "w-full px-3 py-2 text-left transition-colors",
                      isKeyboardActive && "ring-1 ring-inset ring-[#00FFB2]/40",
                      isActive ? "bg-[#00FFB2]/10" : "hover:bg-[#0e0e16]",
                    )}
                  >
                    <div className="flex items-start gap-2">
                      <div className="pt-0.5">
                        <Check
                          size={12}
                          className={isActive ? "text-[#00FFB2]" : "text-transparent"}
                        />
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <code
                            className={cn(
                              "truncate text-xs",
                              isActive ? "text-[#00FFB2]" : "text-[#e8e8f0]",
                            )}
                          >
                            {input.slug}
                          </code>
                          <span className="rounded border border-[#2a2a3a] px-1.5 py-0.5 text-[9px] uppercase tracking-wide text-[#8888a0]">
                            {input.kind}
                          </span>
                        </div>
                        <div className="truncate text-[11px] text-[#c7c7d1]">
                          {input.name}
                        </div>
                        {input.tags.length > 0 && (
                          <div className="mt-1 flex flex-wrap gap-1">
                            {input.tags.slice(0, 4).map((tag) => (
                              <span
                                key={tag}
                                className="rounded bg-[#0e0e16] px-1.5 py-0.5 text-[9px] text-[#8888a0]"
                              >
                                #{tag}
                              </span>
                            ))}
                          </div>
                        )}
                      </div>
                    </div>
                  </button>
                );
              })}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
