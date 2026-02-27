# Phase 1 — Data Fetching & Timelock Extraction

## Context

This is the foundation of the entire project. Before the tool can detect attacks, identify Lightning transactions, or display anything in a dashboard, it needs to do two things: get real Bitcoin transaction data from somewhere, and extract every timelock present in that data. Everything else builds on top of this.

The backend is a Rust binary that will eventually run both as a CLI tool and as an HTTP server, but in this phase the focus is purely on the core library logic and a minimal CLI interface to verify it works. The data source is the mempool.space public API, though the architecture should leave room for a future Bitcoin Core RPC adapter.

---

## Goal 1: Rust project structure

The project needs a Cargo workspace (or a single crate with clear module separation) that separates concerns from day one. The main boundaries are: a data source module responsible for fetching transactions from external APIs, a parsing/analysis module that takes raw transaction data and produces structured timelock information, and an output module that can format results for the CLI (and later for the HTTP API).

The reason for this separation is that the parsing logic should never depend on where the data comes from. The same extraction function that processes a transaction fetched from mempool.space should work identically on a transaction fetched from Bitcoin Core RPC or decoded from a raw hex string. This is what makes the future data source adapter possible without rewriting the core logic.

The key Rust crates to depend on are `bitcoin` (rust-bitcoin, for transaction deserialization and timelock types), `reqwest` (for HTTP calls to mempool.space), `serde` and `serde_json` (for deserializing API responses), and `tokio` (for async runtime, since reqwest is async). The `bitcoin` crate is particularly important because it provides type-safe representations of all four timelock mechanisms — using it means the project doesn't need to implement any low-level parsing of locktime or sequence fields from scratch.

---

## Goal 2: Fetch transactions from mempool.space API

The data source module needs to support three fetching operations that cover all the use cases the tool will need:

**Fetch a single transaction by txid.** This is the most basic operation: given a transaction ID, call `GET /api/tx/{txid}` and return the full transaction object. The API returns a JSON object with all the fields needed for timelock analysis: `locktime`, per-input `sequence`, decoded script fields (`scriptsig_asm`, `inner_redeemscript_asm`, `inner_witnessscript_asm`), witness data, and output values. The module should deserialize this into a Rust struct that captures all timelock-relevant fields.

**Fetch a single transaction as raw hex.** The endpoint `GET /api/tx/{txid}/hex` returns the raw serialized transaction. This is important because rust-bitcoin can deserialize raw transaction hex directly via `consensus::deserialize`, giving access to the strongly-typed `Transaction` struct with its `LockTime` and `Sequence` types. Having both the API JSON (with pre-decoded scripts) and the raw hex (with rust-bitcoin's type system) gives the analysis layer the best of both worlds.

**Fetch transactions from a block.** The endpoint `GET /api/block/{hash}/txs/{start_index}` returns 25 transactions per page from a given block. Combined with `GET /api/block-tip/height` (current tip height) and `GET /api/block-height/{height}` (hash for a given height), this allows scanning entire blocks. The module needs to handle pagination — a block may have thousands of transactions, requiring multiple paginated requests.

All three operations need to respect rate limits. The mempool.space API does not document its limits, but HTTP 429 responses indicate a violation. The fetching layer should implement a configurable delay between requests (starting with something conservative like 250ms), handle 429 responses by backing off and retrying, and provide clear error messages when rate limits are hit rather than failing silently. This rate limiting logic should live in the data source module so it applies to all fetching operations.

There should also be a clear trait or interface boundary between "fetch transaction data" and "the rest of the system." This trait would have methods like "get transaction by txid" and "get transactions from block at height." The mempool.space implementation fulfills this trait. Later, a Bitcoin Core RPC implementation would fulfill the same trait. This is the TODO that keeps the door open for the second data source.

---

## Goal 3: Parse and classify all four timelock types

This is the core analytical function of the entire project. Given a transaction (either as the API JSON struct or as a deserialized rust-bitcoin Transaction), extract every timelock present and classify it. There are four distinct sources of timelocks in a Bitcoin transaction, and the tool must check all of them.

**nLockTime (transaction-level absolute timelock).** Every transaction has a `locktime` field. The rust-bitcoin crate represents this as `absolute::LockTime`, an enum that automatically distinguishes between `Blocks(Height)` when the value is below 500,000,000 and `Seconds(Time)` when it's at or above that threshold. A value of 0 means no absolute timelock. However, nLockTime has a critical nuance: it is only enforced by consensus if at least one input in the transaction has a sequence number that is not `0xFFFFFFFF`. If all inputs have sequence `0xFFFFFFFF`, the locktime field is present but effectively ignored. The extraction must check this condition and report both the locktime value AND whether it is actually active. This is done via `Transaction::is_lock_time_enabled()` in rust-bitcoin, which checks exactly this condition.

**nSequence (per-input relative timelock, BIP 68).** Each transaction input has a 32-bit sequence number. BIP 68 defines how this field encodes relative timelocks when certain bits are set. The rust-bitcoin `Sequence` struct provides `to_relative_lock_time()` which returns `Some(relative::LockTime)` if the sequence number encodes a BIP 68 relative timelock, or `None` if it doesn't (bit 31 set means "no relative timelock"). When a relative timelock is present, it's either `Blocks(Height)` (a u16 block count when bit 22 is clear) or `Time(MTPInterval)` (a u16 value where each unit equals 512 seconds, when bit 22 is set). The tool must iterate over every input in the transaction and parse each sequence number. Most inputs in standard transactions will have sequences like `0xFFFFFFFF` or `0xFFFFFFFE` which do NOT encode relative timelocks, but the tool should still report the raw value and what it means (final, RBF-signaling, etc.).

**OP_CHECKLOCKTIMEVERIFY (script-level absolute timelock).** This opcode appears inside transaction scripts. When present, it compares the transaction's nLockTime against a value pushed onto the stack immediately before the opcode. The mempool.space API pre-decodes scripts in the fields `scriptsig_asm`, `inner_redeemscript_asm`, and `inner_witnessscript_asm` — the tool should scan these string fields for the substring "OP_CHECKLOCKTIMEVERIFY" (or its alias "OP_CLTV"). When found, the preceding element in the assembly string is the timelock value (a numeric push). This value follows the same height-vs-timestamp classification as nLockTime (threshold at 500,000,000). If using the raw hex path with rust-bitcoin, the script can be iterated via `script.instructions()` which yields opcodes and push data in sequence — the push immediately before `OP_CLTV` contains the timelock value.

**OP_CHECKSEQUENCEVERIFY (script-level relative timelock).** Same approach as CLTV: scan decoded scripts for "OP_CHECKSEQUENCEVERIFY" (or "OP_CSV") and extract the preceding numeric push. This value follows the BIP 68 encoding: bit 22 determines whether it's blocks or time, and the lower 16 bits are the magnitude. CSV enforces that the input spending this script has a sequence number that satisfies the relative timelock.

The output of the extraction should be a structured result per transaction containing: the transaction ID, the nLockTime value with its classification (blocks/seconds/disabled), a list of per-input sequence analysis (raw value, whether it encodes a relative timelock, the decoded timelock if present), a list of all CLTV occurrences found in scripts (which input, which script field, the threshold value, its classification), and a list of all CSV occurrences found in scripts (same structure). Each timelock value should also have a human-readable interpretation: block heights should show "Block N", timestamps should show a formatted UTC date, block-count relative timelocks should show "N blocks (≈ X hours/days)", and time-based relative timelocks should show "N × 512 seconds (≈ X hours)".

---

## Goal 4: Minimal CLI interface

The CLI is the first way to verify that the fetching and parsing work correctly. It should support at least two commands:

**Analyze a single transaction.** Given a txid as argument, fetch it from mempool.space, run the full timelock extraction, and print the structured result to stdout (formatted as readable text, or as JSON with a flag). This is the most basic "does it work" check.

**Scan a block.** Given a block height as argument, fetch all transactions in that block, run timelock extraction on each one, and output a summary: how many transactions in the block, how many contain at least one active timelock, and the details of each timelock-bearing transaction. This demonstrates the batch scanning capability.

The CLI output should be clear enough that a developer can look at it and immediately verify whether the extraction is correct by cross-referencing with mempool.space's web interface for the same transaction.

---

## What "done" looks like

A Rust binary that can be run from the command line, pointed at any txid or block height, and that outputs a complete, correctly classified breakdown of every timelock in the target transactions. The data fetching handles rate limits gracefully, the extraction catches all four timelock types, the classification correctly distinguishes block heights from timestamps and active from disabled timelocks, and the architecture has a clean separation between data fetching and analysis with a trait boundary ready for a future Bitcoin Core RPC adapter.
