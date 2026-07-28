import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api";
import { toast } from "@/lib/dialog";
import { Save } from "lucide-react";

const WORKSPACE_ID = "default";

type SlackForm = {
  webhook_url: string;
  signing_secret: string;
  bot_token: string;
  enabled: boolean;
  allowed_channels: string;
  default_channel: string;
  trigger_prefix: string;
};

type TeamsForm = {
  webhook_url: string;
  enabled: boolean;
  allowed_teams: string;
  allowed_channels: string;
  trigger_keywords: string;
};

const emptySlack: SlackForm = {
  webhook_url: "",
  signing_secret: "",
  bot_token: "",
  enabled: false,
  allowed_channels: "",
  default_channel: "",
  trigger_prefix: "/cid",
};

const emptyTeams: TeamsForm = {
  webhook_url: "",
  enabled: false,
  allowed_teams: "",
  allowed_channels: "",
  trigger_keywords: "cid",
};

const csv = (s: string) => s.split(",").map((v) => v.trim()).filter(Boolean);

// 051-Editor-Excellence-Roadmap.md Wave 5.1d: slack.configure/config.get and
// their Teams equivalents had no way to be set from anywhere but a
// hand-crafted RPC call. `trigger_mission` on both bridges stays unwired
// deliberately — it's the inbound path a real Slack/Teams event drives, not
// a user-facing settings action (same shape as deployment.webhook).
export function TeamIntegrationsPanel() {
  const [slack, setSlack] = useState<SlackForm>(emptySlack);
  const [teams, setTeams] = useState<TeamsForm>(emptyTeams);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [slackCfg, teamsCfg] = await Promise.all([
        api.slack.configGet(WORKSPACE_ID),
        api.teams.configGet(WORKSPACE_ID),
      ]);
      if (slackCfg && slackCfg.configured !== false) {
        setSlack({
          webhook_url: slackCfg.webhook_url || "",
          signing_secret: slackCfg.signing_secret || "",
          bot_token: slackCfg.bot_token || "",
          enabled: !!slackCfg.enabled,
          allowed_channels: (slackCfg.allowed_channels || []).join(", "),
          default_channel: slackCfg.default_channel || "",
          trigger_prefix: slackCfg.trigger_prefix || "/cid",
        });
      }
      if (teamsCfg && teamsCfg.configured !== false) {
        setTeams({
          webhook_url: teamsCfg.webhook_url || "",
          enabled: !!teamsCfg.enabled,
          allowed_teams: (teamsCfg.allowed_teams || []).join(", "),
          allowed_channels: (teamsCfg.allowed_channels || []).join(", "),
          trigger_keywords: (teamsCfg.trigger_keywords || []).join(", "),
        });
      }
    } catch (e) {
      toast.error(`Failed to load Slack/Teams config: ${e}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const saveSlack = async () => {
    try {
      await api.slack.configure({
        workspace_id: WORKSPACE_ID,
        webhook_url: slack.webhook_url.trim(),
        signing_secret: slack.signing_secret.trim() || undefined,
        bot_token: slack.bot_token.trim() || undefined,
        enabled: slack.enabled,
        allowed_channels: csv(slack.allowed_channels),
        default_channel: slack.default_channel.trim() || undefined,
        trigger_prefix: slack.trigger_prefix.trim() || undefined,
      });
      toast.success("Slack configuration saved");
    } catch (e) {
      toast.error(`Failed to save Slack configuration: ${e}`);
    }
  };

  const saveTeams = async () => {
    try {
      await api.teams.configure({
        workspace_id: WORKSPACE_ID,
        webhook_url: teams.webhook_url.trim(),
        enabled: teams.enabled,
        allowed_teams: csv(teams.allowed_teams),
        allowed_channels: csv(teams.allowed_channels),
        trigger_keywords: csv(teams.trigger_keywords),
      });
      toast.success("Teams configuration saved");
    } catch (e) {
      toast.error(`Failed to save Teams configuration: ${e}`);
    }
  };

  return (
    <div className="mt-4 space-y-4">
      <div className="text-xs font-medium">Chat integrations</div>
      {loading && <div className="text-[11px] text-muted-foreground">Loading…</div>}

      <div className="p-3 border rounded bg-background space-y-2">
        <div className="flex items-center justify-between">
          <div className="text-xs font-medium">Slack</div>
          <label className="flex items-center gap-1.5 text-[11px]">
            <input type="checkbox" checked={slack.enabled} onChange={(e) => setSlack({ ...slack, enabled: e.target.checked })} />
            Enabled
          </label>
        </div>
        <input
          className="w-full bg-background border rounded px-2 py-1 text-xs"
          placeholder="Webhook URL"
          value={slack.webhook_url}
          onChange={(e) => setSlack({ ...slack, webhook_url: e.target.value })}
        />
        <input
          type="password"
          className="w-full bg-background border rounded px-2 py-1 text-xs"
          placeholder="Bot token (xoxb-…)"
          value={slack.bot_token}
          onChange={(e) => setSlack({ ...slack, bot_token: e.target.value })}
        />
        <input
          type="password"
          className="w-full bg-background border rounded px-2 py-1 text-xs"
          placeholder="Signing secret"
          value={slack.signing_secret}
          onChange={(e) => setSlack({ ...slack, signing_secret: e.target.value })}
        />
        <div className="grid grid-cols-2 gap-2">
          <input
            className="bg-background border rounded px-2 py-1 text-xs"
            placeholder="Allowed channels (comma-separated)"
            value={slack.allowed_channels}
            onChange={(e) => setSlack({ ...slack, allowed_channels: e.target.value })}
          />
          <input
            className="bg-background border rounded px-2 py-1 text-xs"
            placeholder="Default channel"
            value={slack.default_channel}
            onChange={(e) => setSlack({ ...slack, default_channel: e.target.value })}
          />
        </div>
        <input
          className="w-full bg-background border rounded px-2 py-1 text-xs"
          placeholder="Trigger prefix"
          value={slack.trigger_prefix}
          onChange={(e) => setSlack({ ...slack, trigger_prefix: e.target.value })}
        />
        <button onClick={saveSlack} className="text-xs flex items-center gap-1 px-2 py-1 rounded bg-primary text-primary-foreground">
          <Save className="w-3 h-3" /> Save Slack config
        </button>
      </div>

      <div className="p-3 border rounded bg-background space-y-2">
        <div className="flex items-center justify-between">
          <div className="text-xs font-medium">Microsoft Teams</div>
          <label className="flex items-center gap-1.5 text-[11px]">
            <input type="checkbox" checked={teams.enabled} onChange={(e) => setTeams({ ...teams, enabled: e.target.checked })} />
            Enabled
          </label>
        </div>
        <input
          className="w-full bg-background border rounded px-2 py-1 text-xs"
          placeholder="Webhook URL"
          value={teams.webhook_url}
          onChange={(e) => setTeams({ ...teams, webhook_url: e.target.value })}
        />
        <div className="grid grid-cols-2 gap-2">
          <input
            className="bg-background border rounded px-2 py-1 text-xs"
            placeholder="Allowed teams (comma-separated)"
            value={teams.allowed_teams}
            onChange={(e) => setTeams({ ...teams, allowed_teams: e.target.value })}
          />
          <input
            className="bg-background border rounded px-2 py-1 text-xs"
            placeholder="Allowed channels (comma-separated)"
            value={teams.allowed_channels}
            onChange={(e) => setTeams({ ...teams, allowed_channels: e.target.value })}
          />
        </div>
        <input
          className="w-full bg-background border rounded px-2 py-1 text-xs"
          placeholder="Trigger keywords (comma-separated)"
          value={teams.trigger_keywords}
          onChange={(e) => setTeams({ ...teams, trigger_keywords: e.target.value })}
        />
        <button onClick={saveTeams} className="text-xs flex items-center gap-1 px-2 py-1 rounded bg-primary text-primary-foreground">
          <Save className="w-3 h-3" /> Save Teams config
        </button>
      </div>
    </div>
  );
}
