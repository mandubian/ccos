# Browser Automation Capability Plan (CCOS/RTFS)

Status: Draft  
Owner: CCOS/RTFS Core  
Scope: Enable “no-API” tasks (e.g., “search a restaurant in Paris”) via governed browser automation with user-in-the-loop for cookies/consent and other human requirements.

## 1) Why
- Many everyday goals lack stable/public APIs (maps, directories, booking sites).
- Current discovery (Marketplace → MCP → OpenAPI) is insufficient for these.
- We need a browser-backed, governed fallback with user visibility and control.

## 2) Goals
- Provide a first-class “browser capability family” with a small, robust action set.
- Support both external services (Apify/SerpAPI) and native Playwright.
- Keep strong governance: allowlist, rate limits, robots respect, user approvals.
- Integrate cleanly with CCOS/RTFS (effect boundary, causal chain, intent graph).

## 3) Capability Family (proposed interface)
All capability IDs under `web.*` namespace.

- web.open(url)
- web.fill(selector, text)
- web.click(selector)
- web.keypress(keys)
- web.wait({selector|network|time})
- web.scroll({top|bottom|by})
- web.extract(selector, schema, limit?) -> Vector<Map>  // returns structured items
- web.screenshot(path?, full_page?) -> Blob/Path
- web.cookies.get() -> List<Cookie>
- web.cookies.set(cookies)
- web.consent.handle(policy) -> {accepted: Bool, details}
- web.navigate(url) // alias of open, without new context
- web.close()

Notes:
- Selectors: CSS prioritized; allow XPath only if necessary.
- schema: map of keys to selectors, ex: {name: ".title", rating: ".rating", link: "a[href]"}.
- All functions must be idempotent or bounded with timeouts and retries.

## 4) Provider Implementations
- External providers (fastest path)
  - Apify Actors (e.g., Google Maps/Triadvisor, TheFork) as capabilities (HTTP bridge).
  - SerpAPI/SE-Scraper for search result extraction.
  - Pros: fast to production; structured; resilient; Cons: vendor dependency/cost.

- Native Playwright provider
  - Run Playwright/Chromium (headless) via a local Node.js sidecar.
  - Export a minimal JSON-RPC/MCP-compatible API that maps to our `web.*`.
  - Use persistent, sandboxed user-data-dir per session to maintain cookies if allowed.
  - Pros: full control; site profiles; Cons: Node runtime mgmt, anti-bot fragility.

- Playwright MCP
  - Existing MCP servers can be used, but they require a local Node/MCP server.
  - Treat as a provider variant with session lifecycle managed by CCOS SessionPool.

## 5) Architecture in CCOS/RTFS

- RTFS effect boundary
  - All browser actions are effects crossing the host boundary.
  - Capabilities must implement provider contract + governance hooks.
- Capability Marketplace
  - Register `web.*` manifests with ProviderType::{Local(RemoteBridge)|Remote(Service)}.
- Host/Sidecar
  - If native Playwright: launch a short-lived Node sidecar per “browser session” or reuse pool.
  - Communicate via JSON-RPC (stdin/stdout) or HTTP localhost.
- Governance & Causal Chain
  - Log each browser action (redact secrets).
  - Respect `agent_config.toml` egress/allowlist and rate limits.
  - Append structured action logs to causal chain for full traceability.

## 6) Cookie/Consent & Human-in-the-loop

- Consent policy
  - web.consent.handle(policy) will:
    - Auto-accept if policy says “auto” and selector rules are known for the site profile.
    - Otherwise prompt user with `:ccos.user.ask` and render the cookie banner text (if extracted).
- Human-kind checks
  - For login, 2FA, or sensitive forms, require explicit `:ccos.user.ask` confirmation.
  - For “Do you allow cookies?” or “This site uses location” prompts, ask or auto-accept per policy.
- Controls in `agent_config.toml`
  - `capabilities.browser.domains_allowlist = ["*.google.com","*.thefork.com","*.yelp.com"]`
  - `capabilities.browser.max_actions_per_minute`, `max_concurrent_sessions`
  - `capabilities.browser.auto_accept_cookies = false|true|per_site`
  - `capabilities.browser.headless = true|false` (dev vs prod)

## 7) Site Profiles (optional but recommended)

- A site profile describes known selectors and patterns per domain:
  - selectors: search_box, result_item, name, rating, address, link, pagination_next
  - consent: cookie_banner_selector, accept_button_selector
  - rate_limiter, polite delays, scroll strategy
- Store in `docs/site_profiles/*.json` to reduce LLM guesswork and improve reliability.

## 8) Example RTFS Plans

### 8.1 Restaurant search via Google Maps (headless)

```clojure
(do
  ;; Governance: user confirms browsing Google Maps
  (call :ccos.user.ask {:question "Open Google Maps to search restaurants in Paris?"})
  (call :web.open {:url "https://www.google.com/maps"})
  (call :web.consent.handle {:policy "auto_or_prompt"})
  (call :web.wait {:selector "input[aria-label='Search Google Maps']"})
  (call :web.fill {:selector "input[aria-label='Search Google Maps']" :text "restaurants in Paris"})
  (call :web.keypress {:keys ["Enter"]})
  (call :web.wait {:selector ".hfpxzc"})  ;; result item
  (call :web.scroll {:by 2000})
  (let [items (call :web.extract
                    {:selector ".hfpxzc"
                     :schema {:name ".qBF1Pd"
                              :rating ".MW4etd"
                              :address ".hP61id"
                              :link "a[href]"}
                     :limit 20})]
    (return items)))
```

### 8.2 TheFork search with site profile

```clojure
(do
  (call :ccos.user.ask {:question "Use TheFork to search restaurants in Paris?"})
  (call :web.open {:url "https://www.thefork.com"})
  (call :web.consent.handle {:policy "profile:thefork"})
  (call :web.wait {:selector "#search-input"})
  (call :web.fill {:selector "#search-input" :text "Paris"})
  (call :web.click {:selector "button[type='submit']"})
  (call :web.wait {:selector ".restaurant-card"})
  (let [items (call :web.extract
                    {:selector ".restaurant-card"
                     :schema {:name ".restaurant-name"
                              :rating ".rating-value"
                              :address ".address"
                              :link "a[href]"}
                     :limit 20})]
    (return items)))
```

## 9) DiscoveryEngine Integration

- New fallback after MCP/OpenAPI:
  - If capability need matches patterns like “search.*”, “browse.*”, “restaurant.search”, “local.business.search” etc, and discovery is NotFound/Incomplete:
    - Generate a web workflow intent (with allowed domains, site choices).
    - Produce a plan calling `web.*` actions and `:ccos.user.ask` for consent.
- IntentTransformer rule:
  - If the need implies browse/search and no API found, inject goal:
    - “Use an approved site (Google Maps/Yelp/TheFork) to search. Ask user to approve cookies/consent. Extract name/rating/address/link.”
  - Include site profiles in context (if any) to reduce selector synthesis.

## 10) Provider Integration Details

### 10.1 External (Apify/SerpAPI)
- Implement `web.search.providers.apify` capability:
  - Input: query, city, country, source (“google_maps” | “yelp” | “thefork”)
  - Output: list of structured places
- Governance: throttle, budget; domain allowlist not needed (HTTP-only).
- Fastest MVP; use when profiles not available or headless blocked.

### 10.2 Native Playwright Sidecar
- CCOS launches a Node.js sidecar:
  - Mode: per-session or pool with LRU reuse.
  - IPC: JSON-RPC over stdio or localhost HTTP with auth token.
  - Attestation: sign sidecar binary/path; provide hash in capability metadata.
  - Lifecycle: start on first `web.open`, stop after idle timeout or plan end.
- MCP option:
  - If Playwright MCP server is already running, CCOS connects via MCP client.
  - Session management via SessionPoolManager.

## 11) Governance & Security

- Egress allowlist (strict) and denylist.
- robots.txt respect (read-only crawl semantics; no mass scraping).
- Per-domain rate limiting; random jitter delays.
- Redaction: don’t log sensitive DOM text or PII in causal chain.
- Consent gates:
  - Always prompt for login/2FA and for irreversible actions.
- Cleanup:
  - Clear cookies/local storage unless user opted-in to persist per session.

## 12) Testing & Observability

- Unit tests:
  - Selector utility, schema extraction, retry policies.
- Integration tests (tagged, opt-in):
  - Headless navigation to a test page with stable DOM (our own demo page).
- Snapshot tests:
  - Optional screenshot diffs (store locally, not in repo).
- Logs:
  - Structured “Browser Plan” section with numbered steps and ✓/✗ outcomes.

## 13) Rollout Plan

- Phase A (2–3 days): External provider path
  - Add capabilities for Apify/SerpAPI Google Maps search.
  - Wire DiscoveryEngine fallback to this provider.
  - Governance: budget caps and usage notices.

- Phase B (3–5 days): Native Playwright MVP
  - Implement Node sidecar + `web.*` actions: open, wait, fill, click, extract, screenshot, cookies, consent.
  - Site profiles: Google Maps, TheFork.
  - Domain allowlist; timeouts; retries; structured logs.

- Phase C (1–2 weeks): Hardening
  - More site profiles; resilience patterns (scroll pagination, next buttons).
  - Consent auto-handling per site.
  - Visual debug page; improved error taxonomy.
  - Optional MCP integration for Playwright if desired.

## 14) Open Issues / Risks

- Anti-bot and dynamic content complexity; selectors may change.
- CAPTCHA/pathological consent flows; require user interaction or fallback to external provider.
- Terms of Service compliance; ensure only user-driven usage and honor robots.txt.
- Headful vs headless trade-offs; headful sometimes needed for anti-bot.
- Persistence: cookie reuse might be required for some flows; needs explicit user approval.

## 15) Example Capability signatures (conceptual)

- web.open
  - in: {url: String}
  - out: {ok: Bool}
- web.fill
  - in: {selector: String, text: String}
  - out: {ok: Bool}
- web.extract
  - in: {selector: String, schema: Map<String,String>, limit?: Number}
  - out: {items: Vector<Map<String,String>>}

(Full manifests to be added in implementation.)

---
Appendix: This plan complements existing discovery (MCP/OpenAPI). APIs.guru integration remains valuable for API-first cases but is not sufficient for “casual browsing” goals, which this browser capability family addresses.