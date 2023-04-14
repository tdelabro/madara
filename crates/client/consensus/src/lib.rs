// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0
// This file is part of Frontier.
//
// Copyright (c) 2020-2022 Parity Technologies (UK) Ltd.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use std::marker::PhantomData;
use std::ops::Sub;
use std::sync::Arc;

use mp_digest_log::{find_post_log, FindLogError};
use sc_consensus::{BlockCheckParams, BlockImport, BlockImportParams, ImportResult};
use sp_api::{Encode, HeaderT, ProvideRuntimeApi};
use sp_block_builder::BlockBuilder as BlockBuilderApi;
use sp_consensus::Error as ConsensusError;
use sp_core::{blake2_256, H256, U256};
use sp_runtime::traits::{Block as BlockT, UniqueSaturatedInto};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Multiple runtime Starknet log, rejecting!")]
    MultipleRuntimeLogs,
    #[error("Runtime Starknet log not found, rejecting!")]
    NoRuntimeLog,
    #[error(
        "Invalid parent block state commitment for block number {block_number}, expected: {expected:#?} got \
         {recieved:#?}, rejecting!"
    )]
    InvalidParentBlockStateCommitment { block_number: U256, expected: H256, recieved: H256 },
}

impl From<Error> for String {
    fn from(error: Error) -> String {
        error.to_string()
    }
}

impl From<FindLogError> for Error {
    fn from(error: FindLogError) -> Error {
        match error {
            FindLogError::NotFound => Error::NoRuntimeLog,
            FindLogError::MultipleLogs => Error::MultipleRuntimeLogs,
        }
    }
}

impl From<Error> for ConsensusError {
    fn from(error: Error) -> ConsensusError {
        ConsensusError::ClientImport(error.to_string())
    }
}

pub struct MadaraBlockImport<B: BlockT, I, C> {
    inner: I,
    client: Arc<C>,
    _marker: PhantomData<B>,
}

impl<Block: BlockT, I: Clone + BlockImport<Block>, C> Clone for MadaraBlockImport<Block, I, C> {
    fn clone(&self) -> Self {
        MadaraBlockImport { inner: self.inner.clone(), client: self.client.clone(), _marker: PhantomData }
    }
}

impl<B, I, C> MadaraBlockImport<B, I, C>
where
    B: BlockT,
    I: BlockImport<B, Transaction = sp_api::TransactionFor<C, B>>,
    I::Error: Into<ConsensusError>,
    C: ProvideRuntimeApi<B>,
    C::Api: BlockBuilderApi<B>,
{
    pub fn new(inner: I, client: Arc<C>) -> Self {
        Self { inner, client, _marker: PhantomData }
    }
}

#[async_trait::async_trait]
impl<B, I, C> BlockImport<B> for MadaraBlockImport<B, I, C>
where
    B: BlockT,
    I: BlockImport<B, Transaction = sp_api::TransactionFor<C, B>> + Send + Sync,
    I::Error: Into<ConsensusError>,
    C: ProvideRuntimeApi<B> + Send + Sync,
    C::Api: BlockBuilderApi<B>,
{
    type Error = ConsensusError;
    type Transaction = sp_api::TransactionFor<C, B>;

    async fn check_block(&mut self, block: BlockCheckParams<B>) -> Result<ImportResult, Self::Error> {
        self.inner.check_block(block).await.map_err(Into::into)
    }

    async fn import_block(
        &mut self,
        block: BlockImportParams<B, Self::Transaction>,
    ) -> Result<ImportResult, Self::Error> {
        let digest = block.header.digest();
        let block_number = U256::from(UniqueSaturatedInto::<u128>::unique_saturated_into(*block.header.number()));

        let logs = find_post_log(digest).map_err(|e| Self::Error::Other(Box::new(e)))?;

        // TODO:  start computing it when the parent block is recieved
        // Put the logic in the crates client/mapping-sync and store the result in client/db
        let expected_state_commitment =
            if block_number.is_zero() { H256::zero() } else { blake2_256(&block_number.sub(1).encode()).into() };

        if logs.parent_block_state_commitment != expected_state_commitment {
            return Err(Self::Error::Other(Box::new(Error::InvalidParentBlockStateCommitment {
                block_number,
                expected: expected_state_commitment,
                recieved: logs.parent_block_state_commitment,
            })));
        }

        self.inner.import_block(block).await.map_err(Into::into)
    }
}
