use crate::api::types::ApiTransaction;

use super::types::*;

const ANCHOR_VALUE: u64 = 330;

/// Classify a transaction as Lightning-related or not.
pub fn classify_lightning(tx: &ApiTransaction) -> LightningClassification {
    // Skip coinbase transactions
    if tx.vin.iter().any(|v| v.is_coinbase) {
        return not_lightning();
    }

    let commitment_signals = detect_commitment_signals(tx);
    let htlc_signals = detect_htlc_signals(tx);

    // Commitment detection takes priority over HTLC
    let commitment_confidence = commitment_confidence(&commitment_signals);
    if commitment_confidence >= Confidence::Possible {
        let params = extract_commitment_params(tx, &commitment_signals);
        return LightningClassification {
            tx_type: Some(LightningTxType::Commitment),
            confidence: commitment_confidence,
            commitment_signals,
            htlc_signals,
            params,
        };
    }

    // HTLC detection
    if let Some((htlc_type, confidence, params)) = classify_htlc(tx, &htlc_signals) {
        return LightningClassification {
            tx_type: Some(htlc_type),
            confidence,
            commitment_signals,
            htlc_signals,
            params,
        };
    }

    LightningClassification {
        tx_type: None,
        confidence: Confidence::None,
        commitment_signals,
        htlc_signals,
        params: LightningParams::default(),
    }
}

fn not_lightning() -> LightningClassification {
    LightningClassification {
        tx_type: None,
        confidence: Confidence::None,
        commitment_signals: CommitmentSignals::default(),
        htlc_signals: HtlcSignals::default(),
        params: LightningParams::default(),
    }
}

// ─── Commitment detection ────────────────────────────────────────────────────

fn detect_commitment_signals(tx: &ApiTransaction) -> CommitmentSignals {
    let locktime_match = is_lightning_locktime(tx.locktime);
    let sequence_match = tx.vin.iter().any(|v| is_lightning_sequence(v.sequence));
    let anchor_output_count = tx.vout.iter().filter(|o| o.value == ANCHOR_VALUE).count();

    CommitmentSignals {
        locktime_match,
        sequence_match,
        has_anchor_outputs: anchor_output_count > 0,
        anchor_output_count,
    }
}

/// Lightning commitment transactions encode an obscured commitment number in locktime.
/// The upper byte is 0x20, placing the value in range [0x20000000, 0x20FFFFFF].
fn is_lightning_locktime(locktime: u32) -> bool {
    (locktime >> 24) == 0x20
}

/// Lightning commitment transaction inputs have sequence with upper byte 0x80.
fn is_lightning_sequence(sequence: u32) -> bool {
    (sequence >> 24) == 0x80
}

fn commitment_confidence(signals: &CommitmentSignals) -> Confidence {
    let mut score = 0;
    if signals.locktime_match {
        score += 1;
    }
    if signals.sequence_match {
        score += 1;
    }
    if signals.has_anchor_outputs {
        score += 1;
    }

    match score {
        0 => Confidence::None,
        1 => Confidence::Possible,
        _ => Confidence::HighlyLikely,
    }
}

fn extract_commitment_params(tx: &ApiTransaction, signals: &CommitmentSignals) -> LightningParams {
    let commitment_number = if signals.locktime_match && signals.sequence_match {
        let locktime_lower = (tx.locktime & 0x00FFFFFF) as u64;
        let seq_lower = tx
            .vin
            .iter()
            .find(|v| is_lightning_sequence(v.sequence))
            .map(|v| (v.sequence & 0x00FFFFFF) as u64)
            .unwrap_or(0);
        Some((seq_lower << 24) | locktime_lower)
    } else {
        None
    };

    // Count HTLC outputs: P2WSH outputs that aren't anchor outputs
    let htlc_output_count = tx
        .vout
        .iter()
        .filter(|o| o.scriptpubkey_type == "v0_p2wsh" && o.value != ANCHOR_VALUE)
        .count();

    // Subtract 1 for to_local (first non-anchor P2WSH) if present
    let htlc_output_count = htlc_output_count.saturating_sub(1);

    let csv_delays = extract_csv_delays_from_inputs(tx);

    LightningParams {
        commitment_number,
        htlc_output_count: Some(htlc_output_count),
        csv_delays,
        ..Default::default()
    }
}

// ─── HTLC detection ─────────────────────────────────────────────────────────

fn detect_htlc_signals(tx: &ApiTransaction) -> HtlcSignals {
    let mut has_preimage = false;
    let mut preimage = None;
    let mut script_has_cltv = false;
    let mut script_has_csv = false;

    for vin in &tx.vin {
        // Check witness for 32-byte preimage (64 hex chars, valid hex)
        if let Some(ref witness) = vin.witness {
            for elem in witness {
                if elem.len() == 64 && is_valid_hex(elem) {
                    has_preimage = true;
                    preimage = Some(elem.clone());
                    break;
                }
            }
        }

        // Check witness script for CLTV/CSV opcodes
        if let Some(ref asm) = vin.inner_witnessscript_asm {
            if asm.contains("OP_CHECKLOCKTIMEVERIFY") || asm.contains("OP_CLTV") {
                script_has_cltv = true;
            }
            if asm.contains("OP_CHECKSEQUENCEVERIFY") || asm.contains("OP_CSV") {
                script_has_csv = true;
            }
        }
    }

    HtlcSignals {
        locktime_value: tx.locktime,
        has_preimage,
        preimage,
        script_has_cltv,
        script_has_csv,
    }
}

fn is_valid_hex(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_hexdigit())
}

fn classify_htlc(
    tx: &ApiTransaction,
    signals: &HtlcSignals,
) -> Option<(LightningTxType, Confidence, LightningParams)> {
    let has_htlc_script = signals.script_has_cltv || signals.script_has_csv;

    if !has_htlc_script {
        return None;
    }

    let csv_delays = extract_csv_delays_from_inputs(tx);

    if signals.has_preimage && tx.locktime == 0 {
        // HTLC-success: preimage present, locktime = 0
        let params = LightningParams {
            preimage_revealed: true,
            preimage: signals.preimage.clone(),
            csv_delays,
            ..Default::default()
        };
        Some((LightningTxType::HtlcSuccess, Confidence::HighlyLikely, params))
    } else if !signals.has_preimage && is_realistic_block_height(tx.locktime) {
        // HTLC-timeout: no preimage, locktime = realistic block height
        let params = LightningParams {
            cltv_expiry: Some(tx.locktime),
            csv_delays,
            ..Default::default()
        };
        Some((LightningTxType::HtlcTimeout, Confidence::HighlyLikely, params))
    } else if has_htlc_script {
        // Has HTLC-like script patterns but doesn't cleanly match either type
        let params = LightningParams {
            cltv_expiry: if is_realistic_block_height(tx.locktime) {
                Some(tx.locktime)
            } else {
                None
            },
            csv_delays,
            ..Default::default()
        };
        Some((LightningTxType::HtlcTimeout, Confidence::Possible, params))
    } else {
        None
    }
}

/// Check if a locktime value is a realistic block height (not Lightning encoding, not 0).
fn is_realistic_block_height(locktime: u32) -> bool {
    locktime > 0 && locktime < 500_000_000 && (locktime >> 24) != 0x20
}

// ─── Parameter extraction helpers ───────────────────────────────────────────

fn extract_csv_delays_from_inputs(tx: &ApiTransaction) -> Vec<u16> {
    let mut delays = Vec::new();

    for vin in &tx.vin {
        if let Some(ref asm) = vin.inner_witnessscript_asm {
            let tokens: Vec<&str> = asm.split_whitespace().collect();
            for (i, token) in tokens.iter().enumerate() {
                if (*token == "OP_CHECKSEQUENCEVERIFY" || *token == "OP_CSV") && i > 0 {
                    if let Ok(val) = tokens[i - 1].parse::<u16>() {
                        delays.push(val);
                    }
                }
            }
        }
    }

    delays
}
