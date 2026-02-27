use cltv_scan::api::types::*;
use cltv_scan::lightning::detector::classify_lightning;
use cltv_scan::lightning::types::*;

// ─── Test helpers ────────────────────────────────────────────────────────────

fn make_status() -> ApiStatus {
    ApiStatus {
        confirmed: true,
        block_height: Some(886000),
        block_hash: Some("00000000".to_string()),
        block_time: Some(1700000000),
    }
}

fn make_vout(value: u64, script_type: &str) -> ApiVout {
    ApiVout {
        scriptpubkey: "00".to_string(),
        scriptpubkey_asm: "OP_0".to_string(),
        scriptpubkey_type: script_type.to_string(),
        scriptpubkey_address: None,
        value,
    }
}

fn make_vin(sequence: u32) -> ApiVin {
    ApiVin {
        txid: Some("aa".repeat(32)),
        vout: Some(0),
        prevout: None,
        scriptsig: None,
        scriptsig_asm: None,
        inner_redeemscript_asm: None,
        inner_witnessscript_asm: None,
        witness: None,
        is_coinbase: false,
        sequence,
    }
}

fn make_tx(locktime: u32, vins: Vec<ApiVin>, vouts: Vec<ApiVout>) -> ApiTransaction {
    ApiTransaction {
        txid: "bb".repeat(32),
        version: 2,
        locktime,
        vin: vins,
        vout: vouts,
        size: 200,
        weight: 800,
        fee: Some(1000),
        status: make_status(),
    }
}

// ─── Commitment transaction detection ────────────────────────────────────────

#[test]
fn test_regular_tx_not_commitment() {
    // Standard tx: locktime=0, sequence=0xFFFFFFFF, normal outputs
    let tx = make_tx(
        0,
        vec![make_vin(0xFFFFFFFF)],
        vec![make_vout(50_000, "v0_p2wpkh"), make_vout(40_000, "v0_p2wpkh")],
    );
    let result = classify_lightning(&tx);
    assert_eq!(result.confidence, Confidence::None);
    assert_eq!(result.tx_type, None);
}

#[test]
fn test_commitment_locktime_in_lightning_range() {
    // Locktime with upper byte 0x20 → Lightning encoding
    // 0x20_00_00_42 = 536870978
    let locktime: u32 = 0x20000042;
    let sequence: u32 = 0x80000001; // upper byte 0x80
    let tx = make_tx(
        locktime,
        vec![make_vin(sequence)],
        vec![
            make_vout(100_000, "v0_p2wsh"),  // to_local
            make_vout(200_000, "v0_p2wpkh"), // to_remote
            make_vout(330, "v0_p2wsh"),       // anchor
            make_vout(330, "v0_p2wsh"),       // anchor
        ],
    );
    let result = classify_lightning(&tx);
    assert_eq!(result.tx_type, Some(LightningTxType::Commitment));
    assert_eq!(result.confidence, Confidence::HighlyLikely);
    assert!(result.commitment_signals.locktime_match);
    assert!(result.commitment_signals.sequence_match);
    assert!(result.commitment_signals.has_anchor_outputs);
    assert_eq!(result.commitment_signals.anchor_output_count, 2);
}

#[test]
fn test_commitment_without_anchors_is_possible() {
    // Locktime + sequence match but no anchor outputs → Possible (older channel type)
    let locktime: u32 = 0x20000100;
    let sequence: u32 = 0x80000005;
    let tx = make_tx(
        locktime,
        vec![make_vin(sequence)],
        vec![
            make_vout(100_000, "v0_p2wsh"),
            make_vout(200_000, "v0_p2wpkh"),
        ],
    );
    let result = classify_lightning(&tx);
    assert_eq!(result.tx_type, Some(LightningTxType::Commitment));
    // locktime + sequence match = at least Possible, could be HighlyLikely
    assert!(result.confidence >= Confidence::Possible);
    assert!(result.commitment_signals.locktime_match);
    assert!(result.commitment_signals.sequence_match);
    assert!(!result.commitment_signals.has_anchor_outputs);
}

#[test]
fn test_commitment_locktime_only_is_possible() {
    // Only locktime matches, sequence is standard → Possible at most
    let locktime: u32 = 0x20000042;
    let tx = make_tx(
        locktime,
        vec![make_vin(0xFFFFFFFD)], // standard RBF sequence, not 0x80
        vec![make_vout(100_000, "v0_p2wsh")],
    );
    let result = classify_lightning(&tx);
    // With only locktime matching, should not be HighlyLikely
    assert!(result.confidence <= Confidence::Possible);
    assert!(result.commitment_signals.locktime_match);
    assert!(!result.commitment_signals.sequence_match);
}

#[test]
fn test_locktime_just_below_lightning_range() {
    // 0x1F_FF_FF_FF is just below the 0x20 range → not Lightning
    let tx = make_tx(
        0x1FFFFFFF,
        vec![make_vin(0x80000001)],
        vec![make_vout(100_000, "v0_p2wsh")],
    );
    let result = classify_lightning(&tx);
    assert!(!result.commitment_signals.locktime_match);
}

#[test]
fn test_locktime_above_lightning_range() {
    // 0x40_00_00_00 is above the typical Lightning range → not Lightning locktime
    let tx = make_tx(
        0x40000000,
        vec![make_vin(0x80000001)],
        vec![make_vout(100_000, "v0_p2wsh")],
    );
    let result = classify_lightning(&tx);
    assert!(!result.commitment_signals.locktime_match);
}

#[test]
fn test_commitment_number_extraction() {
    // Commitment number is encoded across locktime (lower 24 bits) and sequence (lower 24 bits)
    // obscured_commit_num = ((sequence & 0x00FFFFFF) << 24) | (locktime & 0x00FFFFFF)
    let locktime: u32 = 0x20_AB_CD_EF; // lower 24 bits: 0xABCDEF
    let sequence: u32 = 0x80_12_34_56; // lower 24 bits: 0x123456
    let tx = make_tx(
        locktime,
        vec![make_vin(sequence)],
        vec![
            make_vout(100_000, "v0_p2wsh"),
            make_vout(330, "v0_p2wsh"),
            make_vout(330, "v0_p2wsh"),
        ],
    );
    let result = classify_lightning(&tx);
    assert_eq!(result.tx_type, Some(LightningTxType::Commitment));

    // The obscured commitment number
    let expected: u64 = (0x123456_u64 << 24) | 0xABCDEF_u64;
    assert_eq!(result.params.commitment_number, Some(expected));
}

// ─── HTLC-timeout detection ─────────────────────────────────────────────────

#[test]
fn test_htlc_timeout_detection() {
    // HTLC-timeout: nLockTime = realistic block height, no 32-byte preimage in witness
    let locktime: u32 = 886100; // realistic block height
    let mut vin = make_vin(0);
    // Witness without a 32-byte (64 hex char) element
    vin.witness = Some(vec![
        "".to_string(),           // empty (no preimage)
        "3045".to_string(),       // signature (not 64 chars)
        "00".to_string(),         // some script element
    ]);
    vin.inner_witnessscript_asm = Some(
        "OP_DUP OP_HASH160 abc123 OP_EQUALVERIFY OP_CHECKSIG OP_IF 886100 OP_CHECKLOCKTIMEVERIFY OP_DROP OP_ENDIF 1 OP_CHECKSEQUENCEVERIFY".to_string()
    );
    let tx = make_tx(
        locktime,
        vec![vin],
        vec![make_vout(50_000, "v0_p2wsh")],
    );
    let result = classify_lightning(&tx);
    assert_eq!(result.tx_type, Some(LightningTxType::HtlcTimeout));
    assert!(!result.htlc_signals.has_preimage);
    assert!(result.htlc_signals.script_has_cltv);
    assert_eq!(result.params.cltv_expiry, Some(886100));
}

#[test]
fn test_htlc_timeout_no_preimage_empty_witness_element() {
    // HTLC-timeout with empty string at preimage position
    let locktime: u32 = 886200;
    let mut vin = make_vin(0);
    vin.witness = Some(vec![
        "".to_string(),
        "3044022000".to_string(),
    ]);
    vin.inner_witnessscript_asm = Some(
        "OP_SIZE 32 OP_EQUAL OP_IF OP_HASH160 abc OP_ELSE 886200 OP_CHECKLOCKTIMEVERIFY OP_DROP OP_ENDIF 1 OP_CHECKSEQUENCEVERIFY".to_string()
    );
    let tx = make_tx(
        locktime,
        vec![vin],
        vec![make_vout(50_000, "v0_p2wsh")],
    );
    let result = classify_lightning(&tx);
    assert_eq!(result.tx_type, Some(LightningTxType::HtlcTimeout));
    assert!(!result.htlc_signals.has_preimage);
}

// ─── HTLC-success detection ─────────────────────────────────────────────────

#[test]
fn test_htlc_success_detection() {
    // HTLC-success: nLockTime = 0, 32-byte preimage in witness
    let preimage = "ab".repeat(32); // 64 hex chars = 32 bytes
    let mut vin = make_vin(0);
    vin.witness = Some(vec![
        preimage.clone(),
        "3045".to_string(),
    ]);
    vin.inner_witnessscript_asm = Some(
        "OP_SIZE 32 OP_EQUAL OP_IF OP_HASH160 abc OP_EQUALVERIFY OP_CHECKSIG OP_ELSE 1 OP_CHECKSEQUENCEVERIFY OP_DROP OP_ENDIF".to_string()
    );
    let tx = make_tx(
        0, // locktime = 0 for HTLC-success
        vec![vin],
        vec![make_vout(50_000, "v0_p2wsh")],
    );
    let result = classify_lightning(&tx);
    assert_eq!(result.tx_type, Some(LightningTxType::HtlcSuccess));
    assert!(result.htlc_signals.has_preimage);
    assert_eq!(result.htlc_signals.preimage, Some(preimage));
    assert!(result.params.preimage_revealed);
    assert!(result.params.preimage.is_some());
}

#[test]
fn test_htlc_success_preimage_must_be_hex() {
    // 64 chars but not valid hex → should NOT be detected as preimage
    let not_hex = "zz".repeat(32);
    let mut vin = make_vin(0);
    vin.witness = Some(vec![not_hex, "3045".to_string()]);
    vin.inner_witnessscript_asm = Some(
        "OP_SIZE 32 OP_EQUAL OP_IF OP_HASH160 abc OP_EQUALVERIFY OP_ENDIF 1 OP_CHECKSEQUENCEVERIFY".to_string()
    );
    let tx = make_tx(0, vec![vin], vec![make_vout(50_000, "v0_p2wsh")]);
    let result = classify_lightning(&tx);
    assert!(!result.htlc_signals.has_preimage);
}

// ─── HTLC CSV delay extraction ──────────────────────────────────────────────

#[test]
fn test_csv_delay_extraction_from_htlc() {
    // Both HTLC-timeout and HTLC-success outputs have CSV-delayed scripts
    // The CSV value = to_self_delay (revocation window)
    let mut vin = make_vin(0);
    vin.witness = Some(vec!["".to_string(), "3045".to_string()]);
    vin.inner_witnessscript_asm = Some(
        "OP_IF abc OP_ELSE 144 OP_CHECKSEQUENCEVERIFY OP_DROP def OP_ENDIF".to_string()
    );
    let tx = make_tx(886300, vec![vin], vec![make_vout(50_000, "v0_p2wsh")]);
    let result = classify_lightning(&tx);
    assert!(result.params.csv_delays.contains(&144));
}

// ─── Anchor output counting ─────────────────────────────────────────────────

#[test]
fn test_single_anchor_output() {
    let locktime: u32 = 0x20000001;
    let sequence: u32 = 0x80000001;
    let tx = make_tx(
        locktime,
        vec![make_vin(sequence)],
        vec![
            make_vout(100_000, "v0_p2wsh"),
            make_vout(330, "v0_p2wsh"), // single anchor
        ],
    );
    let result = classify_lightning(&tx);
    assert!(result.commitment_signals.has_anchor_outputs);
    assert_eq!(result.commitment_signals.anchor_output_count, 1);
}

// ─── Edge cases ──────────────────────────────────────────────────────────────

#[test]
fn test_coinbase_tx_not_lightning() {
    let mut vin = make_vin(0xFFFFFFFF);
    vin.is_coinbase = true;
    vin.txid = None;
    let tx = make_tx(
        0x20000042,
        vec![vin],
        vec![make_vout(312_500_000, "v1_p2tr")],
    );
    let result = classify_lightning(&tx);
    // Coinbase should not be classified as Lightning even if locktime happens to match
    assert_eq!(result.tx_type, None);
}

#[test]
fn test_commitment_takes_priority_over_htlc() {
    // A tx that has both commitment signals (locktime 0x20, sequence 0x80)
    // and a witness with 32-byte element should be classified as commitment, not HTLC
    let preimage = "cc".repeat(32);
    let mut vin = make_vin(0x80000001);
    vin.witness = Some(vec![preimage, "3045".to_string()]);
    let tx = make_tx(
        0x20000042,
        vec![vin],
        vec![
            make_vout(100_000, "v0_p2wsh"),
            make_vout(330, "v0_p2wsh"),
            make_vout(330, "v0_p2wsh"),
        ],
    );
    let result = classify_lightning(&tx);
    assert_eq!(result.tx_type, Some(LightningTxType::Commitment));
}

#[test]
fn test_htlc_output_count_on_commitment() {
    // Commitment with 3 HTLC outputs (P2WSH, not anchors, not to_remote P2WPKH)
    let locktime: u32 = 0x20000001;
    let sequence: u32 = 0x80000001;
    let tx = make_tx(
        locktime,
        vec![make_vin(sequence)],
        vec![
            make_vout(100_000, "v0_p2wsh"),   // to_local
            make_vout(200_000, "v0_p2wpkh"),  // to_remote
            make_vout(330, "v0_p2wsh"),        // anchor
            make_vout(330, "v0_p2wsh"),        // anchor
            make_vout(50_000, "v0_p2wsh"),     // HTLC 1
            make_vout(60_000, "v0_p2wsh"),     // HTLC 2
            make_vout(70_000, "v0_p2wsh"),     // HTLC 3
        ],
    );
    let result = classify_lightning(&tx);
    assert_eq!(result.tx_type, Some(LightningTxType::Commitment));
    // HTLC outputs = P2WSH outputs that aren't anchors (330 sats) or to_local (first P2WSH)
    // This is heuristic — the exact count depends on implementation logic
    assert!(result.params.htlc_output_count.is_some());
}
