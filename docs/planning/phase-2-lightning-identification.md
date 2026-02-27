# Phase 2 — Lightning Network Transaction Identification

## Context

Phase 1 extracts timelocks from any Bitcoin transaction generically. Phase 2 adds the ability to recognize Lightning Network transactions specifically. This matters because the most dangerous timelock attacks (flood-and-loot, replacement cycling, forced expiration spam, transaction pinning) all target Lightning's HTLC mechanism. A timelock analyzer that can't distinguish a Lightning force-close from a regular Bitcoin transaction is missing the most security-critical context.

Lightning transactions are regular Bitcoin transactions — they don't have any special flag or marker at the protocol level. But they have very distinctive patterns in their locktime values, sequence numbers, output amounts, script structures, and witness data. This phase implements heuristic detection of these patterns.

All the identification logic belongs in the analysis module, not in the data fetching layer. It takes the same transaction data that Phase 1 already fetches and adds a classification layer on top.

---

## Goal 1: Identify commitment transactions

A Lightning commitment transaction is what gets broadcast when a channel is force-closed. It spends the funding output (the 2-of-2 multisig that anchors the channel) and creates outputs for each party's balance plus any pending HTLCs. These transactions have several distinctive fingerprints that can be detected from the fields available in the mempool.space API response.

**Locktime encoding.** Commitment transactions do not use locktime for its standard purpose. Instead, they encode an obscured commitment number in the locktime field. The encoding places the value in the range starting at 0x20000000 (536,870,912), which technically falls in the "timestamp" domain (above the 500,000,000 threshold) but represents a time far in the past (around 1987), which no legitimate timestamp-based timelock would use. The upper 8 bits are 0x20, and the lower 24 bits carry part of the obscured commitment number. The detection should flag any transaction with a locktime in this characteristic range. Specifically, the value will be ≥ 0x20000000 and the upper byte pattern will be consistent with the Lightning encoding scheme.

**Sequence encoding.** The inputs of commitment transactions use sequence values where the upper byte is 0x80. The lower 24 bits carry the other part of the obscured commitment number. This pattern is unusual for normal Bitcoin transactions — most standard transactions use `0xFFFFFFFF`, `0xFFFFFFFE`, or `0xFFFFFFFD` for their sequence values. A sequence with the 0x80 upper byte combined with the 0x20 locktime pattern is a strong Lightning signal.

**Anchor outputs.** Since the adoption of the anchor outputs channel type (which is now the default in all major Lightning implementations), commitment transactions include one or two outputs with a value of exactly 330 satoshis. These tiny outputs exist solely to allow either party to fee-bump the commitment transaction via CPFP. Finding a 330-sat output is not conclusive on its own (someone could create any output with that value), but combined with the locktime and sequence patterns, it significantly increases confidence.

**Output structure.** Commitment transactions typically produce a recognizable set of outputs: `to_local` (the broadcaster's balance, P2WSH with CSV delay), `to_remote` (the counterparty's balance, P2WPKH or P2WSH depending on the channel type), anchor outputs (330 sats each), and zero or more HTLC outputs (P2WSH). The number and types of outputs, combined with the other signals, contribute to the identification.

The identification should produce a confidence assessment for each transaction: "highly likely commitment transaction" when multiple signals align (locktime in 0x20 range + sequence with 0x80 byte + anchor outputs), "possible commitment transaction" when some signals are present, or "not a commitment transaction" when none match. The tool should extract the obscured commitment number from the locktime and sequence fields when a commitment transaction is identified, as this gives context about how many state updates the channel had before closing.

---

## Goal 2: Distinguish HTLC-timeout from HTLC-success transactions

When a commitment transaction is confirmed, its HTLC outputs eventually get spent by second-stage transactions. There are exactly two types, and distinguishing them is critical for security analysis because they have opposite security implications.

**HTLC-timeout transactions** represent the "refund" path — the HTLC has expired without the payment preimage being revealed, so the funds return to the party that offered the HTLC. These have `nLockTime` set to the CLTV expiry block height (a concrete block number, typically in the current range of block heights, not the 0x20 Lightning encoding). Their witness data does NOT contain a 32-byte preimage — the preimage position in the witness stack is empty or contains a zero-length push. These are the transactions that flood-and-loot attacks try to prevent from confirming: if they can't get into a block before the timelock expires, the attacker profits.

**HTLC-success transactions** represent the "claim" path — someone is revealing the payment preimage to collect the HTLC payment. These have `nLockTime = 0` and their witness data DOES contain a 32-byte value (the preimage, which appears as a 64-character hex string in the witness array). These are the transactions that replacement cycling attacks target: the attacker tries to evict them from the mempool so the preimage is never seen.

The detection heuristic should check: the locktime value (a realistic block height for timeout, 0 for success), and the witness data for the presence or absence of a 32-byte element. The mempool.space API returns witness data as an array of hex strings in the `witness` field of each input — checking if any element is exactly 64 hex characters (32 bytes) is the preimage test.

Additionally, the scripts that these transactions spend have characteristic sizes: offered HTLC scripts are 133 bytes and received HTLC scripts are 139 bytes. If the `inner_witnessscript_asm` field is available (it is when the transaction spends a P2WSH output), the script can be checked for the distinctive patterns: `OP_CHECKLOCKTIMEVERIFY`, `OP_CHECKSEQUENCEVERIFY`, `OP_CHECKMULTISIG`, and the `OP_SIZE 32 OP_EQUAL` pattern that verifies preimage size.

Both HTLC-timeout and HTLC-success transactions produce outputs that themselves have CSV-delayed scripts (the revocation window). The CSV value in these output scripts corresponds to the channel's `to_self_delay` parameter. The tool should extract this CSV value as it feeds into Phase 3's analysis of whether the revocation window is dangerously short.

---

## Goal 3: Extract Lightning-specific parameters from identified transactions

When a transaction is identified as Lightning-related, the tool should extract all the parameters that are relevant for security analysis:

**From commitment transactions:** The obscured commitment number (decoded from locktime and sequence), the number of HTLC outputs (a channel with many pending HTLCs is more exposed to flooding attacks), the `to_self_delay` CSV value from the `to_local` output script if it's already been spent (this is the revocation window), and whether anchor outputs are present (their absence means the older, less secure channel type).

**From HTLC-timeout transactions:** The CLTV expiry block height (this is the deadline — extracted from the nLockTime field), the difference between this expiry and the current block height (how many blocks until/since expiration), and the CSV delay on the output (the revocation window for this HTLC resolution).

**From HTLC-success transactions:** The 32-byte preimage from the witness data, the CSV delay on the output, and the fact that the preimage was revealed on-chain (this means the payment was claimed).

These extracted parameters are the raw material that Phase 3's detection logic will use for security analysis.

---

## Goal 4: CLI commands for Lightning identification

Extend the CLI from Phase 1 with Lightning-specific capabilities:

**Classify a single transaction.** Given a txid, fetch the transaction, run the Lightning identification heuristics, and output whether it's a commitment transaction, HTLC-timeout, HTLC-success, or none of the above, along with all extracted Lightning parameters and the confidence level.

**Scan a block for Lightning activity.** Given a block height, fetch all transactions, identify which ones are Lightning-related, and output a summary: how many commitment transactions (force-closes), how many HTLC-timeout and HTLC-success transactions, and the details of each. This gives a snapshot of Lightning force-close activity in any given block.

---

## What "done" looks like

The tool can scan any block or individual transaction and identify Lightning commitment transactions, HTLC-timeout transactions, and HTLC-success transactions with reasonable accuracy using multiple heuristic signals. Each identified transaction is annotated with its extracted Lightning parameters (commitment number, CLTV expiry, preimage status, CSV delays). The CLI shows all of this clearly. The identification is heuristic and transparent about its confidence levels — it doesn't claim certainty where only pattern matching is available.

---

## Important note on accuracy

No heuristic identification of Lightning transactions is perfect. Some non-Lightning transactions might coincidentally match some patterns (false positives), and some Lightning transactions using non-standard channel types might not match all expected patterns (false negatives). The tool should always present its identification as a confidence assessment, not a binary determination. For the purpose of this project, "highly likely" based on multiple matching signals is sufficient. The goal is useful heuristic analysis, not forensic certainty.
