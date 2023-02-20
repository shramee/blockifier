use std::convert::TryFrom;

use indexmap::IndexMap;
use papyrus_storage::header::{HeaderStorageReader, HeaderStorageWriter};
use papyrus_storage::state::{StateStorageReader, StateStorageWriter};
use pyo3::prelude::*;
use starknet_api::block::{BlockHash, BlockHeader, BlockNumber, BlockTimestamp, GasPrice};
use starknet_api::core::{ClassHash, ContractAddress, GlobalRoot};
use starknet_api::hash::StarkHash;
use starknet_api::state::{ContractClass, StateDiff};

use crate::errors::NativeBlockifierResult;
use crate::py_state_diff::PyBlockInfo;
use crate::PyStateDiff;

#[pyclass]
pub struct Storage {
    pub reader: papyrus_storage::StorageReader,
    pub writer: papyrus_storage::StorageWriter,
}

#[pymethods]
impl Storage {
    #[new]
    #[args(path)]
    pub fn new(path: String) -> NativeBlockifierResult<Storage> {
        let db_config = papyrus_storage::db::DbConfig {
            path,
            max_size: 1 << 35, // 32GB.
        };

        let (reader, writer) = papyrus_storage::open_storage(db_config)?;
        Ok(Storage { reader, writer })
    }

    /// Returns the next block number (the one that was not yet created).
    pub fn get_state_marker(&self) -> NativeBlockifierResult<u64> {
        let block_number = self.reader.begin_ro_txn()?.get_state_marker()?;
        Ok(block_number.0)
    }

    #[args(block_number)]
    pub fn get_block_hash(&self, block_number: u64) -> NativeBlockifierResult<Option<Vec<u8>>> {
        let block_number = BlockNumber(block_number);
        let block_hash = self
            .reader
            .begin_ro_txn()?
            .get_block_header(block_number)?
            .map(|block_header| Vec::from(block_header.block_hash.0.bytes()));
        Ok(block_hash)
    }

    #[args(block_number)]
    pub fn revert_state_diff(&mut self, block_number: u64) -> NativeBlockifierResult<()> {
        let block_number = BlockNumber(block_number);
        let revert_txn = self.writer.begin_rw_txn()?;
        let (revert_txn, _) = revert_txn.revert_state_diff(block_number)?;
        let revert_txn = revert_txn.revert_header(block_number)?;

        revert_txn.commit()?;
        Ok(())
    }

    #[args(block_number, py_state_diff, _py_deployed_contract_class_definitions)]
    /// Appends state diff and block header into Papyrus storage.
    pub fn append_state_diff(
        &mut self,
        block_id: u64,
        previous_block_id: u64,
        py_block_info: PyBlockInfo,
        py_state_diff: PyStateDiff,
        _py_deployed_contract_class_definitions: &PyAny,
    ) -> NativeBlockifierResult<()> {
        let block_number = BlockNumber(py_block_info.block_number);
        let state_diff = StateDiff::try_from(py_state_diff)?;
        // TODO: Figure out how to go from `py_state_diff.class_hash_to_compiled_class_hash` into
        // this type.
        let deployed_contract_class_definitions = IndexMap::<ClassHash, ContractClass>::new();
        let append_txn = self.writer.begin_rw_txn()?.append_state_diff(
            block_number,
            state_diff,
            deployed_contract_class_definitions,
        );
        let append_txn = append_txn?;

        let block_header = BlockHeader {
            block_hash: BlockHash(StarkHash::from(block_id)),
            parent_hash: BlockHash(StarkHash::from(previous_block_id)),
            block_number,
            gas_price: GasPrice(py_block_info.gas_price),
            state_root: GlobalRoot::default(),
            sequencer: ContractAddress::try_from(py_block_info.sequencer_address.0)?,
            timestamp: BlockTimestamp(py_block_info.block_timestamp),
        };
        let append_txn = append_txn.append_header(block_number, &block_header)?;

        append_txn.commit()?;
        Ok(())
    }
}