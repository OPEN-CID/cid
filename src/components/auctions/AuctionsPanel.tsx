import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api";
import { RefreshCw, ShieldCheck, ShieldAlert, ExternalLink } from "lucide-react";

type AuctionSource = {
  id: string;
  name: string;
  operator: string;
  category: string;
  base_url: string;
  listing_path: string;
  tier: "primary" | "cross_check";
  access: "html_listing" | "js_app" | "protected";
  listing_feed: boolean;
  notes: string;
};

type AuctionListing = {
  id: string;
  source_id: string;
  external_id: string;
  title: string;
  url: string;
  bank?: string | null;
  location?: string | null;
  reserve_price?: string | null;
  emd?: string | null;
  auction_start?: string | null;
  notice_url?: string | null;
  origin: "primary" | "unverified";
  source_url: string;
  fetched_at: string;
};

type PollOutcome = {
  source_id: string;
  fetched_at: string;
  listing_count: number;
  note?: string | null;
};

type FieldComparison = {
  field: string;
  primary?: string | null;
  observed?: string | null;
  matches: boolean;
};

const ACCESS_LABEL: Record<AuctionSource["access"], string> = {
  html_listing: "polls today",
  js_app: "needs renderer",
  protected: "browser only",
};

/**
 * Auction watch: official Indian auction/tender sources, polled primary-first.
 * Details come only from Tier::Primary venues (BaankNet, MSTC, C1
 * bankeauctions, CPPP, IREPS, state portals, DRT); third-party aggregators
 * are corroboration-only — coverage diffs and mismatch flags, never feeds.
 * Every listing carries its provenance (source + fetched-at); JS/protected
 * venues show as named gaps instead of silent zeros.
 */
export function AuctionsPanel() {
  const [sources, setSources] = useState<{ official: AuctionSource[]; crosscheck: AuctionSource[] } | null>(null);
  const [outcomes, setOutcomes] = useState<PollOutcome[] | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [query, setQuery] = useState("");
  const [listings, setListings] = useState<AuctionListing[]>([]);
  const [status, setStatus] = useState<string | null>(null);
  const [checkUrl, setCheckUrl] = useState("");
  const [checkListingId, setCheckListingId] = useState("");
  const [report, setReport] = useState<{ all_match: boolean; note: string; fields: FieldComparison[] } | null>(null);

  const loadSources = useCallback(async () => {
    try {
      setSources(await api.auction.sources());
    } catch (e) {
      setStatus(String(e));
    }
  }, []);

  useEffect(() => {
    loadSources();
  }, [loadSources]);

  const refresh = async () => {
    setRefreshing(true);
    setStatus(null);
    try {
      const res = (await api.auction.refresh()) as PollOutcome[];
      setOutcomes(res);
      setListings((await api.auction.search(query || undefined)) as AuctionListing[]);
    } catch (e) {
      setStatus(String(e));
    } finally {
      setRefreshing(false);
    }
  };

  const search = async () => {
    try {
      setListings((await api.auction.search(query || undefined)) as AuctionListing[]);
    } catch (e) {
      setStatus(String(e));
    }
  };

  const crosscheck = async () => {
    if (!checkListingId || !checkUrl.trim()) {
      setStatus("Pick a listing and paste a third-party URL to verify it against.");
      return;
    }
    try {
      setReport((await api.auction.crosscheck(checkListingId, checkUrl.trim())) as {
        all_match: boolean;
        note: string;
        fields: FieldComparison[];
      });
      setStatus(null);
    } catch (e) {
      setStatus(String(e));
    }
  };

  return (
    <div className="p-4 space-y-4 overflow-y-auto h-full text-xs">
      <div className="flex items-center justify-between">
        <div>
          <div className="font-medium">Auction watch — official sources only</div>
          <div className="text-[10px] text-muted-foreground">
            Details come from primary venues; third parties only corroborate.
          </div>
        </div>
        <button
          onClick={refresh}
          disabled={refreshing}
          className="flex items-center gap-1 px-2 py-1 rounded bg-primary text-primary-foreground disabled:opacity-50"
        >
          <RefreshCw className={`w-3 h-3 ${refreshing ? "animate-spin" : ""}`} />
          {refreshing ? "Polling…" : "Poll now"}
        </button>
      </div>

      {status && <div className="text-muted-foreground">{status}</div>}

      {outcomes && (
        <div className="border rounded bg-background p-2 space-y-1">
          <div className="font-medium">Last poll</div>
          {outcomes.map((o) => (
            <div key={o.source_id} className="flex gap-2 items-baseline">
              <span className="font-mono">{o.source_id}</span>
              <span>{o.listing_count} listings</span>
              {o.note && <span className="text-yellow-600 dark:text-yellow-400">{o.note}</span>}
            </div>
          ))}
        </div>
      )}

      <div>
        <div className="font-medium mb-1">Official sources ({sources?.official.length ?? 0})</div>
        <div className="space-y-1.5">
          {(sources?.official ?? []).map((s) => (
            <div key={s.id} className="border rounded p-2 bg-background">
              <div className="flex items-center gap-2">
                <span className="font-medium">{s.name}</span>
                <span className="text-[10px] px-1.5 py-0.5 rounded bg-green-500/20 text-green-600">primary</span>
                {!s.listing_feed ? (
                  <span className="text-[10px] px-1.5 py-0.5 rounded bg-secondary text-muted-foreground">
                    authority ref — no feed
                  </span>
                ) : (
                  <span
                    className={`text-[10px] px-1.5 py-0.5 rounded ${
                      s.access === "html_listing"
                        ? "bg-green-500/20 text-green-600"
                        : s.access === "js_app"
                        ? "bg-yellow-500/20 text-yellow-600"
                        : "bg-red-500/20 text-red-500"
                    }`}
                  >
                    {ACCESS_LABEL[s.access]}
                  </span>
                )}
              </div>
              <div className="text-[10px] text-muted-foreground mt-0.5">
                {s.operator} · {s.category.replace(/_/g, " ")}
              </div>
              <div className="text-[10px] text-muted-foreground">{s.notes}</div>
            </div>
          ))}
        </div>
      </div>

      <div>
        <div className="font-medium mb-1">
          Cross-check sources ({sources?.crosscheck.length ?? 0}) — corroboration only, never ingested
        </div>
        <div className="space-y-1.5">
          {(sources?.crosscheck ?? []).map((s) => (
            <div key={s.id} className="border rounded p-2 bg-background">
              <div className="flex items-center gap-2">
                <span className="font-medium">{s.name}</span>
                <span className="text-[10px] px-1.5 py-0.5 rounded bg-yellow-500/20 text-yellow-600">
                  cross-check
                </span>
              </div>
              <div className="text-[10px] text-muted-foreground">{s.notes}</div>
            </div>
          ))}
        </div>
      </div>

      <div>
        <div className="font-medium mb-1">Listings</div>
        <div className="flex gap-2 mb-2">
          <input
            className="flex-1 bg-background border rounded px-2 py-1 text-[11px]"
            placeholder="Search title, bank, location…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") search();
            }}
          />
          <button onClick={search} className="px-2 py-1 rounded bg-secondary">
            Search
          </button>
        </div>
        {listings.length === 0 ? (
          <div className="text-muted-foreground">No listings stored yet — poll first.</div>
        ) : (
          <div className="space-y-1.5">
            {listings.map((l) => (
              <div key={l.id} className="border rounded p-2 bg-background">
                <div className="flex items-center gap-2">
                  <span className="font-medium flex-1">{l.title}</span>
                  {l.origin === "primary" ? (
                    <span className="flex items-center gap-0.5 text-[10px] text-green-600">
                      <ShieldCheck className="w-3 h-3" /> primary
                    </span>
                  ) : (
                    <span className="flex items-center gap-0.5 text-[10px] text-yellow-600">
                      <ShieldAlert className="w-3 h-3" /> unverified
                    </span>
                  )}
                  <a
                    href={l.url}
                    target="_blank"
                    rel="noopener noreferrer"
                    aria-label={`Open ${l.title} at source`}
                    className="text-muted-foreground hover:text-foreground"
                  >
                    <ExternalLink className="w-3 h-3" />
                  </a>
                </div>
                <div className="text-[10px] text-muted-foreground mt-0.5">
                  {[l.bank, l.location, l.reserve_price].filter(Boolean).join(" · ")}
                </div>
                <div className="text-[10px] text-muted-foreground">
                  {l.source_id} · fetched {l.fetched_at}
                  {l.notice_url && (
                    <>
                      {" · "}
                      <a href={l.notice_url} target="_blank" rel="noopener noreferrer" className="underline">
                        sale notice
                      </a>
                    </>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      <div>
        <div className="font-medium mb-1">Cross-check a listing against a third-party page</div>
        <div className="text-[10px] text-muted-foreground mb-1">
          Compares fields only — a mismatch flags human review and never changes the primary record.
        </div>
        <div className="flex gap-2 mb-1">
          <select
            className="bg-background border rounded px-2 py-1 text-[11px] max-w-[40%]"
            value={checkListingId}
            onChange={(e) => setCheckListingId(e.target.value)}
            aria-label="Listing to verify"
          >
            <option value="">Pick a listing…</option>
            {listings.map((l) => (
              <option key={l.id} value={l.id}>
                {l.title}
              </option>
            ))}
          </select>
          <input
            className="flex-1 bg-background border rounded px-2 py-1 text-[11px] font-mono"
            placeholder="https://third-party.example/…"
            value={checkUrl}
            onChange={(e) => setCheckUrl(e.target.value)}
          />
          <button onClick={crosscheck} className="px-2 py-1 rounded bg-secondary">
            Verify
          </button>
        </div>
        {report && (
          <div className="border rounded p-2 bg-background space-y-1">
            <div className={report.all_match ? "text-green-600" : "text-yellow-600"}>{report.note}</div>
            {report.fields.map((f) => (
              <div key={f.field} className="text-[10px] font-mono">
                {f.matches ? "✓" : "✗"} {f.field}: {f.primary ?? "—"} {f.matches ? "≈" : "≠"} {f.observed ?? "—"}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
