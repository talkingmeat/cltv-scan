# Phase 3 — Security Analysis: Pattern Detection & Attack Heuristics

## Context

Phase 1 extracts timelocks. Phase 2 identifies Lightning transactions. Phase 3 is where the tool becomes a security analyzer: it takes the data from the first two phases and applies detection logic for known attack vectors and dangerous configurations. This is the core differentiator of the project — no existing tool does this. Block explorers show timelock fields, Miniscript compilers check script correctness, but nothing scans live blockchain data for patterns that indicate active attacks or vulnerable configurations.

The detection logic in this phase is heuristic, not deterministic. None of these checks can definitively prove an attack is occurring — they identify conditions that are consistent with documented attack setups or configurations that are known to be dangerous. The value is in surfacing things that deserve investigation, not in making accusations.

---

## Goal 1: Detect dangerous timelock mixing

This is the most concrete and unambiguous vulnerability the tool can detect. When a Bitcoin script contains timelocks that mix block heights and timestamps in the same spending path, the script can become permanently unspendable. This is not theoretical — it's a well-documented issue analyzed by Sanket Kanjalkar and Andrew Poelstra in Blockstream Research's "Don't Mix Your Timelocks" publication.

The underlying problem is a Bitcoin consensus rule: when a transaction has multiple timelocks, all absolute timelocks (nLockTime and CLTV) must use the same domain (all block heights or all timestamps), and all relative timelocks (nSequence and CSV) must also use the same domain within their category. If a script path requires satisfying a CLTV with a block height value AND a CSV with a time-based value (or vice versa), there is no valid transaction that can satisfy both simultaneously. The funds locked by such a script are permanently lost.

The detection needs to operate at two levels. First, within individual scripts: examine each decoded script for the presence of both CLTV and CSV opcodes, and check whether their respective values are in different domains (one height-based, one time-based). Second, across a transaction: check whether the transaction's nLockTime domain conflicts with any CLTV values in the scripts, or whether any input's nSequence domain conflicts with CSV values in the scripts being spent.

The rust-miniscript crate has a `TimelockInfo` struct that tracks exactly this — it records `csv_with_time`, `csv_with_height`, `cltv_with_time`, `cltv_with_height`, and a `contains_combination` flag that indicates the dangerous mix. If the project uses Miniscript for script analysis, this detection comes almost for free. If analyzing scripts directly from the decoded ASM strings, the logic needs to be implemented manually by checking the domain of each timelock value found.

When this mix is detected, the alert should be critical severity, because the consequence is total, permanent fund loss.

---

## Goal 2: Flag dangerously short CLTV deltas

In Lightning, the CLTV expiry delta is the safety margin a routing node has between when its outgoing HTLC expires and when its incoming HTLC expires. This window is the time the node has to claim funds on-chain if the channel partner becomes unresponsive. If this window is too short and the blockchain is congested, the node may not be able to get its transaction confirmed in time, leading to fund loss.

The BOLT specification recommends a minimum `cltv_expiry_delta` of 34 blocks (derived from the formula `3R + 2G + 2S`). Default values vary by implementation: LND uses 80 blocks, CLN uses 34, Eclair uses 144, LDK uses 36. The minimum `final_cltv_expiry_delta` for the last hop is 18 blocks. Approximately 91% of channels advertise a delta of 40 blocks (the legacy LND default).

The tool should analyze HTLC transactions identified in Phase 2 and calculate the effective remaining time: the difference between the CLTV expiry value embedded in the script (or in the nLockTime for HTLC-timeout transactions) and the current block height. Suggested thresholds are: fewer than 18 blocks remaining is critical (below the minimum final hop delta), fewer than 34 blocks is a warning (below the BOLT recommendation), and fewer than 72 blocks is informational (within a range where congestion could cause problems). These thresholds should be configurable.

This analysis also applies beyond Lightning: any transaction with a CLTV timelock that is close to expiring or has already expired should be flagged, because it means a time-sensitive spending condition is now active. For instance, a multisig wallet with a CLTV recovery path that has become active means the recovery key can now sweep the funds.

---

## Goal 3: Detect HTLC timeout clustering (flood-and-loot indicator)

The flood-and-loot attack, documented by Harris and Zohar in 2020, requires an attacker to force many HTLCs to timeout simultaneously. The resulting burst of HTLC-timeout transactions competing for block space creates enough congestion to prevent victims from claiming their HTLCs before the timelocks expire. The research showed that as few as 85 simultaneously attacked channels are sufficient for the attack to be profitable.

The observable on-chain signature is a concentration of HTLC-timeout transactions with CLTV expiry values clustered in a narrow range of block heights. Under normal conditions, HTLC timeouts are spread out across many different expiry heights because payments are independent. A sudden spike of many HTLCs all expiring within a 6-10 block window is abnormal and consistent with coordinated attack staging.

The detection should work by collecting the CLTV expiry values from all HTLC-timeout transactions identified (from Phase 2) within a scanning window (the most recent N blocks, or the current mempool). It then counts how many expirations fall within each block height or within sliding windows of configurable size (e.g., 6 blocks). If the count in any window exceeds a threshold (configurable, with the research's 85 as a reference point), an alert is raised.

This does not need to be a perfect flood-and-loot detector. Even a simpler metric — "there are N HTLC-timeout transactions in the mempool, which is X% above the rolling average" — is valuable information that currently no tool surfaces. The point is to make this pattern visible, not to prove an attack.

---

## Goal 4: Identify anomalous nSequence patterns

Most Bitcoin transactions use one of three standard nSequence values: `0xFFFFFFFF` (final, disables locktime, no RBF), `0xFFFFFFFE` (enables locktime, no RBF), or `0xFFFFFFFD` (enables both locktime and RBF signaling). Any other value is non-standard and potentially encodes a BIP 68 relative timelock.

The tool should flag inputs with non-standard sequence values and classify what they mean. The most common non-standard sequences in practice come from Lightning (the 0x80 upper byte pattern from commitment transactions, and CSV-enforced values like 144 blocks from `to_self_delay`). Beyond those known patterns, the tool should flag: very short relative timelocks (fewer than 6 blocks), which could indicate an attempt to minimize revocation windows; very long relative timelocks (more than 1,000 blocks), which are unusual and might indicate either a specialized custody setup or a misconfiguration; and time-based relative timelocks (bit 22 set), which are rare in practice and deserve attention whenever they appear.

The goal is to surface anything that deviates from the norm. A sequence value that doesn't match any known pattern is interesting by definition and should be highlighted for investigation.

---

## Goal 5: Unified alert system with severity levels

All detections from goals 1 through 4 need to produce structured alerts that follow a consistent format. Each alert should contain: a unique identifier, a severity level (critical, warning, informational), the detection type (timelock mixing, short CLTV delta, HTLC clustering, anomalous sequence), the specific transaction ID and input index where the condition was found, a description of what was detected and why it matters, the raw data that triggered the detection (the specific timelock values, the cluster count, etc.), and a reference to the relevant attack vector with its source (paper author, year, and URL where applicable).

This alert structure serves three purposes: it feeds the CLI output (Phase 1/2 CLI can be extended with a "scan for vulnerabilities" command), it feeds the HTTP API response (Phase 4's server will serialize these as JSON), and it feeds the dashboard display (Phase 5 will render these in the UI).

The CLI should gain a new command: given a block height range or "latest N blocks," scan all transactions, run all detection heuristics, and output all alerts sorted by severity. This is the "security scan" that represents the core product.

---

## What "done" looks like

The tool can scan a range of blocks and produce a list of security-relevant findings: scripts with dangerous timelock mixing, HTLCs with dangerously short remaining time, clusters of HTLC timeouts concentrated in narrow block ranges, and inputs with anomalous sequence values. Each finding is classified by severity and annotated with the relevant attack context. The CLI can run a full security scan on recent blocks and output a clear report. The alert structures are defined and ready to be serialized as JSON for the HTTP API.

---

## Important note on thresholds

All thresholds in this phase (CLTV delta minimums, clustering window size, clustering count threshold, sequence anomaly ranges) should be configurable, both in the CLI (as arguments or a config file) and in the eventual HTTP API (as query parameters). Reasonable defaults should be provided based on the research and BOLT specifications, but different users may want to adjust sensitivity. A node operator running a high-value routing node might want stricter thresholds than a researcher doing a broad survey.
