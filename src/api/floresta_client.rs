use std::fs;
use std::sync::Arc;

use anyhow::{Context, Result};
use bitcoin::{Network, Txid};
use floresta_node::{Config, Florestad};
use floresta_rpc::jsonrpc_client::Client as FlorestaRpcClient;
use floresta_rpc::rpc::FlorestaRPC;
use floresta_rpc::rpc_types::{GetBlockRes, RawTx};
use once_cell::sync::OnceCell;
use tokio::task::spawn_blocking;
use tokio::sync::OnceCell as AsyncOnceCell;

use super::source::DataSource;
use super::types::{ApiPrevout, ApiStatus, ApiTransaction, ApiVin, ApiVout};

const FLORESTA_RPC_URL: &str = "http://127.0.0.1:38332";
static EMBEDDED_FLORESTA: AsyncOnceCell<()> = AsyncOnceCell::const_new();
static FLORESTA_CONFIG_INIT: OnceCell<Config> = OnceCell::new();

async fn ensure_embedded_floresta() -> Result<()> {
    EMBEDDED_FLORESTA
        .get_or_try_init(|| async {
            // Ensure data dir exists
            let data_dir = ".floresta-embedded-mainnet".to_string();
            fs::create_dir_all(&data_dir)
                .with_context(|| format!("creating embedded floresta data dir at {data_dir}"))?;

            // Base config
            let mut config = Config::new(Network::Bitcoin, data_dir.clone());
            config.json_rpc_address = Some("127.0.0.1:38332".to_string());
            config.log_to_stdout = false;
            config.log_to_file = false;
            config.user_agent = "cltv-scan/0.1.0".to_string();
            config.backfill = false;

            FLORESTA_CONFIG_INIT.set(config.clone()).ok();

            // Start node
            let node = Florestad::from_config(config);
            node.start()
                .await
                .map_err(|e| anyhow::anyhow!("starting embedded floresta node: {e}"))?;

            // Keep node alive for the duration of the process
            tokio::spawn(async move {
                // Wait until we're asked to stop; for now we just park this task.
                // Floresta's own tasks are already running.
                let _ = node.should_stop().await;
            });

            Ok(())
        })
        .await
        .map(|_| ())
}

pub struct FlorestaClient {
    client: Arc<FlorestaRpcClient>,
}

impl FlorestaClient {
    pub fn new(rpc_url: &str) -> Self {
        Self {
            client: Arc::new(FlorestaRpcClient::new(rpc_url.to_string())),
        }
    }

    /// Defaults to a local florestad instance on the standard RPC port.
    pub fn default() -> Self {
        Self::new(FLORESTA_RPC_URL)
    }

    fn map_raw_tx_to_api(tx: RawTx) -> ApiTransaction {
        let vin = tx
            .vin
            .into_iter()
            .map(|input| {
                let is_coinbase = input.txid.chars().all(|c| c == '0');

                ApiVin {
                    txid: Some(input.txid),
                    vout: Some(input.vout),
                    prevout: None::<ApiPrevout>,
                    scriptsig: Some(input.script_sig.hex),
                    scriptsig_asm: Some(input.script_sig.asm),
                    inner_redeemscript_asm: None,
                    inner_witnessscript_asm: None,
                    witness: Some(input.witness),
                    is_coinbase,
                    sequence: input.sequence,
                }
            })
            .collect();

        let vout = tx
            .vout
            .into_iter()
            .map(|output| {
                let addr = if output.script_pub_key.address.is_empty() {
                    None
                } else {
                    Some(output.script_pub_key.address)
                };

                ApiVout {
                    scriptpubkey: output.script_pub_key.hex,
                    scriptpubkey_asm: output.script_pub_key.asm,
                    scriptpubkey_type: output.script_pub_key.type_,
                    scriptpubkey_address: addr,
                    value: output.value,
                }
            })
            .collect();

        let status = ApiStatus {
            confirmed: tx.confirmations > 0,
            block_height: None,
            block_hash: if tx.blockhash.is_empty() {
                None
            } else {
                Some(tx.blockhash)
            },
            block_time: Some(tx.blocktime as u64),
        };

        ApiTransaction {
            txid: tx.txid,
            version: tx.version as i32,
            locktime: tx.locktime,
            vin,
            vout,
            size: tx.size as u64,
            weight: tx.weight as u64,
            fee: None,
            status,
        }
    }
}

impl DataSource for FlorestaClient {
    async fn get_transaction(&self, txid: &str) -> Result<ApiTransaction> {
        ensure_embedded_floresta().await?;

        let client = self.client.clone();
        let txid = txid.parse::<Txid>()?;

        let raw = spawn_blocking(move || {
            let value = client.get_transaction(txid, Some(true))?;
            let tx: RawTx = serde_json::from_value(value)?;
            Ok::<_, anyhow::Error>(tx)
        })
        .await??;

        Ok(Self::map_raw_tx_to_api(raw))
    }

    async fn get_transaction_hex(&self, txid: &str) -> Result<String> {
        ensure_embedded_floresta().await?;

        let client = self.client.clone();
        let txid = txid.parse::<Txid>()?;

        let hex = spawn_blocking(move || {
            let value = client.get_transaction(txid, Some(false))?;
            let s = value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("unexpected RPC type for gettransaction"))?
                .to_string();
            Ok::<_, anyhow::Error>(s)
        })
        .await??;

        Ok(hex)
    }

    async fn get_block_txs(&self, hash: &str, start_index: u32) -> Result<Vec<ApiTransaction>> {
        ensure_embedded_floresta().await?;

        let client = self.client.clone();
        let hash = hash.parse()?;

        let txs = spawn_blocking(move || -> Result<Vec<ApiTransaction>> {
            let block = client.get_block(hash, Some(1))?;
            let verbose = match block {
                GetBlockRes::One(b) => b,
                GetBlockRes::Zero(_) => anyhow::bail!("unexpected non-verbose block response"),
            };

            let mut out = Vec::new();
            for txid_str in verbose.tx {
                let txid: Txid = txid_str.parse()?;
                let value = client.get_transaction(txid, Some(true))?;
                let raw: RawTx = serde_json::from_value(value)?;
                out.push(FlorestaClient::map_raw_tx_to_api(raw));
            }

            let start = usize::try_from(start_index).unwrap_or(0);
            let end = (start + 25).min(out.len());
            Ok(out.get(start..end).unwrap_or(&[]).to_vec())
        })
        .await??;

        Ok(txs)
    }

    async fn get_block_tip_height(&self) -> Result<u64> {
        ensure_embedded_floresta().await?;

        let client = self.client.clone();

        let height = spawn_blocking(move || {
            let h = client.get_block_count()?;
            Ok::<_, anyhow::Error>(u64::from(h))
        })
        .await??;

        Ok(height)
    }

    async fn get_block_hash(&self, height: u64) -> Result<String> {
        ensure_embedded_floresta().await?;

        let client = self.client.clone();
        let height_u32 = u32::try_from(height)?;

        let hash = spawn_blocking(move || {
            let h = client.get_block_hash(height_u32)?;
            Ok::<_, anyhow::Error>(h.to_string())
        })
        .await??;

        Ok(hash)
    }

    async fn get_all_block_txs(&self, height: u64) -> Result<Vec<ApiTransaction>> {
        ensure_embedded_floresta().await?;

        let client = self.client.clone();
        let height_u32 = u32::try_from(height)?;

        let txs = spawn_blocking(move || -> Result<Vec<ApiTransaction>> {
            let hash = client.get_block_hash(height_u32)?;
            let block = client.get_block(hash, Some(1))?;
            let verbose = match block {
                GetBlockRes::One(b) => b,
                GetBlockRes::Zero(_) => anyhow::bail!("unexpected non-verbose block response"),
            };

            let mut out = Vec::new();
            for txid_str in verbose.tx {
                let txid: Txid = txid_str.parse()?;
                let value = client.get_transaction(txid, Some(true))?;
                let raw: RawTx = serde_json::from_value(value)?;
                out.push(FlorestaClient::map_raw_tx_to_api(raw));
            }

            Ok(out)
        })
        .await??;

        Ok(txs)
    }
}

#[cfg(test)]
mod tests {
    use super::FlorestaClient;
    use super::DataSource;

    #[tokio::test]
    async fn print_first_10_txs_from_tip_block() {
        let client = FlorestaClient::default();

        let tip_height = client
            .get_block_tip_height()
            .await
            .expect("failed to get tip height");

        let txs = client
            .get_all_block_txs(tip_height)
            .await
            .expect("failed to get block transactions");

        for tx in txs.iter().take(10) {
            println!("{}", tx.txid);
        }
    }
}


