import { useEffect, useState } from "react";
import { api } from "@/lib/api";
import { toast, confirmDialog, showInfoDialog } from "@/lib/dialog";

type McpServer = {
  id: string;
  name: string;
  transport_type: string;
  status: string;
  enabled_for_repos: string[];
};

export function McpPanel() {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [showAdd, setShowAdd] = useState(false);
  const [newServer, setNewServer] = useState({ name: "", transport_type: "stdio", command: "", url: "" });

  const load = async () => {
    try {
      const list = await api.mcp.list();
      setServers(list);
    } catch (e) {
      console.error(e);
    }
  };

  useEffect(() => {
    load();
  }, []);

  const handleAdd = async () => {
    if (!newServer.name.trim()) return;
    try {
      const config =
        newServer.transport_type === "stdio"
          ? { command: newServer.command, args: [] }
          : { url: newServer.url };
      await api.mcp.add(newServer.name, newServer.transport_type, config);
      setShowAdd(false);
      setNewServer({ name: "", transport_type: "stdio", command: "", url: "" });
      load();
    } catch (e) {
      toast.error(`Failed to add server: ${e}`);
    }
  };

  const handleRemove = async (id: string) => {
    if (!(await confirmDialog("Remove this MCP server?"))) return;
    try {
      await api.mcp.remove(id);
      load();
    } catch (e) {
      toast.error(`Failed to remove server: ${e}`);
    }
  };

  return (
    <div className="p-4 space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="font-semibold text-sm">MCP Servers</h3>
        <button onClick={() => setShowAdd(!showAdd)} className="text-xs bg-primary text-primary-foreground px-2 py-1 rounded">
          Add Server
        </button>
      </div>

      <div className="text-xs text-muted-foreground">
        CID is an MCP client targeting 2026-07-28 spec (stateless core, Tasks, MCP Apps, OAuth). Add servers via UI (stdio or HTTP).
      </div>

      {showAdd && (
        <div className="border rounded p-3 space-y-2 bg-card">
          <input
            className="w-full bg-background border rounded px-2 py-1 text-sm"
            placeholder="Server name (e.g., postgres, filesystem)"
            value={newServer.name}
            onChange={(e) => setNewServer({ ...newServer, name: e.target.value })}
          />
          <select
            className="w-full bg-background border rounded px-2 py-1 text-sm"
            value={newServer.transport_type}
            onChange={(e) => setNewServer({ ...newServer, transport_type: e.target.value })}
          >
            <option value="stdio">stdio (local command)</option>
            <option value="http">HTTP (remote URL)</option>
          </select>
          {newServer.transport_type === "stdio" ? (
            <input
              className="w-full bg-background border rounded px-2 py-1 text-sm font-mono"
              placeholder="Command (e.g., npx -y @modelcontextprotocol/server-filesystem /tmp)"
              value={newServer.command}
              onChange={(e) => setNewServer({ ...newServer, command: e.target.value })}
            />
          ) : (
            <input
              className="w-full bg-background border rounded px-2 py-1 text-sm font-mono"
              placeholder="URL (e.g., http://localhost:3000/mcp)"
              value={newServer.url}
              onChange={(e) => setNewServer({ ...newServer, url: e.target.value })}
            />
          )}
          <div className="flex gap-2">
            <button onClick={handleAdd} className="bg-primary text-primary-foreground text-xs px-3 py-1 rounded">
              Add
            </button>
            <button onClick={() => setShowAdd(false)} className="bg-secondary text-xs px-3 py-1 rounded">
              Cancel
            </button>
          </div>
        </div>
      )}

      <div className="space-y-2">
        {servers.map((srv) => (
          <div key={srv.id} className="border rounded p-3 bg-card">
            <div className="flex items-center gap-2">
              <div className={`w-2 h-2 rounded-full ${srv.status === "connected" ? "bg-green-500" : srv.status === "error" ? "bg-red-500" : "bg-muted-foreground"}`} />
              <span className="font-medium text-sm">{srv.name}</span>
              <span className="text-[11px] bg-accent px-1 rounded">{srv.transport_type}</span>
              <span className="text-[11px] ml-auto">{srv.status}</span>
            </div>
            <div className="flex gap-2 mt-2">
              <button
                onClick={async () => {
                  try {
                    const tools = await api.mcp.tools(srv.id);
                    showInfoDialog(`Tools — ${srv.name}`, JSON.stringify(tools, null, 2));
                  } catch (e) {
                    toast.error(`Failed to list tools: ${e}`);
                  }
                }}
                className="text-[11px] bg-secondary px-2 py-1 rounded"
              >
                List Tools
              </button>
              <button onClick={() => handleRemove(srv.id)} className="text-[11px] bg-destructive/20 text-destructive px-2 py-1 rounded">
                Remove
              </button>
            </div>
          </div>
        ))}
        {servers.length === 0 && <div className="text-xs text-muted-foreground">No MCP servers configured. Add one above.</div>}
      </div>
    </div>
  );
}
