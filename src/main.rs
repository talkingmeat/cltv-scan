use anyhow::Result;
use clap::{Parser, Subcommand};

use cltv_scan::api::client::MempoolClient;
use cltv_scan::api::source::DataSource;
use cltv_scan::cli::output;
use cltv_scan::lightning::detector::classify_lightning;
use cltv_scan::timelock::extractor::analyze_transaction;

#[derive(Parser)]
#[command(name = "cltv-scan", about = "Bitcoin timelock vulnerability scanner")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze timelocks in a single transaction
    Tx {
        /// Transaction ID to analyze
        txid: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Scan all transactions in a block for timelocks
    Block {
        /// Block height to scan
        height: u64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Lightning Network transaction identification
    Lightning {
        #[command(subcommand)]
        command: LightningCommands,
    },
}

#[derive(Subcommand)]
enum LightningCommands {
    /// Classify a single transaction as Lightning-related
    Tx {
        /// Transaction ID to classify
        txid: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Scan a block for Lightning Network activity
    Block {
        /// Block height to scan
        height: u64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = MempoolClient::default();

    match cli.command {
        Commands::Tx { txid, json } => {
            let tx = client.get_transaction(&txid).await?;
            let analysis = analyze_transaction(&tx);

            if json {
                println!("{}", serde_json::to_string_pretty(&analysis)?);
            } else {
                output::print_transaction_analysis(&analysis);
            }
        }
        Commands::Block { height, json } => {
            eprintln!("Fetching block {height}...");
            let txs = client.get_all_block_txs(height).await?;
            eprintln!("Analyzing {} transactions...", txs.len());

            let analyses: Vec<_> = txs.iter().map(|tx| analyze_transaction(tx)).collect();

            if json {
                println!("{}", serde_json::to_string_pretty(&analyses)?);
            } else {
                output::print_block_summary(height, &analyses);
            }
        }
        Commands::Lightning { command } => match command {
            LightningCommands::Tx { txid, json } => {
                let tx = client.get_transaction(&txid).await?;
                let result = classify_lightning(&tx);

                if json {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    output::print_lightning_classification(&txid, &result);
                }
            }
            LightningCommands::Block { height, json } => {
                eprintln!("Fetching block {height}...");
                let txs = client.get_all_block_txs(height).await?;
                eprintln!("Classifying {} transactions...", txs.len());

                let results: Vec<_> = txs
                    .iter()
                    .map(|tx| (tx.txid.clone(), classify_lightning(tx)))
                    .collect();

                if json {
                    println!("{}", serde_json::to_string_pretty(&results)?);
                } else {
                    output::print_lightning_block_summary(height, &results);
                }
            }
        },
    }

    Ok(())
}
