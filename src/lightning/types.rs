use serde::Serialize;

/// Confidence level for Lightning transaction identification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// No Lightning signals detected.
    None,
    /// Some signals match but not conclusive.
    Possible,
    /// Multiple strong signals align.
    HighlyLikely,
}

/// What type of Lightning transaction this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LightningTxType {
    /// Force-close: spends funding output, creates to_local/to_remote/HTLC outputs.
    Commitment,
    /// Refund path: HTLC expired, no preimage revealed.
    HtlcTimeout,
    /// Claim path: preimage revealed on-chain.
    HtlcSuccess,
}

/// Signals found when checking for commitment transaction patterns.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CommitmentSignals {
    /// Locktime is in the Lightning encoding range (upper byte 0x20).
    pub locktime_match: bool,
    /// At least one input has sequence with upper byte 0x80.
    pub sequence_match: bool,
    /// At least one output has exactly 330 satoshis (anchor output).
    pub has_anchor_outputs: bool,
    /// Number of anchor outputs found (0, 1, or 2).
    pub anchor_output_count: usize,
}

/// Signals found when checking for HTLC second-stage transaction patterns.
#[derive(Debug, Clone, Default, Serialize)]
pub struct HtlcSignals {
    /// nLockTime is a realistic block height (for timeout) or 0 (for success).
    pub locktime_value: u32,
    /// Whether a 32-byte preimage was found in witness data.
    pub has_preimage: bool,
    /// The preimage hex if found.
    pub preimage: Option<String>,
    /// Whether OP_CHECKLOCKTIMEVERIFY was found in the witness script.
    pub script_has_cltv: bool,
    /// Whether OP_CHECKSEQUENCEVERIFY was found in the witness script.
    pub script_has_csv: bool,
}

/// Complete Lightning identification result for a transaction.
#[derive(Debug, Clone, Serialize)]
pub struct LightningClassification {
    pub tx_type: Option<LightningTxType>,
    pub confidence: Confidence,
    pub commitment_signals: CommitmentSignals,
    pub htlc_signals: HtlcSignals,
    pub params: LightningParams,
}

/// Extracted Lightning-specific parameters.
#[derive(Debug, Clone, Default, Serialize)]
pub struct LightningParams {
    /// Obscured commitment number (from locktime + sequence encoding).
    pub commitment_number: Option<u64>,
    /// Number of HTLC outputs on a commitment transaction.
    pub htlc_output_count: Option<usize>,
    /// CLTV expiry block height (from HTLC-timeout nLockTime).
    pub cltv_expiry: Option<u32>,
    /// CSV delay values found in output scripts.
    pub csv_delays: Vec<u16>,
    /// Whether a preimage was revealed (HTLC-success).
    pub preimage_revealed: bool,
    /// The preimage itself if revealed.
    pub preimage: Option<String>,
}
