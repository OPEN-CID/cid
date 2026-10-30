import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, fireEvent, act, within } from "@testing-library/react";
import { AuctionsPanel } from "./AuctionsPanel";
import { api } from "@/lib/api";

vi.mock("@/lib/api", () => ({
  api: {
    auction: {
      sources: vi.fn(),
      refresh: vi.fn(),
      search: vi.fn(),
      crosscheck: vi.fn(),
    },
  },
}));

const sourcesPayload = {
  official: [
    {
      id: "bankeauctions-c1",
      name: "BankAuctions (C1 India)",
      operator: "C1 India Pvt Ltd (bank-designated venue)",
      category: "bank_auction",
      base_url: "https://bankeauctions.com",
      listing_path: "/",
      tier: "primary",
      access: "html_listing",
      listing_feed: true,
      notes: "Designated venue for 300+ banks.",
    },
    {
      id: "baanknet",
      name: "BaankNet",
      operator: "PSB Alliance",
      category: "bank_auction",
      base_url: "https://baanknet.com",
      listing_path: "/",
      tier: "primary",
      access: "js_app",
      listing_feed: true,
      notes: "React SPA.",
    },
    {
      id: "ibbi",
      name: "IBBI",
      operator: "IBBI",
      category: "authority_ref",
      base_url: "https://ibbi.gov.in",
      listing_path: "/",
      tier: "primary",
      access: "html_listing",
      listing_feed: false,
      notes: "Orders only.",
    },
  ],
  crosscheck: [
    {
      id: "bidbazaar",
      name: "BidBazaar",
      operator: "private aggregator",
      category: "bank_auction",
      base_url: "https://bidbazaar.co",
      listing_path: "/browse",
      tier: "cross_check",
      access: "html_listing",
      listing_feed: false,
      notes: "Corroboration only.",
    },
  ],
};

const listingsPayload = [
  {
    id: "bankeauctions-c1:AUC123",
    source_id: "bankeauctions-c1",
    external_id: "AUC123",
    title: "Individual House in Jaipur",
    url: "https://bankeauctions.com/auction/123",
    bank: "State Bank of India",
    location: "Jaipur",
    reserve_price: "Rs. 2.05 Cr",
    emd: null,
    auction_start: null,
    notice_url: "https://bankeauctions.com/notice/1.pdf",
    origin: "primary",
    source_url: "https://bankeauctions.com/",
    fetched_at: "2026-01-01T00:00:00Z",
  },
];

async function flushAsyncWork() {
  await act(async () => {
    for (let i = 0; i < 6; i++) {
      await Promise.resolve();
    }
  });
}

describe("AuctionsPanel", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.mocked(api.auction.sources).mockReset().mockResolvedValue(sourcesPayload);
    vi.mocked(api.auction.refresh).mockReset();
    vi.mocked(api.auction.search).mockReset().mockResolvedValue([]);
    vi.mocked(api.auction.crosscheck).mockReset();
  });

  it("renders primary sources with access badges and the cross-check section separately", async () => {
    render(<AuctionsPanel />);
    await flushAsyncWork();

    expect(screen.getByText("BankAuctions (C1 India)")).toBeInTheDocument();
    expect(screen.getByText("polls today")).toBeInTheDocument();
    expect(screen.getByText("needs renderer")).toBeInTheDocument();
    expect(screen.getByText("authority ref — no feed")).toBeInTheDocument();
    // Third parties are labeled as corroboration-only, never as feeds.
    expect(screen.getByText("BidBazaar")).toBeInTheDocument();
    expect(screen.getByText("cross-check")).toBeInTheDocument();
  });

  it("polling shows per-source outcomes including named gaps", async () => {
    vi.mocked(api.auction.refresh).mockResolvedValueOnce([
      { source_id: "bankeauctions-c1", fetched_at: "t", listing_count: 3, note: null },
      { source_id: "baanknet", fetched_at: "t", listing_count: 0, note: "JS app; needs a browser renderer" },
    ]);
    render(<AuctionsPanel />);
    await flushAsyncWork();

    fireEvent.click(screen.getByRole("button", { name: /Poll now/ }));
    await flushAsyncWork();

    expect(await screen.findByText("3 listings")).toBeInTheDocument();
    expect(screen.getByText(/needs a browser renderer/)).toBeInTheDocument();
  });

  it("search renders listings with provenance and the sale-notice link", async () => {
    vi.mocked(api.auction.search).mockResolvedValueOnce(listingsPayload);
    render(<AuctionsPanel />);
    await flushAsyncWork();

    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    await flushAsyncWork();

    // Once in the row, once in the cross-check picker — both wirings pinned.
    const titles = await screen.findAllByText("Individual House in Jaipur");
    expect(titles).toHaveLength(2);
    const row = titles[0].closest("div");
    expect(row).not.toBeNull();
    expect(within(row as HTMLElement).getByText("primary")).toBeInTheDocument();
    expect(screen.getByText(/State Bank of India/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /sale notice/ })).toHaveAttribute(
      "href",
      "https://bankeauctions.com/notice/1.pdf"
    );
  });

  it("cross-check shows the per-field report without touching the record", async () => {
    vi.mocked(api.auction.search).mockResolvedValueOnce(listingsPayload);
    vi.mocked(api.auction.crosscheck).mockResolvedValueOnce({
      all_match: true,
      note: "2/2 comparable fields agree",
      fields: [
        { field: "title", primary: "Individual House in Jaipur", observed: "Individual House in Jaipur", matches: true },
      ],
    });
    render(<AuctionsPanel />);
    await flushAsyncWork();

    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    await flushAsyncWork();

    fireEvent.change(screen.getByLabelText("Listing to verify"), {
      target: { value: "bankeauctions-c1:AUC123" },
    });
    fireEvent.change(screen.getByPlaceholderText(/third-party/), {
      target: { value: "https://bidbazaar.co/p/1137104" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Verify" }));
    await flushAsyncWork();

    expect(vi.mocked(api.auction.crosscheck)).toHaveBeenCalledWith(
      "bankeauctions-c1:AUC123",
      "https://bidbazaar.co/p/1137104"
    );
    expect(await screen.findByText("2/2 comparable fields agree")).toBeInTheDocument();
  });
});
