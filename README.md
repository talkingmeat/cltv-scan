# cltv-scan

A Bitcoin timelock security analyzer that scans live blockchain transactions, extracts and classifies all four types of timelocks, identifies Lightning Network transactions on-chain, detects patterns associated with known attack vectors, and monitors the mempool in real-time for emerging threats.

Built for [Bitcoin++ Exploits Edition 2026](https://btcpp.dev/).

---

## Why this project exists

Block explorers show that a transaction has a locktime of 886000 or a sequence of 0xFFFFFFFD. They do not tell you what that means, whether it is dangerous, or whether the transaction is part of a Lightning force-close that is about to lose money.

The research literature documents at least nine distinct timelock-related attack vectors -- from flood-and-loot (Harris & Zohar, 2020) to replacement cycling (Riard, 2023) -- yet the ecosystem lacks a single tool that systematically scans transactions for timelock vulnerabilities. The space is covered by a patchwork of script debuggers, Miniscript compilers, block explorers, and Lightning monitoring tools that each address a fragment of the problem.

cltv-scan fills that gap. It connects to the mempool.space API, pulls real transaction data, and runs heuristic analysis across four layers: raw timelock extraction, Lightning transaction fingerprinting, security pattern detection, and structured alerting. The output is available as terminal text, JSON, or through an HTTP API that a dashboard can consume.

---

## What it does

### Timelock extraction

Every Bitcoin transaction can contain up to four distinct types of timelocks. cltv-scan extracts and classifies all of them:

- **nLockTime** -- the transaction-level absolute timelock. Classified as block height or Unix timestamp (threshold at 500,000,000). Reports whether it is actually enforced (requires at least one input with sequence != 0xFFFFFFFF) or effectively disabled.
- **nSequence (BIP 68)** -- per-input relative timelocks. Parses the 32-bit sequence field: bit 31 (disable), bit 22 (time vs blocks), lower 16 bits (magnitude). Distinguishes standard values (0xFFFFFFFF final, 0xFFFFFFFE locktime-enabled, 0xFFFFFFFD RBF) from actual relative timelocks.
- **OP_CHECKLOCKTIMEVERIFY** -- script-level absolute timelocks. Scanned from decoded script ASM fields (scriptsig_asm, inner_redeemscript_asm, inner_witnessscript_asm). Extracts the threshold value pushed before the opcode.
- **OP_CHECKSEQUENCEVERIFY** -- script-level relative timelocks. Same scanning approach, with BIP 68 encoding applied to the extracted value.

All values get human-readable formatting: block heights show as "block 886000", timestamps as "2024-01-15 12:00 UTC", relative timelocks as "144 blocks (~24.0 hours)".

### Lightning identification

Lightning Network transactions are regular Bitcoin transactions with distinctive fingerprints. cltv-scan uses heuristic detection to classify them:

- **Commitment transactions** (force-closes) -- identified by locktime in the 0x20 range (Lightning encodes the obscured commitment number here), input sequences with 0x80 upper byte, and anchor outputs of exactly 330 satoshis. Multiple matching signals produce a "highly likely" confidence; single signals produce "possible".
- **HTLC-timeout transactions** -- the refund path when an HTLC expires. Identified by a realistic block height in nLockTime, no 32-byte preimage in the witness data, and OP_CHECKLOCKTIMEVERIFY in the witness script.
- **HTLC-success transactions** -- the claim path when someone reveals the payment preimage. Identified by nLockTime of 0 and a 32-byte element (64 hex characters) in the witness data.

From identified transactions, cltv-scan extracts: the obscured commitment number, the number of HTLC outputs, CLTV expiry block heights, CSV delay values, and preimages.

### Security analysis

Four detection heuristics scan for known attack vectors and dangerous configurations:

**Timelock mixing** (severity: critical) -- Detects scripts that mix block-height-based and time-based timelocks in the same spending path. This makes the script permanently unspendable because Bitcoin consensus requires all timelocks in a transaction to use the same domain. Checks three levels: CLTV vs CSV within a script, nLockTime vs CLTV across the transaction, and nSequence vs CSV across the transaction. Based on "Don't Mix Your Timelocks" by Kanjalkar and Poelstra (Blockstream Research).

**Short CLTV delta** (severity: configurable) -- Flags CLTV timelocks that are close to expiring or already expired. Thresholds derived from the BOLT specification: critical below 18 blocks (minimum final hop delta per [BOLT #785](https://github.com/lightning/bolts/pull/785)), warning below 34 blocks (BOLT #2 recommendation via formula 3R + 2G + 2S), informational below 72 blocks (congestion risk zone). Already-expired CLTVs are always critical.

**HTLC timeout clustering** (severity: warning) -- Counts HTLC-timeout transactions with CLTV expiry values concentrated in a narrow block-height window. A sliding window of 6 blocks (configurable) that exceeds 85 concurrent expirations (configurable) triggers an alert. This is the observable on-chain signature of a [flood-and-loot attack](https://arxiv.org/abs/2006.08513) (Harris & Zohar, 2020), which showed that just 85 simultaneously attacked channels suffice to guarantee profit.

**Anomalous nSequence** (severity: informational/warning) -- Flags inputs with non-standard sequence values: very short relative timelocks (< 6 blocks, may indicate minimized revocation windows), very long relative timelocks (> 1000 blocks, unusual), and time-based relative timelocks (bit 22 set, rare in practice). Lightning commitment sequences (0x80 upper byte) are recognized and excluded from anomaly detection.

All detections produce structured alerts with severity level, affected transaction, description, raw data, and attack reference (paper, author, year, URL).

### Mempool monitor

A real-time monitoring mode that continuously polls unconfirmed transactions from the mempool, analyzes each one, and prints findings as they appear. Only transactions with active timelocks, Lightning classification, or security alerts are displayed. Configurable polling interval and minimum severity filter.

### HTTP server

The same binary runs as a CLI tool or an HTTP server. The server wraps all analytical capabilities as JSON API endpoints:

| Endpoint | Description |
|---|---|
| `GET /api/tx/{txid}` | Full timelock + Lightning + security analysis for a single transaction |
| `GET /api/block/{height}` | Analyzed transactions in a block, with filtering and pagination |
| `GET /api/scan?start=N&end=M` | Security alerts across a block range, filterable by severity and detection type |
| `GET /api/lightning?start=N&end=M` | Lightning activity summary with HTLC expiry distribution |

Features: CORS support for browser clients, in-memory caching ([moka](https://github.com/moka-rs/moka)) to reduce mempool.space API calls, configurable mempool.space base URL for self-hosted instances.

---

## Installation

Requires Rust 1.80+ (2024 edition).

```bash
git clone https://github.com/AguasBCN/cltv-scan.git
cd cltv-scan
cargo build --release
```

The binary is at `target/release/cltv-scan`.

---

## Usage

### Analyze a single transaction

```bash
# Terminal output
cltv-scan tx <txid>

# JSON output
cltv-scan tx <txid> --json
```

### Scan a block for timelocks

```bash
cltv-scan block <height>
cltv-scan block <height> --json
```

### Lightning identification

```bash
# Classify a single transaction
cltv-scan lightning tx <txid>

# Scan a block for Lightning activity
cltv-scan lightning block <height>
```

### Security scan

```bash
# Scan a single block
cltv-scan scan <height>

# Scan a range of blocks
cltv-scan scan <start> -e <end>

# With custom thresholds
cltv-scan scan <height> --cltv-critical 18 --cltv-warning 34 --cluster-threshold 85

# JSON output
cltv-scan scan <height> --json
```

### Monitor the mempool

```bash
# Watch for interesting transactions in real-time (polls every 10s)
cltv-scan monitor

# Custom polling interval
cltv-scan monitor --interval 5

# Only show warning and critical alerts
cltv-scan monitor --min-severity warning

# JSON output (one line per transaction, useful for piping)
cltv-scan monitor --json
```

### Start the HTTP server

```bash
# Default: port 3001, public mempool.space
cltv-scan serve

# Custom configuration
cltv-scan serve -p 8080 --mempool-url https://mempool.mynode.local --request-delay-ms 100
```

---

## Example output

```
$ cltv-scan scan 886500

Security Scan: block 886500
========================================================================
455 alerts: 19 critical, 12 warning, 424 informational

[CRITICAL] short-cltv-delta
  tx: 9303ffe0f2fcfb3907546ff3b8e1ce8fd29d8386f9a11df132becb12f67ae48d input[0]
  CLTV timelock at block 886866 has expired (51694 blocks ago).
  Time-sensitive spending condition is now active.

[WARNING ] anomalous-sequence
  tx: ae090d9d6ba34c17b80c1d26e995bcc5325ce69d4566ac9120b015a7d982f61b input[0]
  Input 0 has a very long relative timelock (2016 blocks ~ 14.0 days).
  Unusual -- may indicate specialized custody or misconfiguration.
```

```
$ cltv-scan monitor --min-severity warning

Monitoring mempool (every 10s, Ctrl+C to stop)...

[14:23:05] 9303ffe0f2fcfb3907546ff3b8e1ce8fd29d8386f9a11df132becb12f67ae48d
  ⚡ Lightning: commitment (force-close) [highly likely]
  [CRITICAL] short-cltv-delta: CLTV timelock at block 886866 has expired (51694 blocks ago).
  timelocks: nLockTime, 2 CLTV

[14:23:15] ae090d9d6ba34c17b80c1d26e995bcc5325ce69d4566ac9120b015a7d982f61b
  [WARNING ] anomalous-sequence: Input 0 has a very long relative timelock (2016 blocks ~ 14.0 days).
  timelocks: 1 nSequence
```

```
$ cltv-scan lightning block 886500

Block 886500 -- Lightning Activity
========================================================================
3117 transactions scanned, 220 Lightning-related
  188 commitment (force-close), 13 HTLC-timeout, 19 HTLC-success
```

---

## Architecture

```
src/
  api/          Data fetching layer
    types.rs      mempool.space API response structs
    source.rs     DataSource trait (extensible to Bitcoin Core RPC)
    client.rs     MempoolClient with rate limiting and retry
    cache.rs      CachedClient wrapper (moka in-memory cache)
  timelock/     Timelock extraction and classification
    types.rs      TransactionAnalysis, NLocktimeInfo, SequenceInfo, ScriptTimelock
    classify.rs   Height/timestamp classification, BIP 68 parsing, human-readable formatting
    extractor.rs  Core extraction of all 4 timelock types
  lightning/    Lightning Network transaction identification
    types.rs      LightningClassification, Confidence, signals and params
    detector.rs   Heuristic detection (commitment, HTLC-timeout, HTLC-success)
  security/     Security pattern detection
    types.rs      Alert, Severity, DetectionType, SecurityConfig
    analyzer.rs   4 detectors (mixing, short CLTV, clustering, anomalous sequences)
  server/       HTTP API (axum)
    types.rs      Request/response structs
    handlers.rs   Endpoint handlers
    mod.rs        Router setup with CORS
  cli/          Terminal output formatting
    output.rs     Human-readable and JSON formatting
  main.rs       CLI entry point (clap subcommands)
  lib.rs        Public API re-exports
```

The architecture enforces a strict separation: the analysis modules (`timelock`, `lightning`, `security`) never depend on how data is fetched or how results are displayed. The `DataSource` trait abstracts the data layer, making it possible to add a Bitcoin Core RPC adapter without changing any analysis logic.

---

## Attack vectors covered

| Attack | Detection | Severity | Reference |
|---|---|---|---|
| Timelock mixing | Active | Critical | [Kanjalkar & Poelstra, Blockstream Research](https://blog.blockstream.com/dont-mix-your-timelocks/) |
| Short CLTV delta | Active | Configurable | [BOLT #2](https://github.com/lightning/bolts/blob/master/02-peer-protocol.md), [BOLT #785](https://github.com/lightning/bolts/pull/785) |
| Flood-and-loot | Active | Warning | [Harris & Zohar, 2020](https://arxiv.org/abs/2006.08513) |
| Anomalous nSequence | Active | Info/Warning | [BIP 68](https://github.com/bitcoin/bips/blob/master/bip-0068.mediawiki) |
| Forced expiration spam | Reference | -- | [Poon & Dryja, 2016](https://lightning.network/lightning-network-paper.pdf) |
| Time-dilation attacks | Reference | -- | [Riard & Naumenko, 2020](https://arxiv.org/abs/2006.01418) |
| Transaction pinning | Reference | -- | [Teinturier](https://github.com/t-bast/lightning-docs/blob/master/pinning-attacks.md) |
| Replacement cycling | Reference | -- | [Riard, 2023](https://bitcoinops.org/en/newsletters/2023/11/01/) (CVE-2023-40231) |
| Congestion attacks | Reference | -- | [Mizrahi & Zohar, 2020](https://arxiv.org/abs/2002.06564) |

---

## Data source

cltv-scan uses the [mempool.space](https://mempool.space) public API. The `DataSource` trait abstracts the data layer, designed for a future Bitcoin Core RPC adapter that would eliminate external API dependency and enable full blockchain history scanning.

Rate limiting: configurable delay between requests (default 250ms) with exponential backoff on HTTP 429 responses. Self-hosting mempool.space ([instructions](https://github.com/mempool/mempool)) eliminates rate limits entirely.

---

## Tests

```bash
cargo test
```

54 tests across three test suites:
- `lightning_tests.rs` -- 16 tests for commitment, HTLC-timeout, HTLC-success detection
- `security_tests.rs` -- 25 tests for all four detection heuristics and the alert system
- `server_tests.rs` -- 13 integration tests for all API endpoints with mock DataSource

---

## References

- Harris, J. & Zohar, A. (2020). [Flood & Loot: A Systemic Attack On The Lightning Network](https://arxiv.org/abs/2006.08513)
- Riard, A. & Naumenko, G. (2020). [Time-Dilation Attacks on the Lightning Network](https://arxiv.org/abs/2006.01418)
- Riard, A. (2023). [Replacement Cycling Attacks](https://bitcoinops.org/en/newsletters/2023/11/01/) (CVE-2023-40231)
- Mizrahi, A. & Zohar, A. (2020). [Congestion Attacks in Payment Channel Networks](https://arxiv.org/abs/2002.06564)
- Teinturier, B. [Transaction Pinning Attacks](https://github.com/t-bast/lightning-docs/blob/master/pinning-attacks.md)
- Nadahalli, T. et al. (2021). [Timelocked Bribing](https://link.springer.com/chapter/10.1007/978-3-662-64322-8_3). Financial Cryptography
- Kanjalkar, S. & Poelstra, A. [Don't Mix Your Timelocks](https://blog.blockstream.com/dont-mix-your-timelocks/). Blockstream Research
- Poon, J. & Dryja, T. (2016). [The Bitcoin Lightning Network](https://lightning.network/lightning-network-paper.pdf)
- [BOLT Specifications](https://github.com/lightning/bolts)
- [BIP 68 -- Relative Lock-time Using Sequence](https://github.com/bitcoin/bips/blob/master/bip-0068.mediawiki)

---

## License

MIT
