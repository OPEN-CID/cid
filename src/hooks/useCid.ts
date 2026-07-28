import { create } from "zustand";
import { api } from "@/lib/api";

export type RepoChannel = {
  id: string;
  workspace_id: string;
  name: string;
  path: string;
  remote_url?: string;
  agents_md_content?: string;
  /** review_prompt.md §1.2: whether a human has reviewed this repo's
   * AGENTS.md. Until true, it's detected (agents_md_content above) but never
   * loaded into the model's system prompt. */
  agents_md_approved: boolean;
  created_at: string;
};

export type Mission = {
  id: string;
  repo_channel_id: string;
  title: string;
  task_description: string;
  session_mode: "worktree" | "shared";
  autonomy_level: "manual" | "co_pilot" | "autonomous";
  status: string;
  worktree_path?: string;
  branch_name?: string;
  created_at: string;
  updated_at: string;
};

export type ChatMessage = {
  id: string;
  mission_id: string;
  role: "user" | "assistant" | "system" | "tool";
  content: string;
  tool_calls: ToolCall[];
  created_at: string;
  is_streaming?: boolean;
};

export type ToolCall = {
  id: string;
  name: string;
  arguments: unknown;
  status: string;
  result?: unknown;
  requires_approval: boolean;
  approved?: boolean;
  /** review_prompt.md §1.2: set when this call was made while untrusted repo
   * content (an approved AGENTS.md/SKILL.md, or a prior file/diff/MCP read
   * this Mission) was present in context. */
  provenance?: string;
  /** Set for MCP tool calls, to route the result through McpAppCard. */
  server_id?: string;
};

/** `mission.tool_call.request` notification payload — see model/mod.rs. */
export type PendingApproval = {
  mission_id: string;
  tool_call_id: string;
  tool_name: string;
  arguments: unknown;
  requires_approval: boolean;
};

type CidState = {
  connected: boolean;
  repos: RepoChannel[];
  selectedRepoId: string | null;
  missions: Mission[];
  selectedMissionId: string | null;
  messages: Record<string, ChatMessage[]>;
  isCoreAvailable: boolean;
  // Neither is currently read anywhere — no consumer destructures them from
  // useCid() — so unknown costs nothing while still being honest that their
  // shape isn't modeled (051-Editor-Excellence-Roadmap.md Wave 2.1 scope).
  settings: unknown;
  mcpServers: unknown[];

  setConnected: (v: boolean) => void;
  loadRepos: () => Promise<void>;
  selectRepo: (id: string | null) => void;
  loadMissions: (repoId?: string) => Promise<void>;
  selectMission: (id: string | null) => void;
  loadMessages: (missionId: string) => Promise<void>;
  addMessage: (missionId: string, msg: ChatMessage) => void;
  updateMessage: (missionId: string, msgId: string, patch: Partial<ChatMessage>) => void;
};

export const useCid = create<CidState>((set, get) => ({
  connected: false,
  repos: [],
  selectedRepoId: null,
  missions: [],
  selectedMissionId: null,
  messages: {},
  isCoreAvailable: false,
  settings: null,
  mcpServers: [],

  setConnected: (v) => set({ connected: v }),

  loadRepos: async () => {
    try {
      const repos = await api.repo.list();
      set({ repos, isCoreAvailable: true });
    } catch (e) {
      console.warn("[CID] Failed to load repos, core might be offline", e);
      set({ isCoreAvailable: false });
    }
  },

  selectRepo: (id) => {
    set({ selectedRepoId: id, selectedMissionId: null });
    if (id) {
      get().loadMissions(id);
    }
  },

  loadMissions: async (repoId) => {
    const targetRepoId = repoId || get().selectedRepoId;
    if (!targetRepoId) return;
    try {
      const missions = await api.mission.list(targetRepoId);
      set({ missions });
    } catch (e) {
      console.error("Failed to load missions", e);
    }
  },

  selectMission: (id) => {
    set({ selectedMissionId: id });
    if (id) {
      get().loadMessages(id);
    }
  },

  loadMessages: async (missionId) => {
    try {
      const msgs = await api.message.list(missionId);
      set((s) => ({
        messages: { ...s.messages, [missionId]: msgs },
      }));
    } catch (e) {
      console.error("Failed to load messages", e);
    }
  },

  addMessage: (missionId, msg) => {
    set((s) => ({
      messages: {
        ...s.messages,
        [missionId]: [...(s.messages[missionId] || []), msg],
      },
    }));
  },

  updateMessage: (missionId, msgId, patch) => {
    set((s) => {
      const msgs = s.messages[missionId] || [];
      const idx = msgs.findIndex((m) => m.id === msgId);
      if (idx === -1) return s;
      const newMsgs = [...msgs];
      newMsgs[idx] = { ...newMsgs[idx], ...patch };
      return {
        messages: { ...s.messages, [missionId]: newMsgs },
      };
    });
  },
}));
