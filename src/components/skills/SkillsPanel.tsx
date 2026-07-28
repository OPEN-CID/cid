import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api";
import { toast } from "@/lib/dialog";
import { useCid } from "@/hooks/useCid";

type Skill = {
  id: string;
  name: string;
  content: string;
  scope: string;
  scope_id?: string | null;
  created_at: string;
  updated_at: string;
};

export function SkillsPanel() {
  const { repos, selectedRepoId } = useCid();
  const [skills, setSkills] = useState<Skill[]>([]);
  const [agentsMd, setAgentsMd] = useState<string | null>(null);
  const [agentsMdEdit, setAgentsMdEdit] = useState<string>("");
  const [isEditingAgents, setIsEditingAgents] = useState(false);
  const [newSkill, setNewSkill] = useState({ name: "", content: "", scope: "workspace" });

  const selectedRepo = repos.find((r) => r.id === selectedRepoId);

  const load = useCallback(async () => {
    try {
      const list = await api.skills.list();
      setSkills(list);
      if (selectedRepo) {
        const agents = await api.repo.agentsMd(selectedRepo.path);
        setAgentsMd(agents.content);
        setAgentsMdEdit(agents.content || "");
      }
    } catch (e) {
      console.error(e);
    }
    // Keyed on selectedRepo?.path rather than the selectedRepo object for the
    // same reason as LeftRail's context-engine-status effect: `.find()`
    // re-derives a new object every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedRepo?.path]);

  useEffect(() => {
    load();
  }, [load]);

  const handleSaveAgents = async () => {
    if (!selectedRepo) return;
    try {
      await api.call("repo.agents_md.write", { path: selectedRepo.path, content: agentsMdEdit });
      setAgentsMd(agentsMdEdit);
      setIsEditingAgents(false);
    } catch (e) {
      toast.error(`Failed to save AGENTS.md: ${e}`);
    }
  };

  const handleSaveSkill = async () => {
    if (!newSkill.name.trim() || !newSkill.content.trim()) return;
    try {
      await api.skills.save({
        id: `skill-${Date.now()}`,
        name: newSkill.name,
        content: newSkill.content,
        scope: newSkill.scope,
        scope_id: newSkill.scope === "repo" ? selectedRepoId : null,
        created_at: new Date().toISOString(),
        updated_at: new Date().toISOString(),
      });
      setNewSkill({ name: "", content: "", scope: "workspace" });
      load();
    } catch (e) {
      toast.error(`Failed to save skill: ${e}`);
    }
  };

  return (
    <div className="p-4 space-y-6 overflow-y-auto h-full">
      <div>
        <div className="flex items-center justify-between mb-2">
          <h3 className="font-semibold text-sm">AGENTS.md (Repo Channel Context)</h3>
          {selectedRepo && (
            <button
              onClick={() => (isEditingAgents ? handleSaveAgents() : setIsEditingAgents(true))}
              className="text-[11px] bg-primary text-primary-foreground px-2 py-1 rounded"
            >
              {isEditingAgents ? "Save" : agentsMd ? "Edit" : "Create"}
            </button>
          )}
        </div>
        <div className="text-xs text-muted-foreground mb-2">
          Auto-detected from repo root, pinned at top of channel. Editable inline (writes back to actual file in repo — CID doesn&apos;t fork the format into a DB).
        </div>
        {isEditingAgents ? (
          <div className="space-y-2">
            <textarea
              className="w-full bg-card border rounded p-3 text-xs font-mono min-h-[200px]"
              value={agentsMdEdit}
              onChange={(e) => setAgentsMdEdit(e.target.value)}
              placeholder="# AGENTS.md\n\nRepo-specific instructions for CID and other agents..."
            />
            <div className="flex gap-2">
              <button onClick={handleSaveAgents} className="text-xs bg-primary text-primary-foreground px-3 py-1 rounded">
                Save
              </button>
              <button onClick={() => setIsEditingAgents(false)} className="text-xs bg-secondary px-3 py-1 rounded">
                Cancel
              </button>
            </div>
          </div>
        ) : agentsMd ? (
          <pre className="bg-card border rounded p-3 text-xs whitespace-pre-wrap max-h-64 overflow-y-auto">{agentsMd}</pre>
        ) : (
          <div className="bg-card border rounded p-3 text-xs text-muted-foreground">
            No AGENTS.md found in this repo. Create one to give CID repo-specific instructions. This file is read natively by 20-30+ tools (Claude Code, Cursor, Copilot, Junie) — CID shares the same format, zero migration.
          </div>
        )}
      </div>

      <div>
        <h3 className="font-semibold text-sm mb-2">Skills (Workspace & Repo)</h3>
        <div className="text-xs text-muted-foreground mb-2">
          Workspace context = org-wide conventions. Repo context = repo-specific. Resolution: Mission &gt; Repo &gt; Workspace, nearest wins. Skills are markdown snippets stored in SQLite Phase 0, full multi-file SKILL.md support is Phase 1.
        </div>

        <div className="space-y-2 mb-4">
          {skills.map((s) => (
            <div key={s.id} className="border rounded p-2 bg-card">
              <div className="flex items-center gap-2">
                <span className="font-medium text-xs">{s.name}</span>
                <span className="text-[10px] bg-accent px-1 rounded">{s.scope}</span>
                <span className="text-[10px] text-muted-foreground ml-auto">{new Date(s.updated_at).toLocaleDateString()}</span>
              </div>
              <pre className="text-[11px] mt-1 whitespace-pre-wrap">{s.content.slice(0, 200)}</pre>
            </div>
          ))}
          {skills.length === 0 && <div className="text-xs text-muted-foreground">No skills yet — add workspace or repo-specific conventions.</div>}
        </div>

        <div className="border rounded p-3 bg-card space-y-2">
          <div className="text-xs font-medium">Add Skill</div>
          <input
            className="w-full bg-background border rounded px-2 py-1 text-xs"
            placeholder="Skill name (e.g., commit-convention)"
            value={newSkill.name}
            onChange={(e) => setNewSkill({ ...newSkill, name: e.target.value })}
          />
          <select
            className="w-full bg-background border rounded px-2 py-1 text-xs"
            value={newSkill.scope}
            onChange={(e) => setNewSkill({ ...newSkill, scope: e.target.value })}
          >
            <option value="workspace">Workspace (org-wide)</option>
            <option value="repo">Repo Channel (this repo)</option>
          </select>
          <textarea
            className="w-full bg-background border rounded px-2 py-1 text-xs font-mono min-h-[80px]"
            placeholder="Skill content markdown..."
            value={newSkill.content}
            onChange={(e) => setNewSkill({ ...newSkill, content: e.target.value })}
          />
          <button onClick={handleSaveSkill} className="bg-primary text-primary-foreground text-xs px-3 py-1 rounded">
            Save Skill
          </button>
        </div>
      </div>
    </div>
  );
}
