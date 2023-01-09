// Copyright 2019 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

use crate::vm;
use cfx_internal_common::{ChainIdParams, ChainIdParamsInner};
use cfx_parameters::block::EVM_TRANSACTION_GAS_RATIO;
use cfx_types::{AllChainID, U256};
use primitives::{BlockHeight, BlockNumber};

#[derive(Debug)]
pub struct CommonParams {
    /// Account start nonce.
    pub account_start_nonce: U256,
    /// Maximum size of extra data.
    pub maximum_extra_data_size: usize,
    /// Network id.
    pub network_id: u64,
    /// Chain id.
    pub chain_id: ChainIdParams,
    /// Minimum gas limit.
    pub min_gas_limit: U256,
    /// Gas limit bound divisor (how much gas limit can change per block)
    pub gas_limit_bound_divisor: U256,
    /// Number of first block where max code size limit is active.
    /// Maximum size of transaction's RLP payload.
    pub max_transaction_size: usize,
    /// The gas ratio of evm transactions for the block can pack the EVM
    /// transactions
    pub evm_transaction_gas_ratio: u64,

    /// Set the internal contracts to state at the genesis blocks, even if it
    /// is not activated.
    pub early_set_internal_contracts_states: bool,
    /// The upgrades activated at given block number.
    pub transition_numbers: TransitionsBlockNumber,
    /// The upgrades activated at given block height (a.k.a. epoch number).
    pub transition_heights: TransitionsEpochHeight,
}

#[derive(Default, Debug, Clone)]
pub struct TransitionsBlockNumber {
    /// CIP43: Introduce Finality via Voting Among Staked
    pub cip43a: BlockNumber,
    pub cip43b: BlockNumber,
    /// CIP62: Enable EC-related builtin contract
    pub cip62: BlockNumber,
    /// CIP64: Get current epoch number through internal contract
    pub cip64: BlockNumber,
    /// CIP71: Configurable anti-reentrancy
    pub cip71: BlockNumber,
    /// CIP78: Correct `is_sponsored` fields in receipt
    pub cip78a: BlockNumber,
    /// CIP78: Correct `is_sponsored` fields in receipt
    pub cip78b: BlockNumber,
    /// CIP90: Two Space for Transaction Execution
    pub cip90b: BlockNumber,
    /// CIP92: Enable Blake2F builtin function
    pub cip92: BlockNumber,
    /// CIP-94: On-chain Parameter DAO Vote
    pub cip94: BlockNumber,
    /// CIP-97: Remove Staking List
    pub cip97: BlockNumber,
    /// CIP-98: Fix BLOCKHASH in espace
    pub cip98: BlockNumber,
    /// CIP-105: PoS staking based minimal votes.
    pub cip105: BlockNumber,
    pub cip_sigma_fix: BlockNumber,
}

#[derive(Default, Debug, Clone)]
pub struct TransitionsEpochHeight {
    /// The height to change block base reward.
    /// The block `custom` field of this height is required to be
    /// `tanzanite_transition_header_custom`.
    pub cip40: BlockHeight,
    /// CIP76: Remove VM-related constraints in syncing blocks
    pub cip76: BlockHeight,
    /// CIP86: Difficulty adjustment.
    pub cip86: BlockHeight,
    /// CIP90: Two Space for Transaction Execution
    pub cip90a: BlockHeight,
    /// CIP94 Hardfork enable heights.
    pub cip94: BlockHeight,
}

impl Default for CommonParams {
    fn default() -> Self {
        CommonParams {
            account_start_nonce: 0x00.into(),
            maximum_extra_data_size: 0x20,
            network_id: 0x1,
            chain_id: ChainIdParamsInner::new_simple(AllChainID::new(1, 1)),
            min_gas_limit: 10_000_000.into(),
            gas_limit_bound_divisor: 0x0400.into(),
            max_transaction_size: 300 * 1024,
            evm_transaction_gas_ratio: EVM_TRANSACTION_GAS_RATIO,
            early_set_internal_contracts_states: false,
            transition_numbers: Default::default(),
            transition_heights: Default::default(),
        }
    }
}

impl CommonParams {
    pub fn spec(&self, number: BlockNumber) -> vm::Spec {
        vm::Spec::new_spec_from_common_params(&self, number)
    }
}
