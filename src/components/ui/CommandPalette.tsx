import { useEffect, useMemo, useRef, useState } from "react";
import { Search } from "lucide-react";
import { useFocusTrap } from "@/lib/useFocusTrap";
import { t } from "@/lib/i18n";

export type Command = {
  id: string;
  label: string;
  hint?: string;
  keywords?: string;
  action: () => void;
};

// 051-Editor-Excellence-Roadmap.md Wave 5.2: a keyboard-first entry point to
// every surface — Ctrl+K opens a filterable list of every command the parent
// registers (tab switches, session creation, theme toggle, etc.), navigable
// entirely without a mouse.
export function CommandPalette({ commands }: { commands: Command[] }) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const modalRef = useFocusTrap<HTMLDivElement>(open, () => setOpen(false));

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((v) => !v);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  useEffect(() => {
    if (open) {
      setQuery("");
      setActiveIndex(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return commands;
    return commands.filter(
      (c) => c.label.toLowerCase().includes(q) || c.keywords?.toLowerCase().includes(q)
    );
  }, [commands, query]);

  useEffect(() => {
    setActiveIndex(0);
  }, [filtered.length]);

  const run = (cmd: Command) => {
    setOpen(false);
    cmd.action();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex((i) => Math.min(i + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const cmd = filtered[activeIndex];
      if (cmd) run(cmd);
    }
  };

  if (!open) return null;

  return (
    <div className="fixed inset-0 bg-black/50 flex items-start justify-center pt-[15vh] z-[120]">
      <div
        ref={modalRef}
        role="dialog"
        aria-modal="true"
        aria-label={t().commandPalette.ariaLabel}
        tabIndex={-1}
        className="bg-card border rounded-lg w-[480px] max-w-[90vw] max-h-[60vh] flex flex-col overflow-hidden shadow-xl"
      >
        <div className="flex items-center gap-2 px-3 py-2 border-b">
          <Search className="w-4 h-4 text-muted-foreground shrink-0" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={t().commandPalette.placeholder}
            className="flex-1 bg-transparent outline-none text-sm"
            aria-label={t().commandPalette.searchAriaLabel}
            role="combobox"
            aria-expanded="true"
            aria-controls="command-palette-list"
            aria-activedescendant={filtered[activeIndex] ? `command-${filtered[activeIndex].id}` : undefined}
          />
          <kbd className="text-[10px] text-muted-foreground border rounded px-1">Esc</kbd>
        </div>
        <div id="command-palette-list" role="listbox" className="flex-1 overflow-y-auto p-1">
          {filtered.length === 0 && <div className="text-xs text-muted-foreground p-3">{t().commandPalette.noMatches}</div>}
          {filtered.map((cmd, i) => (
            <button
              key={cmd.id}
              id={`command-${cmd.id}`}
              role="option"
              aria-selected={i === activeIndex}
              onMouseEnter={() => setActiveIndex(i)}
              onClick={() => run(cmd)}
              className={`w-full text-left px-3 py-2 rounded text-sm flex items-center justify-between ${
                i === activeIndex ? "bg-accent text-accent-foreground" : "hover:bg-accent/50"
              }`}
            >
              <span>{cmd.label}</span>
              {cmd.hint && <span className="text-[10px] text-muted-foreground">{cmd.hint}</span>}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
