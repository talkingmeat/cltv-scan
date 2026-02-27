# Phase 4 — HTTP Server API

## Context

Phases 1 through 3 built the complete analytical engine: fetching transactions, extracting timelocks, identifying Lightning transactions, and detecting security-relevant patterns. All of this works via the CLI. Phase 4 wraps this engine in an HTTP server so the dashboard (Phase 5) can consume it.

The key architectural decision from the beginning was that the Rust binary should run both as a CLI tool and as an HTTP server. This phase adds the server mode. The same core library functions that the CLI commands call are now exposed as HTTP endpoints returning JSON. No analysis logic is duplicated — the server is purely a transport layer over the existing engine.

The suggested HTTP framework for Rust is `axum` (from the Tokio ecosystem, well-suited for async), though `actix-web` or `warp` are also viable. The choice should align with whatever the team is most comfortable with. The important thing is that the server is lightweight and simple — it's not the product, it's the bridge to the dashboard.

---

## Goal 1: Transaction analysis endpoint

An endpoint that accepts a transaction ID and returns the full timelock analysis for that transaction. This is the HTTP equivalent of Phase 1's "analyze a single transaction" CLI command.

The response should be a JSON object containing: the transaction ID, the nLockTime analysis (value, classification as blocks or timestamp, whether it's active or disabled), the per-input sequence analysis (each input's raw sequence value, whether it encodes a relative timelock, the decoded timelock if present), all CLTV occurrences found in scripts (which input, which script field, the threshold value, its classification), all CSV occurrences found in scripts (same structure), the Lightning identification from Phase 2 (whether it's a commitment, HTLC-timeout, or HTLC-success, with confidence level and extracted Lightning parameters), and any security alerts from Phase 3 triggered by this specific transaction.

This endpoint is what the dashboard will call when a user searches for a specific txid or clicks on a transaction in the table to see its details.

---

## Goal 2: Block scanning endpoint

An endpoint that accepts a block height (or "latest" for the most recent block) and returns the timelock analysis for all transactions in that block. This is the HTTP equivalent of Phase 1's "scan a block" CLI command.

The response should include the block metadata (height, hash, timestamp, number of transactions) and a list of analyzed transactions. Since most transactions in a block have no interesting timelocks, the endpoint should support a filter parameter to return only transactions that have at least one active timelock, or only transactions that triggered at least one alert. Without filtering, the response for a full block could be very large (thousands of transactions).

Pagination is also important since blocks can contain thousands of transactions. The endpoint should support offset and limit parameters so the dashboard can load results incrementally.

---

## Goal 3: Security scan endpoint

An endpoint that accepts a block range (start height to end height, or "latest N blocks") and returns all security alerts found across those blocks. This is the HTTP equivalent of Phase 3's security scan CLI command.

The response should be a list of alerts sorted by severity (critical first), each containing the alert structure defined in Phase 3: severity, detection type, transaction ID, description, raw data, and attack reference. The endpoint should support filtering by severity level and by detection type, so the dashboard can show "only critical alerts" or "only HTLC clustering alerts."

This is the endpoint that powers the dashboard's main "findings" or "alerts" view — the first thing a user sees when they want to know if anything suspicious is happening on-chain.

---

## Goal 4: Lightning activity endpoint

An endpoint that returns a summary of Lightning Network activity across a specified block range. This aggregates the Phase 2 identifications into a useful overview: number of force-closes (commitment transactions), number of HTLC resolutions (timeout and success separately), the distribution of CLTV expiry values for pending/recent HTLCs, and the list of all identified Lightning transactions with their extracted parameters.

This endpoint powers the dashboard's Lightning-specific view and feeds into the HTLC expiry timeline visualization.

---

## Goal 5: CORS and configuration

Since the dashboard runs in the browser (built with Shakespeare.diy, which is client-side only), the HTTP server must include proper CORS headers to allow cross-origin requests from the dashboard's domain. This means the `Access-Control-Allow-Origin` header needs to be set, along with the appropriate method and header permissions.

The server should accept configuration for: the port to listen on, the mempool.space API base URL (allowing the user to point at a self-hosted instance instead of the public API), CORS allowed origins, and the detection thresholds from Phase 3 (so they can be adjusted without recompiling).

Configuration should be possible via command-line arguments, environment variables, or a config file, with sensible defaults for all values. The CLI mode and server mode should be selected via a subcommand: something like `timelock-analyzer cli scan-block 880000` for CLI mode and `timelock-analyzer serve --port 3001` for server mode.

---

## Goal 6: Caching layer

The mempool.space API has undocumented rate limits, and the dashboard may make repeated requests for the same data (e.g., viewing the same block multiple times, or multiple users viewing the same transaction). The server should cache results from the mempool.space API to avoid redundant external calls.

The caching strategy can be simple: an in-memory cache (a HashMap behind a Mutex or RwLock, or a dedicated crate like `moka`) with a time-based expiration. Confirmed block data can be cached indefinitely (blocks don't change once confirmed). Transaction data for confirmed transactions can also be cached indefinitely. Mempool data (unconfirmed transactions) should have a short TTL since transactions can be replaced or confirmed at any time.

This cache sits between the data fetching module and the analysis module. When the server receives a request to analyze a transaction, it first checks the cache, and only calls mempool.space if the data isn't cached. This both improves response times for the dashboard and reduces the risk of hitting rate limits.

---

## What "done" looks like

The Rust binary has a `serve` subcommand that starts an HTTP server exposing all the analytical capabilities built in Phases 1-3 as JSON API endpoints. The server handles CORS for browser-based clients, caches mempool.space responses to minimize external API calls, and supports configuration via arguments or environment variables. The same binary can still be used as a CLI tool with the appropriate subcommand. The API is ready to be consumed by the dashboard.

---

## API design note

The endpoints described above are the minimum needed to power the dashboard. The exact paths, parameter names, and response shapes should be designed to be intuitive and consistent, but the specifics will be finalized during implementation when the dashboard's actual data needs become clear. The important thing at this stage is that every analytical capability (transaction analysis, block scanning, security alerts, Lightning identification) is available over HTTP with appropriate filtering and pagination.
