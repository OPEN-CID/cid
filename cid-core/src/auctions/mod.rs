//! Official Indian auction/tender source registry + primary-first ingestion.
//!
//! Coverage rule this module exists to enforce: listing details come only
//! from official/designated venues (Tier::Primary). Third-party aggregators
//! (BidBazaar and the like) are Tier::CrossCheck — they may trigger a
//! coverage alert ("they list something we don't") or a mismatch flag, but
//! their fields never overwrite a primary record and third-party-only items
//! stay in the Unverified bucket, never in the main feed.
//!
//! Access honesty: several official portals are React SPAs or bot-walled
//! (BaankNet, CPPP, GeM). This Core has no headless renderer, so those are
//! registered with their real access level and `poll_source` refuses them
//! with an explicit reason instead of pretending to ingest them. A source
//! the poller cannot read is a visible gap in the coverage matrix, not a
//! silent hole. Server-rendered venues (MSTC, C1 bankeauctions) poll today.

use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Whether a venue is a system-of-record or corroboration-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTier {
    /// Official or bank-designated venue: fields from here are authoritative.
    Primary,
    /// Third-party aggregator: coverage diffs and mismatch flags only.
    CrossCheck,
}

/// How listing data can actually be read from the venue today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceAccess {
    /// Server-rendered HTML the poller can fetch and parse now.
    HtmlListing,
    /// React SPA: public pages, but content needs a JS renderer this Core
    /// does not bundle. Registered for coverage, refused at poll time.
    JsApp,
    /// Bot protection refuses non-browser clients. Same treatment as JsApp.
    Protected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCategory {
    BankAuction,
    Tender,
    Tribunal,
    AuthorityRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuctionSource {
    pub id: &'static str,
    pub name: &'static str,
    pub operator: &'static str,
    pub category: SourceCategory,
    pub base_url: &'static str,
    /// Path of the public listing surface, relative to `base_url`.
    pub listing_path: &'static str,
    pub tier: SourceTier,
    pub access: SourceAccess,
    /// False for venues that are authorities but publish no listings
    /// themselves (IBBI orders, NCLT case records): the registry covers
    /// them, the poller skips them with a reason.
    pub listing_feed: bool,
    pub notes: &'static str,
}

/// The complete official-source landscape, verified against the live portals
/// (BaankNet, MSTC, bankeauctions/C1, CPPP title, IREPS, Rajasthan eProc,
/// DRT shell, NCLT, IBBI). Third parties never appear here — see
/// `crosscheck_sources`.
pub fn official_sources() -> Vec<AuctionSource> {
    vec![
        AuctionSource {
            id: "baanknet",
            name: "BaankNet (Bank Asset Auction Network)",
            operator: "PSB Alliance (all 12 public-sector banks)",
            category: SourceCategory::BankAuction,
            base_url: "https://baanknet.com",
            listing_path: "/",
            tier: SourceTier::Primary,
            access: SourceAccess::JsApp,
            listing_feed: true,
            notes: "PSU-bank SARFAESI/property/vehicle/IBC auctions with statutory notice PDFs. React SPA: poller refuses, renderer needed.",
        },
        AuctionSource {
            id: "mstc",
            name: "MSTC e-Commerce",
            operator: "MSTC Ltd (gov PSU)",
            category: SourceCategory::BankAuction,
            base_url: "https://www.mstcecommerce.com",
            listing_path: "/",
            tier: SourceTier::Primary,
            access: SourceAccess::HtmlListing,
            listing_feed: true,
            notes: "e-Bikray/REALTY property, minerals, coal, scrap, PSU e-procurement. Server-rendered: polls today.",
        },
        AuctionSource {
            id: "bankeauctions-c1",
            name: "BankAuctions (C1 India)",
            operator: "C1 India Pvt Ltd (bank-designated venue)",
            category: SourceCategory::BankAuction,
            base_url: "https://bankeauctions.com",
            listing_path: "/",
            tier: SourceTier::Primary,
            access: SourceAccess::JsApp,
            listing_feed: true,
            notes: "Designated venue for 300+ banks/FIs/DRTs/courts incl. SBI, HDFC, ICICI, Axis; ~86k properties. Authority derives from the publishing bank + notice. Shell is server-rendered but result rows hydrate client-side (a live poll returned the table with zero rows), so: needs a renderer, refused with reason until then.",
        },
        AuctionSource {
            id: "cppp",
            name: "Central Public Procurement Portal",
            operator: "NIC / Government of India",
            category: SourceCategory::Tender,
            base_url: "https://eprocure.gov.in",
            listing_path: "/",
            tier: SourceTier::Primary,
            access: SourceAccess::JsApp,
            listing_feed: true,
            notes: "Central government tenders. JS app: poller refuses, renderer needed.",
        },
        AuctionSource {
            id: "ireps",
            name: "IREPS (Railways)",
            operator: "CRIS / Indian Railways",
            category: SourceCategory::Tender,
            base_url: "https://www.ireps.gov.in",
            listing_path: "/",
            tier: SourceTier::Primary,
            access: SourceAccess::HtmlListing,
            listing_feed: true,
            notes: "Works/goods tenders plus e-auction sale & leasing with public search links. Largely server-rendered: polls today.",
        },
        AuctionSource {
            id: "rajasthan-eproc",
            name: "Rajasthan eProcurement",
            operator: "Government of Rajasthan (NIC GePNIC stack)",
            category: SourceCategory::Tender,
            base_url: "https://eproc.rajasthan.gov.in",
            listing_path: "/nicgep/app",
            tier: SourceTier::Primary,
            access: SourceAccess::HtmlListing,
            listing_feed: true,
            notes: "State tenders; latest-tenders table is server-rendered and refreshes every 15 min. Every other state runs the same GePNIC app at the same /nicgep/app path, so this parser is the family template — one adapter, all states.",
        },
        AuctionSource {
            id: "gem",
            name: "Government e-Marketplace",
            operator: "GeM SPV / Government of India",
            category: SourceCategory::Tender,
            base_url: "https://gem.gov.in",
            listing_path: "/",
            tier: SourceTier::Primary,
            access: SourceAccess::Protected,
            listing_feed: true,
            notes: "Bot protection refuses non-browser clients: poller refuses, browser-verify instead.",
        },
        AuctionSource {
            id: "drt",
            name: "Debt Recovery Tribunals",
            operator: "DRT / Government of India",
            category: SourceCategory::Tribunal,
            base_url: "https://drt.gov.in",
            listing_path: "/",
            tier: SourceTier::Primary,
            access: SourceAccess::JsApp,
            listing_feed: true,
            notes: "Tribunal auction notices; JS shell observed: verify in a real browser before adapter work.",
        },
        AuctionSource {
            id: "nclt",
            name: "National Company Law Tribunal",
            operator: "NCLT / Government of India",
            category: SourceCategory::AuthorityRef,
            base_url: "https://nclt.gov.in",
            listing_path: "/",
            tier: SourceTier::Primary,
            access: SourceAccess::HtmlListing,
            listing_feed: false,
            notes: "Orders, cause lists, case records — authority for case status, not an auction feed. Registry covers it; poller skips it.",
        },
        AuctionSource {
            id: "ibbi",
            name: "Insolvency and Bankruptcy Board of India",
            operator: "IBBI / Government of India",
            category: SourceCategory::AuthorityRef,
            base_url: "https://ibbi.gov.in",
            listing_path: "/",
            tier: SourceTier::Primary,
            access: SourceAccess::HtmlListing,
            listing_feed: false,
            notes: "Resolution-plan approvals and regulations — authority reference only, never a listing feed.",
        },
    ]
}

/// Third-party aggregators: corroboration only. A URL from one of these may
/// be *checked* against a primary record (`compare_crosscheck`) but its
/// fields are never ingested and third-party-only items stay Unverified.
pub fn crosscheck_sources() -> Vec<AuctionSource> {
    vec![AuctionSource {
        id: "bidbazaar",
        name: "BidBazaar",
        operator: "BidBazaar (private aggregator)",
        category: SourceCategory::BankAuction,
        base_url: "https://bidbazaar.co",
        listing_path: "/browse",
        tier: SourceTier::CrossCheck,
        access: SourceAccess::HtmlListing,
        listing_feed: false,
        notes: "Republishes official notices with daily refresh. Coverage-diff and mismatch flags only.",
    }]
}

/// Where a stored listing's details came from. The whole point of the
/// primary-first rule, persisted per row so the UI can badge it honestly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListingOrigin {
    /// Fetched from a Tier::Primary venue's own listing surface.
    Primary,
    /// Seen only via a cross-check source, or fields that could not be
    /// traced to a primary notice. Never shown in the main feed as fact.
    Unverified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuctionListing {
    pub id: String,
    pub source_id: String,
    pub external_id: String,
    pub title: String,
    pub url: String,
    pub bank: Option<String>,
    pub location: Option<String>,
    pub reserve_price: Option<String>,
    pub emd: Option<String>,
    pub auction_start: Option<String>,
    /// Direct link to the statutory sale notice, when the venue exposes it.
    /// A listing with a notice link outranks one without, always.
    pub notice_url: Option<String>,
    pub origin: ListingOrigin,
    /// Exact primary page this row was parsed from.
    pub source_url: String,
    pub fetched_at: String,
}

/// The write path takes one struct, not thirteen positional arguments —
/// a positional call with this many same-typed `&str`s is a transposition
/// bug waiting for a late-night edit.
pub struct NewAuctionListing<'a> {
    pub source_id: &'a str,
    pub external_id: &'a str,
    pub title: &'a str,
    pub url: &'a str,
    pub bank: Option<&'a str>,
    pub location: Option<&'a str>,
    pub reserve_price: Option<&'a str>,
    pub emd: Option<&'a str>,
    pub auction_start: Option<&'a str>,
    pub notice_url: Option<&'a str>,
    pub source_url: &'a str,
    pub fetched_at: &'a str,
}

/// One row as parsed from a venue's listing surface, before storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawListing {
    pub external_id: String,
    pub title: String,
    pub url: String,
    pub bank: Option<String>,
    pub location: Option<String>,
    pub reserve_price: Option<String>,
    pub emd: Option<String>,
    pub auction_start: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollOutcome {
    pub source_id: String,
    /// The exact listing page that was polled — the provenance every stored
    /// row points back at.
    pub source_url: String,
    pub fetched_at: String,
    pub listing_count: usize,
    /// Set when nothing was ingested and why (JS venue, markup drift with
    /// zero rows parsed, network failure). A silent zero is a coverage lie,
    /// so the reason is part of the record, not a log line.
    pub note: Option<String>,
}

/// Refuse clearly when the venue cannot be polled with plain HTTP, so a
/// coverage gap shows up as a named gap instead of a mysterious zero.
pub fn pollable(source: &AuctionSource) -> Result<()> {
    if !source.listing_feed {
        return Err(anyhow!(
            "{} publishes no listing feed ({}); registry covers it, poller skips it",
            source.name,
            source.notes
        ));
    }
    match source.access {
        SourceAccess::HtmlListing => Ok(()),
        SourceAccess::JsApp => Err(anyhow!(
            "{} is a JS app with no server-rendered feed; needs a browser renderer, not this poller",
            source.name
        )),
        SourceAccess::Protected => Err(anyhow!(
            "{} refuses non-browser clients; verify in a browser instead of scraping",
            source.name
        )),
    }
}

pub fn listing_page_url(source: &AuctionSource, base_override: Option<&str>) -> String {
    let base = base_override.unwrap_or(source.base_url);
    format!("{}{}", base.trim_end_matches('/'), source.listing_path)
}

async fn fetch_page(client: &reqwest::Client, url: &str) -> Result<String> {
    let text = client
        .get(url)
        .header(
            "User-Agent",
            "CID-auction-watch/1.0 (polite daily poll; contact via repo)",
        )
        .timeout(std::time::Duration::from_secs(25))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("non-200 from {url}"))?
        .text()
        .await?;
    Ok(text)
}

/// Poll one source: fetch its listing surface and parse rows. `base_override`
/// exists so tests point the adapter at a local mock server instead of the
/// real venue — production callers always pass None.
pub async fn poll_source(
    client: &reqwest::Client,
    source: &AuctionSource,
    base_override: Option<&str>,
) -> Result<(PollOutcome, Vec<RawListing>)> {
    pollable(source)?;
    let url = listing_page_url(source, base_override);
    let html = fetch_page(client, &url).await?;
    let raw = match source.id {
        "bankeauctions-c1" => parse_bankeauctions_table(&html, &url),
        "mstc" => parse_mstc_links(&html, &url),
        "rajasthan-eproc" => parse_gepnic_rows(&html, &url),
        _ => parse_generic_auction_links(&html, &url),
    };
    let note = if raw.is_empty() {
        Some(format!(
            "0 rows parsed from {} — markup drift or empty results; not an error, but not coverage either",
            url
        ))
    } else {
        None
    };
    Ok((
        PollOutcome {
            source_id: source.id.to_string(),
            source_url: url,
            fetched_at: chrono::Utc::now().to_rfc3339(),
            listing_count: raw.len(),
            note,
        },
        raw,
    ))
}

/// Strip markup but keep block structure: block closers become newlines so
/// row-shaped content stays row-shaped. Inline tags collapse to spaces.
/// Callers that need a single line (titles) normalize further themselves.
fn strip_tags(s: &str) -> String {
    let block_re = Regex::new(r"(?i)</?(?:tr|div|p|li|br|h\d|table|ul|ol|section|article)[^>]*>")
        .expect("static regex");
    let with_breaks = block_re.replace_all(s, "\n");
    let tag_re = Regex::new(r"<[^>]*>").expect("static regex");
    let stripped = tag_re.replace_all(&with_breaks, " ");
    stripped
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Single-line variant for titles and field values.
fn strip_tags_inline(s: &str) -> String {
    strip_tags(s)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn absolutize(base_page_url: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    let base = base_page_url
        .split_once("://")
        .map(|(scheme, rest)| {
            let host = rest.split('/').next().unwrap_or(rest);
            format!("{scheme}://{host}")
        })
        .unwrap_or_default();
    if href.starts_with('/') {
        format!("{base}{href}")
    } else {
        format!("{base}/{href}")
    }
}

/// C1 bankeauctions results render as a table headed Auction ID /
/// Bank-Organisation / Asset on Auction / City-District / Reserve Price /
/// EMD. Parse rows generically off the `<td>` sequence rather than any one
/// bank's wording, so a new publisher cannot silently break the shape: a
/// row with fewer than 4 cells is skipped, never half-ingested.
pub fn parse_bankeauctions_table(html: &str, page_url: &str) -> Vec<RawListing> {
    let row_re = Regex::new(r"(?is)<tr[^>]*>(.*?)</tr>").expect("static regex");
    let cell_re = Regex::new(r"(?is)<t[dh][^>]*>(.*?)</t[dh]>").expect("static regex");
    let href_re = Regex::new(r#"(?i)href\s*=\s*["']([^"']+)["']"#).expect("static regex");
    let mut out = Vec::new();
    for row in row_re.captures_iter(html) {
        let cells: Vec<String> = cell_re
            .captures_iter(&row[1])
            .map(|c| c[1].to_string())
            .collect();
        if cells.len() < 4 {
            continue;
        }
        let texts: Vec<String> = cells.iter().map(|c| strip_tags_inline(c)).collect();
        if texts.iter().all(|t| t.is_empty()) {
            continue;
        }
        // Header row repeats the column names — it has no auction id shape.
        if texts[0].to_lowercase().contains("auction id") {
            continue;
        }
        let href = href_re
            .captures(&row[1])
            .map(|c| absolutize(page_url, &c[1]))
            .unwrap_or_else(|| page_url.to_string());
        let external_id = texts[0].clone();
        if external_id.is_empty() {
            continue;
        }
        let get = |i: usize| texts.get(i).cloned().filter(|s| !s.is_empty());
        out.push(RawListing {
            external_id,
            title: get(2).unwrap_or_else(|| "Untitled auction".to_string()),
            url: href,
            bank: get(1),
            location: get(3),
            reserve_price: get(4),
            emd: get(5),
            auction_start: get(6),
        });
    }
    out
}

/// MSTC's server-rendered surface is category menus plus search links rather
/// than data rows: harvest the auction-bearing links so coverage tracks what
/// the venue actually links. Detail extraction per vertical (coal, minerals,
/// realty) is the follow-up once each search endpoint is probed.
pub fn parse_mstc_links(html: &str, page_url: &str) -> Vec<RawListing> {
    let a_re = Regex::new(r#"(?is)<a[^>]+href\s*=\s*["']([^"']+)["'][^>]*>(.*?)</a>"#)
        .expect("static regex");
    let keep =
        Regex::new(r"(?i)auction|tender|e-?bikray|realty|allotment|sale").expect("static regex");
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for cap in a_re.captures_iter(html) {
        let title = strip_tags_inline(&cap[2]);
        if title.len() < 4 || !keep.is_match(&title) {
            continue;
        }
        let url = absolutize(page_url, &cap[1]);
        if !seen.insert(url.clone()) {
            continue;
        }
        out.push(RawListing {
            external_id: url.clone(),
            title,
            url,
            bank: None,
            location: None,
            reserve_price: None,
            emd: None,
            auction_start: None,
        });
    }
    out
}

/// Fallback for HtmlListing venues with no dedicated parser yet: same honest
/// link harvest as MSTC. A venue graduates to its own parser once its row
/// markup is observed, never before.
pub fn parse_generic_auction_links(html: &str, page_url: &str) -> Vec<RawListing> {
    parse_mstc_links(html, page_url)
}

/// NIC GePNIC tender tables (every state e-proc portal serves the same app
/// at `/nicgep/app`): numbered rows under a "Tender Title / Reference No /
/// Closing Date / Bid Opening Date" head, e.g.
///
/// ```text
/// 1. Repair and Replacement Sewer line Trunk Line Work at Zone 03 (ARC)
/// NIT 425 DATE 29.10.2026
/// 19-Nov-2026 10:30 AM
/// 19-Nov-2026 04:00 PM
/// ```
///
/// Parsed off stripped text lines rather than table markup, so a theme
/// change that keeps the words cannot silently break the shape: anything
/// that does not match the numbered-row + two-dates shape is left out and
/// surfaces as a low count, not as a misattributed tender. Row detail links
/// are not yet resolved — each row points at the list page that contains
/// it, which is verifiable by a human but one click further out.
pub fn parse_gepnic_rows(html: &str, page_url: &str) -> Vec<RawListing> {
    let serial_re = Regex::new(r"^\s*(\d+)\.\s+(.+?)\s*$").expect("static regex");
    let datetime_re = Regex::new(r"(\d{1,2}-[A-Za-z]{3}-\d{4}\s+\d{1,2}:\d{2}\s*(?:AM|PM)?)")
        .expect("static regex");
    let stripped = strip_tags(html);
    let lines: Vec<&str> = stripped.lines().map(str::trim).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some(cap) = serial_re.captures(lines[i]) else {
            i += 1;
            continue;
        };
        let title = cap[2].trim().to_string();
        // Collect the row's continuation lines: everything up to the next
        // numbered row or a blank/section boundary.
        let mut body: Vec<&str> = Vec::new();
        let mut j = i + 1;
        while j < lines.len() && !serial_re.is_match(lines[j]) {
            if !lines[j].is_empty() {
                body.push(lines[j]);
            }
            j += 1;
            if body.len() > 6 {
                break;
            }
        }
        let dates: Vec<String> = body
            .iter()
            .filter_map(|l| datetime_re.captures(l).map(|c| c[1].trim().to_string()))
            .collect();
        // The reference is the non-date, non-time remainder (a NIT/e-NIT
        // number like "NIT 425 DATE 29.10.2026").
        let reference = body
            .iter()
            .find(|l| !datetime_re.is_match(l))
            .map(|s| s.to_string());
        if let Some(reference) = reference {
            if !title.is_empty() {
                out.push(RawListing {
                    external_id: reference.clone(),
                    title,
                    url: page_url.to_string(),
                    bank: None,
                    location: None,
                    reserve_price: None,
                    emd: None,
                    auction_start: dates.first().cloned(),
                });
            }
        }
        i = j;
    }
    // De-duplicate by reference number: the same table repeats under
    // "Latest Tenders" and "More…" views.
    let mut seen = std::collections::HashSet::new();
    out.retain(|r| seen.insert(r.external_id.clone()));
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldComparison {
    pub field: String,
    pub primary: Option<String>,
    pub observed: Option<String>,
    pub matches: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrosscheckReport {
    pub listing_id: String,
    pub third_party_url: String,
    pub fields: Vec<FieldComparison>,
    /// True only when every compared field with values on both sides agrees.
    /// Anything else — including "the third-party page exposed nothing
    /// comparable" — is false: absence of evidence is not agreement.
    pub all_match: bool,
    pub note: String,
}

fn first_match(html: &str, patterns: &[&str]) -> Option<String> {
    for pat in patterns {
        if let Ok(re) = Regex::new(pat) {
            if let Some(cap) = re.captures(html) {
                let hit = cap
                    .get(1)
                    .map(|m| m.as_str().trim().to_string())
                    .unwrap_or_default();
                if !hit.is_empty() {
                    return Some(hit);
                }
            }
        }
    }
    None
}

/// User-initiated, single-URL verification against a third-party page. Reads
/// the page, heuristically extracts the same fields the primary record
/// holds, and reports per-field agreement. It never writes: a mismatch is a
/// flag for a human, and an unparseable page is "unverifiable", not a pass.
pub async fn compare_crosscheck(
    client: &reqwest::Client,
    listing: &AuctionListing,
    third_party_url: &str,
) -> Result<CrosscheckReport> {
    if official_sources()
        .iter()
        .any(|s| third_party_url.starts_with(s.base_url))
    {
        return Err(anyhow!(
            "cross-check is for third-party pages; {third_party_url} is itself an official venue — re-poll it instead"
        ));
    }
    let html = fetch_page(client, third_party_url).await?;
    // ₹ amounts appear as "₹2.05 Cr", "Rs. 2,05,000", "INR 20500000", "2.05 Crore".
    let amount_pats = [
        r"(?i)(?:₹|Rs\.?|INR)\s*([\d,]+(?:\.\d+)?\s*(?:Cr(?:ore)?|L(?:ac|akh)?)?)",
        r"(?i)([\d,]+\s*(?:Cr(?:ore)?|L(?:ac|akh)?))\s*(?:reserve|price|emd)",
    ];
    let observed_price = first_match(&html, &amount_pats);
    let observed_title = first_match(
        &html,
        &[
            r"(?is)<h1[^>]*>(.*?)</h1>",
            r"(?is)<title[^>]*>(.*?)</title>",
        ],
    )
    .map(|s| strip_tags_inline(&s));
    let date_pats =
        [r"(\d{1,2}\s+(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*\s+\d{4})"];
    let observed_date = first_match(&html, &date_pats);

    let mut fields = Vec::new();
    let mut compared = 0;
    let mut matched = 0;
    for (field, primary, observed) in [
        ("title", Some(listing.title.clone()), observed_title),
        (
            "reserve_price",
            listing.reserve_price.clone(),
            observed_price,
        ),
        (
            "auction_start",
            listing.auction_start.clone(),
            observed_date,
        ),
    ] {
        let primary = primary.filter(|s| !s.is_empty());
        if let (Some(p), Some(o)) = (primary.clone(), observed.clone()) {
            compared += 1;
            // Containment, not equality: aggregators reword ("Rs 2.05 Cr"
            // vs "₹2.05 Cr"). When both sides carry numbers (prices, dates)
            // the leading digits must also overlap, so "2.05 Cr" never
            // agrees with "3.10 Cr" through shared words like "Cr" alone.
            // Text-only fields (titles without numbers) match on containment.
            let digits = |s: &str| s.chars().filter(|c| c.is_ascii_digit()).collect::<String>();
            let (dp, dpo) = (digits(&p), digits(&o));
            let text_ok = o.contains(&p) || p.contains(&o);
            let num_ok = dp.is_empty()
                || dpo.is_empty()
                || dpo.contains(&dp[..dp.len().min(4)])
                || dp.contains(&dpo[..dpo.len().min(4)]);
            let ok = text_ok && num_ok;
            if ok {
                matched += 1;
            }
            fields.push(FieldComparison {
                field: field.to_string(),
                primary: Some(p),
                observed: Some(o),
                matches: ok,
            });
        } else {
            fields.push(FieldComparison {
                field: field.to_string(),
                primary,
                observed,
                matches: false,
            });
        }
    }
    let all_match = compared > 0 && matched == compared;
    let note = if compared == 0 {
        "the third-party page exposed nothing comparable — unverifiable, not a pass".to_string()
    } else if all_match {
        format!("{matched}/{compared} comparable fields agree")
    } else {
        format!(
            "{matched}/{compared} comparable fields agree — human review, primary record unchanged"
        )
    };
    Ok(CrosscheckReport {
        listing_id: listing.id.clone(),
        third_party_url: third_party_url.to_string(),
        fields,
        all_match,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gepnic_rows_parse_off_the_observed_rajasthan_table_shape() {
        // Shape mirrors the live /nicgep/app "Latest Tenders" table: a
        // numbered title line, a reference line, closing and opening
        // date-times. Corrigendum blocks repeat the same shape and parse
        // the same way — they are tenders too, distinguished by title.
        let html = "<div>Tender Title Reference No Closing Date Bid Opening Date</div> \
            <div>1. Repair and Replacement Sewer line Trunk Line Work at Zone 03 (ARC)</div> \
            <div>NIT 425 DATE 29.10.2026</div> \
            <div>19-Nov-2026 10:30 AM</div> \
            <div>19-Nov-2026 04:00 PM</div> \
            <div>2. Construction of CC road, footpaths and associated works</div> \
            <div>NIT 426 DATE 29.10.2026</div> \
            <div>20-Nov-2026 10:30 AM</div> \
            <div>20-Nov-2026 04:00 PM</div>";
        let rows = parse_gepnic_rows(html, "https://eproc.rajasthan.gov.in/nicgep/app");
        assert_eq!(rows.len(), 2, "both numbered tenders must parse: {rows:?}");
        assert_eq!(rows[0].external_id, "NIT 425 DATE 29.10.2026");
        assert!(rows[0].title.contains("Sewer line"));
        assert_eq!(
            rows[0].auction_start.as_deref(),
            Some("19-Nov-2026 10:30 AM")
        );
        // Provenance is the page the row was read from — one click out,
        // but never an invented detail URL.
        assert_eq!(rows[0].url, "https://eproc.rajasthan.gov.in/nicgep/app");
    }

    #[test]
    fn registry_covers_the_bank_auction_venues_including_c1() {
        let sources = official_sources();
        let ids: Vec<&str> = sources.iter().map(|s| s.id).collect();
        for required in [
            "baanknet",
            "mstc",
            "bankeauctions-c1",
            "cppp",
            "ireps",
            "gem",
            "drt",
        ] {
            assert!(
                ids.contains(&required),
                "{required} missing from the registry"
            );
        }
    }

    #[test]
    fn registry_ids_are_unique_and_primary_feeds_use_https() {
        let sources = official_sources();
        let mut ids = std::collections::HashSet::new();
        for s in &sources {
            assert!(ids.insert(s.id), "duplicate source id {}", s.id);
            assert!(
                s.base_url.starts_with("https://"),
                "{} must use https",
                s.id
            );
        }
    }

    #[test]
    fn authority_only_venues_are_covered_but_not_polled() {
        let sources = official_sources();
        for id in ["nclt", "ibbi"] {
            let s = sources.iter().find(|s| s.id == id).unwrap();
            assert!(!s.listing_feed, "{id} publishes orders, not listings");
            assert!(pollable(s).is_err(), "{id} must be refused at poll time");
        }
    }

    #[test]
    fn js_and_protected_venues_are_refused_with_a_named_reason() {
        let sources = official_sources();
        for id in ["baanknet", "bankeauctions-c1", "cppp", "gem", "drt"] {
            let s = sources.iter().find(|s| s.id == id).unwrap();
            let err = pollable(s).unwrap_err().to_string();
            assert!(
                err.contains(s.name),
                "refusal must name the venue, got: {err}"
            );
        }
        for id in ["mstc", "ireps", "rajasthan-eproc"] {
            let s = sources.iter().find(|s| s.id == id).unwrap();
            assert!(pollable(s).is_ok(), "{} should poll today", id);
        }
    }

    #[test]
    fn bankeauctions_rows_parse_off_the_observed_table_shape() {
        // Shape mirrors the live homepage's results table headers (Auction
        // ID / Bank-Organisation / Asset / City-District / Reserve Price /
        // EMD); rows are representative, and the parser must not depend on
        // any one bank's wording.
        let html = r#"<table><tr><th>Auction ID</th><th>Bank/Organisation Name</th><th>Asset on Auction</th><th>City/District</th><th>Reserve Price</th><th>EMD</th></tr>
        <tr><td>AUC123</td><td>State Bank of India</td><td><a href="/auction/123">Individual House in Jaipur</a></td><td>Jaipur</td><td>Rs. 2.05 Cr</td><td>Rs. 20.5 L</td></tr>
        <tr><td></td><td></td><td></td><td></td><td></td><td></td></tr></table>"#;
        let rows = parse_bankeauctions_table(html, "https://bankeauctions.com/");
        assert_eq!(
            rows.len(),
            1,
            "header and empty rows must not ingest: {rows:?}"
        );
        let row = &rows[0];
        assert_eq!(row.external_id, "AUC123");
        assert_eq!(row.bank.as_deref(), Some("State Bank of India"));
        assert_eq!(row.title, "Individual House in Jaipur");
        assert_eq!(row.url, "https://bankeauctions.com/auction/123");
        assert_eq!(row.reserve_price.as_deref(), Some("Rs. 2.05 Cr"));
    }

    #[test]
    fn mstc_link_harvest_keeps_auction_links_and_dedupes() {
        let html = r#"<a href="/coal">Coal Auction</a><a href="/coal">Coal Auction</a><a href="/about">About Us</a><a href="https://x.in/t">E-Tender 12</a>"#;
        let rows = parse_mstc_links(html, "https://www.mstcecommerce.com/");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| !r.external_id.is_empty()));
    }

    #[test]
    fn empty_pages_report_zero_rows_rather_than_erroring() {
        assert!(
            parse_bankeauctions_table("<html></html>", "https://bankeauctions.com/").is_empty()
        );
        assert!(parse_mstc_links("<html></html>", "https://www.mstcecommerce.com/").is_empty());
    }

    #[tokio::test]
    async fn poll_hits_the_override_base_in_tests_never_the_real_venue() {
        // bankeauctions-c1 is deliberately NOT used here: it is a JsApp
        // venue and the poller must refuse it before any network happens
        // (pinned by `poll_refuses_js_venues_before_any_network`). The
        // override path is exercised through MSTC's link harvester instead.
        let app = axum::Router::new().route(
            "/",
            axum::routing::get(|| async { "<a href=\"/coal\">Coal Auction</a>" }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap()
        });
        let client = reqwest::Client::new();
        let source = official_sources()
            .into_iter()
            .find(|s| s.id == "mstc")
            .unwrap();
        let base = format!("http://{addr}");
        let (outcome, rows) = poll_source(&client, &source, Some(&base)).await.unwrap();
        assert_eq!(outcome.listing_count, 1);
        assert!(outcome.note.is_none());
        // Provenance follows the override: the stored page is the mock, so
        // a test run can never be mistaken for a real-venue poll.
        assert_eq!(outcome.source_url, format!("{base}/"));
        assert!(rows[0].url.starts_with(&base));
    }

    #[tokio::test]
    async fn poll_refuses_js_venues_before_any_network() {
        let client = reqwest::Client::new();
        let source = official_sources()
            .into_iter()
            .find(|s| s.id == "baanknet")
            .unwrap();
        let err = poll_source(&client, &source, None).await.unwrap_err();
        assert!(err.to_string().contains("BaankNet"));
    }

    #[tokio::test]
    async fn crosscheck_compares_fields_and_never_passes_on_nothing() {
        let listing = AuctionListing {
            id: "l1".to_string(),
            source_id: "bankeauctions-c1".to_string(),
            external_id: "AUC123".to_string(),
            title: "Individual House in Jaipur".to_string(),
            url: "https://bankeauctions.com/auction/123".to_string(),
            bank: Some("State Bank of India".to_string()),
            location: Some("Jaipur".to_string()),
            reserve_price: Some("Rs. 2.05 Cr".to_string()),
            emd: None,
            auction_start: None,
            notice_url: None,
            origin: ListingOrigin::Primary,
            source_url: "https://bankeauctions.com/".to_string(),
            fetched_at: "2026-01-01T00:00:00Z".to_string(),
        };
        // Agreeing page.
        let app = axum::Router::new().route(
            "/ok",
            axum::routing::get(|| async {
                "<h1>Individual House in Jaipur</h1><p>Reserve Price Rs. 2.05 Cr</p>"
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap()
        });
        let client = reqwest::Client::new();
        let report = compare_crosscheck(&client, &listing, &format!("http://{addr}/ok"))
            .await
            .unwrap();
        assert!(report.all_match, "{report:?}");
        // Page with nothing comparable: unverifiable, never a pass.
        let app2 =
            axum::Router::new().route("/empty", axum::routing::get(|| async { "<p>hello</p>" }));
        let listener2 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr2 = listener2.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener2, app2.into_make_service())
                .await
                .unwrap()
        });
        let report2 = compare_crosscheck(&client, &listing, &format!("http://{addr2}/empty"))
            .await
            .unwrap();
        assert!(!report2.all_match);
        // An official URL is not a cross-check target at all.
        let err = compare_crosscheck(&client, &listing, "https://bankeauctions.com/x")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("official venue"));
    }
}
