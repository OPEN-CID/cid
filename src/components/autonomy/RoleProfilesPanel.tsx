import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api";
import { toast, confirmDialog } from "@/lib/dialog";
import { useCid } from "@/hooks/useCid";
import { Plus, Trash2, Pencil } from "lucide-react";

type ToolPermission = "read_file" | "write_file" | "run_terminal" | "git_ops" | "mcp_tools";
type ProfileScope = "workspace" | "repo";

type RoleProfile = {
  id: string;
  name: string;
  description: string;
  scope: ProfileScope;
  scope_id: string;
  system_prompt: string;
  model_provider?: string | null;
  model_id?: string | null;
  allowed_tools: ToolPermission[];
};

const ALL_TOOLS: ToolPermission[] = ["read_file", "write_file", "run_terminal", "git_ops", "mcp_tools"];

const emptyForm = {
  name: "",
  description: "",
  scope: "repo" as ProfileScope,
  system_prompt: "",
  model_provider: "",
  model_id: "",
  allowed_tools: [] as ToolPermission[],
};

// 051-Editor-Excellence-Roadmap.md Wave 5.1a: role_profile.* (Phase 4, real
// tool-permission enforcement in the dispatch path) had no way to create or
// assign a profile from any surface — folded in beside the autonomy
// allow-list rather than a new top-level tab, since a profile is itself an
// autonomy concept (which tools an agent may use at all).
export function RoleProfilesPanel() {
  const { repos, selectedRepoId } = useCid();
  const selectedRepo = repos.find((r) => r.id === selectedRepoId);
  const [profiles, setProfiles] = useState<RoleProfile[]>([]);
  const [loading, setLoading] = useState(false);
  const [showForm, setShowForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [form, setForm] = useState(emptyForm);

  const load = useCallback(async () => {
    if (!selectedRepo) return;
    setLoading(true);
    try {
      const list = await api.roleProfile.listForRepo(selectedRepo.workspace_id, selectedRepo.id);
      setProfiles(list || []);
    } catch (e) {
      toast.error(`Failed to load role profiles: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [selectedRepo]);

  useEffect(() => {
    load();
  }, [load]);

  const startCreate = () => {
    setEditingId(null);
    setForm(emptyForm);
    setShowForm(true);
  };

  const startEdit = (p: RoleProfile) => {
    setEditingId(p.id);
    setForm({
      name: p.name,
      description: p.description,
      scope: p.scope,
      system_prompt: p.system_prompt,
      model_provider: p.model_provider || "",
      model_id: p.model_id || "",
      allowed_tools: p.allowed_tools,
    });
    setShowForm(true);
  };

  const toggleTool = (tool: ToolPermission) => {
    setForm((f) => ({
      ...f,
      allowed_tools: f.allowed_tools.includes(tool) ? f.allowed_tools.filter((t) => t !== tool) : [...f.allowed_tools, tool],
    }));
  };

  const submit = async () => {
    if (!selectedRepo || !form.name.trim()) return;
    const input = {
      name: form.name.trim(),
      description: form.description.trim(),
      scope: form.scope,
      scope_id: form.scope === "workspace" ? selectedRepo.workspace_id : selectedRepo.id,
      system_prompt: form.system_prompt,
      model_provider: form.model_provider.trim() || null,
      model_id: form.model_id.trim() || null,
      allowed_tools: form.allowed_tools,
    };
    try {
      if (editingId) {
        await api.roleProfile.update(editingId, input);
      } else {
        await api.roleProfile.create(input);
      }
      setShowForm(false);
      setEditingId(null);
      setForm(emptyForm);
      await load();
    } catch (e) {
      toast.error(`Failed to save role profile: ${e}`);
    }
  };

  const remove = async (p: RoleProfile) => {
    if (!(await confirmDialog(`Delete role profile "${p.name}"?`))) return;
    try {
      await api.roleProfile.delete(p.id);
      await load();
    } catch (e) {
      toast.error(`Failed to delete role profile: ${e}`);
    }
  };

  if (!selectedRepo) return null;

  return (
    <div className="space-y-3 border-t pt-4">
      <div className="flex items-center justify-between">
        <div className="font-medium">Role profiles</div>
        <button onClick={startCreate} className="flex items-center gap-1 px-2 py-0.5 rounded bg-primary text-primary-foreground">
          <Plus className="w-3 h-3" /> New profile
        </button>
      </div>
      <div className="text-[10px] text-muted-foreground">
        A profile is a scoped prompt + tool-permission set a Mission&apos;s Planner can spawn as a subagent (e.g. a
        &quot;Security Reviewer&quot; limited to read_file). Workspace-scoped profiles are visible to every repo;
        repo-scoped ones only to this repo.
      </div>

      {loading && <div className="text-muted-foreground">Loading…</div>}

      <div className="space-y-1.5">
        {profiles.map((p) => (
          <div key={p.id} className="flex items-center gap-2 p-2 border rounded bg-background">
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-1.5">
                <span className="font-medium truncate">{p.name}</span>
                <span className="text-[10px] bg-accent px-1 rounded">{p.scope}</span>
              </div>
              {p.description && <div className="text-[10px] text-muted-foreground truncate">{p.description}</div>}
              <div className="text-[10px] text-muted-foreground truncate">
                {p.allowed_tools.length ? p.allowed_tools.join(", ") : "no tools allowed"}
              </div>
            </div>
            <button onClick={() => startEdit(p)} className="text-muted-foreground hover:text-foreground" aria-label={`Edit ${p.name}`}>
              <Pencil className="w-3 h-3" />
            </button>
            <button onClick={() => remove(p)} className="text-muted-foreground hover:text-red-500" aria-label={`Delete ${p.name}`}>
              <Trash2 className="w-3 h-3" />
            </button>
          </div>
        ))}
        {!loading && profiles.length === 0 && <div className="text-muted-foreground">No role profiles configured.</div>}
      </div>

      {showForm && (
        <div className="p-2 border rounded bg-background space-y-1.5">
          <div className="font-medium">{editingId ? "Edit profile" : "New profile"}</div>
          <input
            className="w-full bg-background border rounded px-2 py-1 text-[11px]"
            placeholder="Name (e.g., Security Reviewer)"
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
          />
          <input
            className="w-full bg-background border rounded px-2 py-1 text-[11px]"
            placeholder="Description"
            value={form.description}
            onChange={(e) => setForm({ ...form, description: e.target.value })}
          />
          <select
            className="w-full bg-background border rounded px-2 py-1 text-[11px]"
            value={form.scope}
            onChange={(e) => setForm({ ...form, scope: e.target.value as ProfileScope })}
          >
            <option value="repo">This repo only</option>
            <option value="workspace">Whole workspace</option>
          </select>
          <textarea
            className="w-full bg-background border rounded px-2 py-1 text-[11px] font-mono min-h-[60px]"
            placeholder="System prompt"
            value={form.system_prompt}
            onChange={(e) => setForm({ ...form, system_prompt: e.target.value })}
          />
          <div className="grid grid-cols-2 gap-1.5">
            <input
              className="bg-background border rounded px-2 py-1 text-[11px]"
              placeholder="Model provider (optional)"
              value={form.model_provider}
              onChange={(e) => setForm({ ...form, model_provider: e.target.value })}
            />
            <input
              className="bg-background border rounded px-2 py-1 text-[11px]"
              placeholder="Model id (optional)"
              value={form.model_id}
              onChange={(e) => setForm({ ...form, model_id: e.target.value })}
            />
          </div>
          <div className="flex flex-wrap gap-2">
            {ALL_TOOLS.map((tool) => (
              <label key={tool} className="flex items-center gap-1">
                <input type="checkbox" checked={form.allowed_tools.includes(tool)} onChange={() => toggleTool(tool)} />
                {tool}
              </label>
            ))}
          </div>
          <div className="flex gap-2">
            <button
              onClick={submit}
              disabled={!form.name.trim()}
              className="px-3 py-1 rounded bg-primary text-primary-foreground disabled:opacity-50"
            >
              {editingId ? "Save" : "Create"}
            </button>
            <button onClick={() => setShowForm(false)} className="px-3 py-1 rounded bg-secondary">
              Cancel
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
