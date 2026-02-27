# Phase 5 — Dashboard Frontend

## Context

Phases 1 through 4 built the complete backend: a Rust binary that fetches Bitcoin transactions, extracts timelocks, identifies Lightning transactions, detects security-relevant patterns, and exposes everything via an HTTP API. Phase 5 is the visual layer — a web dashboard that consumes the backend API and presents all of this information in a way that is immediately understandable and useful.

The dashboard is built using Shakespeare.diy, which generates React 18 / TypeScript / TailwindCSS / ShadCN UI applications from natural language descriptions. It runs entirely in the browser (client-side only), which is why the heavy lifting was done in the Rust backend — the frontend's job is purely to fetch data from the backend API, display it, and provide interactivity. If Shakespeare.diy hits a limitation, the fallback is to export the code and continue with any other AI tool or manual development in React.

The dashboard communicates exclusively with the Rust backend (Phase 4's HTTP server), never directly with mempool.space. This keeps the architecture clean and avoids browser-side rate limit issues with mempool.space.

---

## Goal 1: Main dashboard view — Alert feed

The first thing a user should see when they open the dashboard is the security state of recent blocks. This is the alert feed powered by Phase 4's security scan endpoint.

The view should show alerts from the most recent blocks (a configurable range, defaulting to something like the last 6 blocks or the last hour), sorted by severity with critical alerts at the top. Each alert card should display: a severity badge (visual color coding — red for critical, yellow for warning, blue for informational), the alert type (timelock mixing, short CLTV delta, HTLC clustering, anomalous sequence), the affected transaction ID (clickable, leading to the transaction detail view), a brief description of what was detected, and the relevant attack vector name.

If there are no alerts (which will be the common case during normal network conditions), the dashboard should clearly show "no security findings in the last N blocks" rather than appearing empty or broken. This is an important UX detail — the tool should feel useful even when nothing alarming is happening, because it's confirming that things are normal.

The alert feed should have filtering controls: by severity level (show only critical, only warnings, etc.), by detection type (show only HTLC clustering, only timelock mixing, etc.), and by block range (analyze a different set of blocks).

---

## Goal 2: Block explorer view — Timelock-focused

A view where the user can browse recent blocks and see the timelock analysis for each transaction. This is powered by Phase 4's block scanning endpoint.

The primary element is a transaction table showing the transactions in a selected block, filtered by default to only show transactions with at least one active timelock. The table should display for each transaction: the txid (truncated, with full txid on hover), the number and types of timelocks found (e.g., "2 CLTV, 1 CSV, nLockTime active"), the Lightning identification if applicable (commitment, HTLC-timeout, HTLC-success, or none), and a severity indicator if the transaction triggered any alerts.

Above the table, a block selector allows the user to navigate between blocks. This could be a simple height input, forward/back arrows for adjacent blocks, and a "latest" button.

The table should be sortable by different columns (number of timelocks, severity, Lightning type) and should support showing all transactions in a block versus only those with active timelocks.

---

## Goal 3: Transaction detail view

When a user clicks on a transaction (from the table, from the alert feed, or by entering a txid manually), they see the full timelock breakdown for that transaction. This is powered by Phase 4's transaction analysis endpoint.

The detail view should present, in a structured and readable layout:

**Transaction header:** The full txid (clickable link to mempool.space for cross-reference), the block height and confirmation count (or "unconfirmed" if in mempool), and the Lightning classification if applicable (with confidence level).

**nLockTime section:** The raw value, its classification (block height N, Unix timestamp formatted as a date, or zero/disabled), and whether it's active (at least one input has sequence ≠ 0xFFFFFFFF) or disabled.

**Inputs section:** A list of all inputs, each showing the raw sequence value in hex, the decoded BIP 68 interpretation (relative timelock in blocks or time, or "no relative timelock," or "RBF signaling"), and any CLTV or CSV opcodes found in the input's scripts with their values classified. If the script contains CLTV, show the timelock value and how it compares to the current block height (e.g., "CLTV at block 880,000 — expired 150 blocks ago" or "CLTV at block 881,000 — expires in 50 blocks"). Same for CSV values.

**Lightning details (if identified):** The transaction type (commitment, HTLC-timeout, HTLC-success), the extracted commitment number for commitment transactions, the CLTV expiry and remaining blocks for HTLC transactions, whether a preimage was revealed (for HTLC-success), and the CSV delays found.

**Security alerts:** Any alerts triggered by this specific transaction, displayed as inline warnings within the relevant section (e.g., a "timelock mixing" warning appears in the script section where the mixing was detected, a "short CLTV delta" warning appears next to the relevant CLTV value).

There should also be a text input prominently placed (in the header or as a dedicated "analyze" feature) where the user can paste any txid to load its detail view. This is essential for usability — someone hears about a suspicious transaction and wants to check it.

---

## Goal 4: Lightning activity view

A dedicated view showing Lightning Network force-close activity across recent blocks. This is powered by Phase 4's Lightning activity endpoint.

The main elements are:

**Summary statistics:** Total force-closes (commitment transactions) in the selected range, total HTLC-timeout and HTLC-success transactions, and any trend indicator (more or fewer than the previous equivalent period).

**HTLC expiry timeline:** A visualization showing upcoming HTLC expirations relative to the current block height. This could be a bar chart or histogram where the X axis is block height and the Y axis is the count of HTLCs expiring at each height. Clusters of expirations at the same height range should be visually obvious — this is the key flood-and-loot indicator. Use a chart library available in Shakespeare.diy (Recharts is well-supported).

**Lightning transaction list:** A filtered table showing only Lightning-identified transactions, with their type, confidence level, extracted parameters, and any related alerts.

---

## Goal 5: Attack reference panels

Each type of detection should have an associated contextual panel that explains the relevant attack vector. These panels are not the main content — they are supplementary context that appears when a user clicks "learn more" on an alert or views a specific detection type.

Each panel should contain: the attack name and a concise explanation (2-3 sentences on what it is and how it works), the key parameters (e.g., "85 channels for flood-and-loot, 2 hours for time-dilation"), the detection criteria the tool uses, and the source reference (paper author, year, and a link to the paper).

The nine attack vectors to cover are: flood-and-loot (Harris & Zohar, 2020), forced expiration spam (Poon & Dryja, 2016), time-dilation attacks (Riard & Naumenko, 2020), transaction pinning (Teinturier), replacement cycling (Riard, 2023), congestion attacks (Mizrahi & Zohar, 2020), channel jamming, timelocked bribing (Nadahalli et al., 2021), and the time warp attack. Not all of these have active detection heuristics in Phase 3, but having the reference panel makes the tool educational and comprehensive. The panels for attacks without active detection should note that they are included for reference and are not currently being scanned.

---

## Goal 6: Visual design and UX principles

The dashboard is a security tool, and its visual design should reflect that. Some principles to follow:

**Color coding for severity.** Red for critical findings, amber/yellow for warnings, blue for informational, green for normal/clear. These colors should be used consistently across all views — alert badges, table row highlights, chart segments.

**Monospace for technical data.** Transaction IDs, hex values, script opcodes, and block heights should all use a monospace font. This makes them visually distinct from descriptive text and easier to read and compare.

**Dense but readable.** Security tools benefit from density — the user wants to see a lot of information at once without excessive scrolling. But density shouldn't mean clutter. Use clear visual hierarchy (headings, spacing, card borders) to separate sections.

**External links.** Every transaction ID should link to the same transaction on mempool.space for cross-reference. Block heights should link to the block on mempool.space. Paper references should link to the actual papers. The tool should be a starting point for investigation, not a walled garden.

**Loading and error states.** Since the dashboard depends on the Rust backend which in turn depends on mempool.space, there are multiple points of failure. The dashboard should clearly indicate when data is loading, when the backend is unreachable, and when the backend reports errors (rate limits, network issues). These states should be handled gracefully with user-facing messages, not blank screens or cryptic errors.

---

## What "done" looks like

A web dashboard that connects to the Rust backend and provides four main views: an alert feed showing security findings, a block explorer focused on timelock analysis, a transaction detail view with complete timelock breakdown, and a Lightning activity view with an HTLC expiry timeline. The dashboard handles a manual txid input for on-demand analysis, includes contextual attack reference panels, and follows security-tool visual conventions. It is deployed via Shakespeare.diy's hosting (or exported and deployed elsewhere) and is usable by anyone with the backend running.
