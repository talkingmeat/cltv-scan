# Timelock Attack Visualizer — Implementation Plan

## What this project is

A real-time Bitcoin timelock security analyzer that scans live blockchain transactions, extracts and classifies all four types of timelocks, identifies Lightning Network transactions on-chain, and detects patterns associated with known attack vectors. No tool like this currently exists in the Bitcoin ecosystem.

The architecture is a Rust backend (runnable as both a CLI tool and an HTTP server) that does all the heavy analytical work, paired with a web dashboard (built with Shakespeare.diy) that provides the visual interface. The data source is the mempool.space public API, with the architecture designed to support a future Bitcoin Core RPC adapter.

---

## Phase structure

The project is split into 5 phases. The first three build the Rust backend incrementally — each one adds a layer of capability on top of the previous one. Phase 4 wraps the backend in an HTTP server. Phase 5 builds the dashboard that consumes the API.

Each phase produces something functional and testable on its own. Phase 1 alone gives a CLI tool that can analyze any transaction's timelocks. Phases 1+2 add Lightning identification. Phases 1+2+3 add security detection. Phase 4 makes everything available over HTTP. Phase 5 makes it visual.

**Phase 1: Data Fetching & Timelock Extraction** — Rust project setup, mempool.space API integration with rate limit handling, parsing of all four timelock types (nLockTime, nSequence/BIP 68, OP_CLTV, OP_CSV) from real transactions, human-readable classification, and a minimal CLI to verify everything works. This is the foundation — nothing else functions without it.

**Phase 2: Lightning Network Identification** — Heuristic detection of Lightning commitment transactions (locktime encoding, sequence patterns, anchor outputs), classification of HTLC-timeout vs HTLC-success transactions (preimage presence, locktime patterns, script structures), and extraction of Lightning-specific parameters (commitment numbers, CLTV expiry values, CSV delays). Extends the CLI with Lightning-specific commands.

**Phase 3: Security Analysis** — Detection heuristics for known attack vectors: dangerous timelock mixing (funds permanently unspendable), dangerously short CLTV deltas (vulnerable to congestion), HTLC timeout clustering (flood-and-loot indicator), and anomalous nSequence patterns. Unified alert system with severity levels and attack references. Extends the CLI with a security scan command.

**Phase 4: HTTP Server API** — Wraps all backend capabilities in JSON API endpoints using axum (or equivalent). Adds CORS support for browser clients, an in-memory caching layer for mempool.space responses, and configuration management. The same binary runs as CLI or server via subcommands.

**Phase 5: Dashboard Frontend** — React/TypeScript web application built with Shakespeare.diy. Four main views: alert feed (security findings), block explorer (timelock-focused), transaction detail (full timelock breakdown), and Lightning activity (force-close tracking with HTLC expiry timeline). Contextual attack reference panels for the nine documented attack vectors.

---

## Dependency map

```
Phase 1 (extraction) ──→ Phase 2 (lightning) ──→ Phase 3 (security) ──→ Phase 4 (server) ──→ Phase 5 (dashboard)
```

The phases are sequential: each one depends on the previous. Phase 2 needs Phase 1's transaction data. Phase 3 needs Phase 2's Lightning identification for the most critical detections. Phase 4 needs Phase 3's alert system to expose. Phase 5 needs Phase 4's API to consume.

However, within phases, goals can be worked on in parallel by different team members once the shared foundations are in place.

---

## Key technical decisions

**Rust for the backend** because the `bitcoin` crate (rust-bitcoin) provides type-safe parsing of all timelock mechanisms, the `miniscript` crate adds timelock mixing detection, and the language's performance allows scanning large amounts of transaction data without bottlenecks. The ecosystem has the best Bitcoin-specific tooling of any language.

**mempool.space as data source** because its API returns decoded script fields (avoiding the need for full script decoding in the client), it supports CORS, it's well-documented, and it provides all four timelock-relevant fields. The rate limits are a constraint that the caching layer in Phase 4 mitigates. The architecture uses a trait-based data source interface so a Bitcoin Core RPC adapter can be added later without changing the analysis logic.

**Shakespeare.diy for the dashboard** because it enables rapid prototyping of React applications with ShadCN UI components, chart libraries (Recharts), and built-in deployment. If it hits limitations, the code can be exported and continued in any React development environment.

**CLI + server dual mode** because the CLI is essential for development and testing (no need to spin up a server to check if parsing works), while the server mode is essential for the dashboard. Both modes share the same core library — no logic duplication.

---

## Files in this plan

| File | Phase | Content |
|------|-------|---------|
| `phase-1-data-fetching-and-timelock-extraction.md` | 1 | API integration, timelock parsing, classification, CLI |
| `phase-2-lightning-identification.md` | 2 | Commitment tx fingerprinting, HTLC classification, parameter extraction |
| `phase-3-security-analysis.md` | 3 | Timelock mixing, short deltas, clustering, anomalies, alert system |
| `phase-4-http-server.md` | 4 | REST endpoints, CORS, caching, configuration, dual CLI/server binary |
| `phase-5-dashboard.md` | 5 | Alert feed, block explorer, transaction detail, Lightning view, attack references |

---

## TODO: Future Bitcoin Core RPC adapter

The data source module in Phase 1 defines a trait interface for fetching transactions. The mempool.space implementation fulfills this trait. A future phase (not currently planned in detail) would add a second implementation that connects to a local Bitcoin Core node via its JSON-RPC interface (`decoderawtransaction`, `getblock`, `getblockcount`, etc.). This would eliminate dependency on external APIs, remove all rate limit concerns, and enable scanning the full blockchain history. The analysis and detection layers would require zero changes — only the data source would differ.
