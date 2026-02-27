use crate::lightning::types::{Confidence, LightningClassification, LightningTxType};
use crate::timelock::types::{SequenceMeaning, TransactionAnalysis};

pub fn print_transaction_analysis(analysis: &TransactionAnalysis) {
    println!("Transaction: {}", analysis.txid);
    println!("{}", "─".repeat(72));

    // nLockTime
    println!(
        "nLockTime:   {} {}",
        analysis.nlocktime.human_readable,
        if analysis.nlocktime.raw_value > 0 {
            format!("(raw: {})", analysis.nlocktime.raw_value)
        } else {
            String::new()
        }
    );
    println!();

    // Inputs / sequences
    println!("Inputs ({}):", analysis.inputs.len());
    for input in &analysis.inputs {
        let meaning = match &input.meaning {
            SequenceMeaning::Final => "final".to_string(),
            SequenceMeaning::LocktimeEnabled => "locktime enabled".to_string(),
            SequenceMeaning::RbfEnabled => "RBF + locktime".to_string(),
            SequenceMeaning::RelativeTimelock => {
                if let Some(ref rtl) = input.relative_timelock {
                    format!("relative timelock: {}", rtl.human_readable)
                } else {
                    "relative timelock".to_string()
                }
            }
            SequenceMeaning::NonStandard => "non-standard".to_string(),
        };
        println!("  [{}] {} — {}", input.input_index, input.raw_hex, meaning);
    }

    // CLTV
    if !analysis.cltv_timelocks.is_empty() {
        println!();
        println!("OP_CHECKLOCKTIMEVERIFY ({}):", analysis.cltv_timelocks.len());
        for tl in &analysis.cltv_timelocks {
            println!(
                "  input[{}] {}: {} (raw: {})",
                tl.input_index, tl.script_field, tl.human_readable, tl.raw_value
            );
        }
    }

    // CSV
    if !analysis.csv_timelocks.is_empty() {
        println!();
        println!("OP_CHECKSEQUENCEVERIFY ({}):", analysis.csv_timelocks.len());
        for tl in &analysis.csv_timelocks {
            println!(
                "  input[{}] {}: {} (raw: {})",
                tl.input_index, tl.script_field, tl.human_readable, tl.raw_value
            );
        }
    }

    // Summary
    println!();
    if analysis.summary.has_active_timelocks {
        let mut parts = Vec::new();
        if analysis.summary.nlocktime_active {
            parts.push("nLockTime".to_string());
        }
        if analysis.summary.relative_timelock_count > 0 {
            parts.push(format!("{} nSequence", analysis.summary.relative_timelock_count));
        }
        if analysis.summary.cltv_count > 0 {
            parts.push(format!("{} CLTV", analysis.summary.cltv_count));
        }
        if analysis.summary.csv_count > 0 {
            parts.push(format!("{} CSV", analysis.summary.csv_count));
        }
        println!("Active timelocks: {}", parts.join(", "));
    } else {
        println!("No active timelocks.");
    }
}

pub fn print_lightning_classification(txid: &str, lc: &LightningClassification) {
    println!("Transaction: {txid}");
    println!("{}", "─".repeat(72));

    match lc.tx_type {
        None => println!("Lightning: not identified"),
        Some(ref t) => {
            let type_str = match t {
                LightningTxType::Commitment => "Commitment (force-close)",
                LightningTxType::HtlcTimeout => "HTLC-timeout (refund)",
                LightningTxType::HtlcSuccess => "HTLC-success (claim)",
            };
            let conf = match lc.confidence {
                Confidence::None => "none",
                Confidence::Possible => "possible",
                Confidence::HighlyLikely => "highly likely",
            };
            println!("Lightning:   {type_str} [{conf}]");
        }
    }

    // Commitment signals
    let s = &lc.commitment_signals;
    if s.locktime_match || s.sequence_match || s.has_anchor_outputs {
        println!();
        println!("Commitment signals:");
        if s.locktime_match {
            println!("  locktime in 0x20 range (Lightning encoding)");
        }
        if s.sequence_match {
            println!("  sequence with 0x80 upper byte");
        }
        if s.has_anchor_outputs {
            println!("  {} anchor output(s) (330 sats)", s.anchor_output_count);
        }
    }

    // Extracted parameters
    let p = &lc.params;
    let has_params = p.commitment_number.is_some()
        || p.cltv_expiry.is_some()
        || p.preimage_revealed
        || !p.csv_delays.is_empty()
        || p.htlc_output_count.is_some();

    if has_params {
        println!();
        println!("Parameters:");
        if let Some(cn) = p.commitment_number {
            println!("  commitment number: {cn} (obscured)");
        }
        if let Some(count) = p.htlc_output_count {
            println!("  HTLC outputs: {count}");
        }
        if let Some(expiry) = p.cltv_expiry {
            println!("  CLTV expiry: block {expiry}");
        }
        if p.preimage_revealed {
            if let Some(ref pre) = p.preimage {
                println!("  preimage: {pre}");
            } else {
                println!("  preimage: revealed");
            }
        }
        if !p.csv_delays.is_empty() {
            let delays: Vec<String> = p.csv_delays.iter().map(|d| format!("{d} blocks")).collect();
            println!("  CSV delays: {}", delays.join(", "));
        }
    }
}

pub fn print_lightning_block_summary(
    height: u64,
    results: &[(String, LightningClassification)],
) {
    let lightning_txs: Vec<_> = results.iter().filter(|(_, lc)| lc.tx_type.is_some()).collect();

    let commitments = lightning_txs.iter().filter(|(_, lc)| lc.tx_type == Some(LightningTxType::Commitment)).count();
    let htlc_timeouts = lightning_txs.iter().filter(|(_, lc)| lc.tx_type == Some(LightningTxType::HtlcTimeout)).count();
    let htlc_successes = lightning_txs.iter().filter(|(_, lc)| lc.tx_type == Some(LightningTxType::HtlcSuccess)).count();

    println!("Block {height} — Lightning Activity");
    println!("{}", "═".repeat(72));
    println!(
        "{} transactions scanned, {} Lightning-related",
        results.len(),
        lightning_txs.len()
    );

    if !lightning_txs.is_empty() {
        println!(
            "  {} commitment (force-close), {} HTLC-timeout, {} HTLC-success",
            commitments, htlc_timeouts, htlc_successes
        );
    }
    println!();

    if lightning_txs.is_empty() {
        println!("No Lightning transactions identified in this block.");
        return;
    }

    for (txid, lc) in &lightning_txs {
        print_lightning_classification(txid, lc);
        println!();
    }
}

pub fn print_block_summary(height: u64, analyses: &[TransactionAnalysis]) {
    let total = analyses.len();
    let with_timelocks: Vec<_> = analyses.iter().filter(|a| a.summary.has_active_timelocks).collect();

    println!("Block {height}");
    println!("{}", "═".repeat(72));
    println!(
        "{total} transactions, {} with active timelocks",
        with_timelocks.len()
    );
    println!();

    if with_timelocks.is_empty() {
        println!("No active timelocks found in this block.");
        return;
    }

    for analysis in &with_timelocks {
        print_transaction_analysis(analysis);
        println!();
    }
}
