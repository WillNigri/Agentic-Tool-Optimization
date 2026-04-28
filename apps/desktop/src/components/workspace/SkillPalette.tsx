import { useState } from "react";
import { Search, Sparkles, Star, TrendingUp, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { MOCK_MARKETPLACE_SKILLS } from "@/lib/marketplace-mock";

interface SkillPaletteProps {
  existingSkillNames: string[];
  onClose: () => void;
  onInstall: (skill: { name: string; description: string; content: string }, runtime: string) => void;
}

function getSuggestions(existingNames: string[]) {
  const nameSet = new Set(existingNames.map((n) => n.toLowerCase()));
  return MOCK_MARKETPLACE_SKILLS.filter(
    (s) => !nameSet.has(s.name.toLowerCase())
  ).slice(0, 5);
}

export default function SkillPalette({ existingSkillNames, onClose, onInstall }: SkillPaletteProps) {
  const [search, setSearch] = useState("");
  const suggestions = getSuggestions(existingSkillNames);
  const popular = MOCK_MARKETPLACE_SKILLS.sort((a, b) => (b.installs ?? 0) - (a.installs ?? 0)).slice(0, 8);

  const filtered = search
    ? MOCK_MARKETPLACE_SKILLS.filter(
        (s) => s.name.toLowerCase().includes(search.toLowerCase()) || s.description?.toLowerCase().includes(search.toLowerCase())
      )
    : [];

  return (
    <div className="absolute left-0 top-0 bottom-0 w-64 bg-cs-card border-r border-cs-border z-20 flex flex-col shadow-2xl">
      <div className="flex items-center justify-between px-3 py-2.5 border-b border-cs-border">
        <h3 className="text-xs font-semibold">Add Skills</h3>
        <button onClick={onClose} className="p-1 rounded hover:bg-cs-border text-cs-muted"><X size={14} /></button>
      </div>

      <div className="px-3 py-2">
        <div className="relative">
          <Search size={12} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-cs-muted" />
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search skills..."
            className="w-full pl-8 pr-3 py-1.5 rounded-md border border-cs-border/60 bg-cs-bg text-xs focus:outline-none focus:border-cs-accent"
          />
        </div>
      </div>

      <div className="flex-1 overflow-y-auto px-3 pb-3">
        {search ? (
          <>
            <SectionLabel icon={Search} label={`Results (${filtered.length})`} />
            {filtered.length === 0 && <p className="text-[10px] text-cs-muted py-2">No skills found.</p>}
            {filtered.map((s) => (
              <SkillCard key={s.name} skill={s} onInstall={onInstall} />
            ))}
          </>
        ) : (
          <>
            {suggestions.length > 0 && (
              <>
                <SectionLabel icon={Star} label="Suggested for you" />
                {suggestions.map((s) => (
                  <SkillCard key={s.name} skill={s} onInstall={onInstall} suggested />
                ))}
              </>
            )}
            <SectionLabel icon={TrendingUp} label="Popular" />
            {popular.map((s) => (
              <SkillCard key={s.name} skill={s} onInstall={onInstall} />
            ))}
          </>
        )}
      </div>
    </div>
  );
}

function SectionLabel({ icon: Icon, label }: { icon: typeof Star; label: string }) {
  return (
    <div className="flex items-center gap-1.5 mt-3 mb-1.5">
      <Icon size={10} className="text-cs-muted" />
      <span className="text-[9px] text-cs-muted uppercase tracking-wide font-medium">{label}</span>
    </div>
  );
}

function SkillCard({
  skill,
  onInstall,
  suggested,
}: {
  skill: { name: string; description?: string; content?: string; installs?: number; author?: string };
  onInstall: (skill: { name: string; description: string; content: string }, runtime: string) => void;
  suggested?: boolean;
}) {
  return (
    <div
      className={cn(
        "rounded-md border px-2.5 py-2 mb-1.5 cursor-pointer transition-colors",
        suggested
          ? "border-cs-accent/30 bg-cs-accent/5 hover:bg-cs-accent/10"
          : "border-cs-border/60 hover:border-cs-accent/30 hover:bg-cs-bg"
      )}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData("application/workspace-skill", JSON.stringify(skill));
        e.dataTransfer.effectAllowed = "copy";
      }}
      onClick={() => onInstall(
        { name: skill.name, description: skill.description ?? "", content: skill.content ?? "" },
        "claude"
      )}
    >
      <div className="flex items-center gap-1.5">
        <Sparkles size={10} className={suggested ? "text-cs-accent" : "text-cs-muted"} />
        <span className="text-[11px] font-medium truncate flex-1">{skill.name}</span>
        {skill.installs && <span className="text-[8px] text-cs-muted">{skill.installs}</span>}
      </div>
      {skill.description && <p className="text-[9px] text-cs-muted line-clamp-1 mt-0.5 ml-4">{skill.description}</p>}
    </div>
  );
}
