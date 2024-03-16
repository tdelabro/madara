use std::sync::Arc;

use ethers::providers::{Http, Provider, StreamExt};
use ethers::types::U256;
pub use mc_eth_client::config::EthereumClientConfig;
use mp_transactions::compute_hash::ComputeTransactionHash;
use pallet_starknet_runtime_api::{ConvertTransactionRuntimeApi, StarknetRuntimeApi};
use sc_client_api::HeaderBackend;
use sc_transaction_pool_api::{TransactionPool, TransactionSource};
use sp_api::ProvideRuntimeApi;
use sp_runtime::traits::Block as BlockT;
use starknet_api::transaction::Fee;
use starknet_core_contract_client::interfaces::{LogMessageToL2Filter, StarknetMessagingEvents};

use crate::contract::parse_handle_l1_message_transaction;
use crate::error::L1MessagesWorkerError;

const TX_SOURCE: TransactionSource = TransactionSource::External;

fn create_event_listener(
    config: EthereumClientConfig,
) -> Result<StarknetMessagingEvents<Provider<Http>>, mc_eth_client::error::Error> {
    let address = config.contracts.core_contract()?;
    let provider: Provider<Http> = config.provider.try_into()?;
    Ok(StarknetMessagingEvents::new(address, Arc::new(provider)))
}

pub async fn run_worker<C, P, B>(
    config: EthereumClientConfig,
    client: Arc<C>,
    pool: Arc<P>,
    backend: Arc<mc_db::Backend<B>>,
) where
    B: BlockT,
    C: ProvideRuntimeApi<B> + HeaderBackend<B>,
    C::Api: StarknetRuntimeApi<B> + ConvertTransactionRuntimeApi<B>,
    P: TransactionPool<Block = B> + 'static,
{
    log::info!("⟠ Starting L1 Messages Worker with settings: {:?}", config);

    let event_listener = match create_event_listener(config) {
        Ok(res) => res,
        Err(e) => {
            log::error!("⟠ Ethereum client config error: {:?}", e);
            return;
        }
    };

    let last_synced_event_block = match backend.messaging().last_synced_l1_block_with_event() {
        Ok(blknum) => blknum,
        Err(e) => {
            log::error!("⟠ Madara Messaging DB unavailable: {:?}", e);
            return;
        }
    };

    let events = event_listener.event::<LogMessageToL2Filter>().from_block(last_synced_event_block.block_number);
    let mut event_stream = match events.stream_with_meta().await {
        Ok(stream) => stream,
        Err(e) => {
            log::error!("⟠ Unexpected error with L1 event stream: {:?}", e);
            return;
        }
    };

    while let Some(event_res) = event_stream.next().await {
        if let Ok((event, meta)) = event_res {
            log::info!(
                "⟠ Processing L1 Message from block: {:?}, transaction_hash: {:?}, log_index: {:?}",
                meta.block_number,
                meta.transaction_hash,
                meta.log_index
            );

            match process_l1_message::<_, _, _, _>(
                event,
                &client,
                &pool,
                &backend,
                &meta.block_number.as_u64(),
                &meta.log_index.as_u64(),
            )
            .await
            {
                Ok(Some(tx_hash)) => {
                    log::info!(
                        "⟠ L1 Message from block: {:?}, transaction_hash: {:?}, log_index: {:?} submitted, \
                         transaction hash on L2: {:?}",
                        meta.block_number,
                        meta.transaction_hash,
                        meta.log_index,
                        tx_hash
                    );
                }
                Ok(None) => {}
                Err(e) => {
                    log::error!(
                        "⟠ Unexpected error while processing L1 Message from block: {:?}, transaction_hash: {:?}, \
                         log_index: {:?}, error: {:?}",
                        meta.block_number,
                        meta.transaction_hash,
                        meta.log_index,
                        e
                    )
                }
            }
        }
    }
}

async fn process_l1_message<C, P, B, PE>(
    event: LogMessageToL2Filter,
    client: &Arc<C>,
    pool: &Arc<P>,
    backend: &Arc<mc_db::Backend<B>>,
    l1_block_number: &u64,
    event_index: &u64,
) -> Result<Option<P::Hash>, L1MessagesWorkerError<PE>>
where
    B: BlockT,
    C: ProvideRuntimeApi<B> + HeaderBackend<B>,
    C::Api: StarknetRuntimeApi<B> + ConvertTransactionRuntimeApi<B>,
    P: TransactionPool<Block = B, Error = PE> + 'static,
    PE: std::error::Error,
{
    // Check against panic
    // https://docs.rs/ethers/latest/ethers/types/struct.U256.html#method.as_u128
    let paid_fee_on_l1: Fee = if event.fee > U256::from_big_endian(&(u128::MAX.to_be_bytes())) {
        return Err(L1MessagesWorkerError::ToFeeError);
    } else {
        Fee(event.fee.as_u128())
    };
    let tx = parse_handle_l1_message_transaction(event)?;

    let best_block_hash = client.info().best_hash;

    match client.runtime_api().l1_nonce_unused(best_block_hash, tx.nonce) {
        Ok(true) => Ok(()),
        Ok(false) => {
            log::debug!("⟠ Event already processed: {:?}", tx);
            return Ok(None);
        }
        Err(e) => {
            log::error!("⟠ Unexpected Runtime Api error: {:?}", e);
            Err(L1MessagesWorkerError::RuntimeApiError(e))
        }
    }?;

    let chain_id = client.runtime_api().chain_id(best_block_hash).map_err(L1MessagesWorkerError::RuntimeApiError)?;
    // TODO: Find a way not to hardcode that
    let tx_hash = tx.compute_hash(chain_id, false);
    let transaction = blockifier::transaction::transactions::L1HandlerTransaction { tx, tx_hash, paid_fee_on_l1 };

    let extrinsic = client.runtime_api().convert_l1_transaction(best_block_hash, transaction).map_err(|e| {
        log::error!("⟠ Failed to convert L1 Transaction via Runtime Api: {:?}", e);
        L1MessagesWorkerError::ConvertTransactionRuntimeApiError(e)
    })?;

    let tx_hash = pool.submit_one(best_block_hash, TX_SOURCE, extrinsic).await.map_err(|e| {
        log::error!("⟠ Failed to submit transaction with L1 Message: {:?}", e);
        L1MessagesWorkerError::SubmitTxError(e)
    })?;

    backend
        .messaging()
        .update_last_synced_l1_block_with_event(&mc_db::LastSyncedEventBlock::new(
            l1_block_number.to_owned(),
            event_index.to_owned(),
        ))
        .map_err(|e| {
            log::error!("⟠ Failed to save last L1 synced block: {:?}", e);
            L1MessagesWorkerError::DatabaseError(e)
        })?;

    Ok(Some(tx_hash))
}
