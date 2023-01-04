use crate::{
    evm::FinalizationResult,
    vm::{self, ReturnData},
};

use cfx_types::{Address, AddressWithSpace, Space, U256};

/// The result contains more data than finalization result.
#[derive(Debug)]
pub struct FrameResult {
    /// Space
    pub space: Space,
    /// Final amount of gas left.
    pub gas_left: U256,
    /// Apply execution state changes or revert them.
    pub apply_state: bool,
    /// Return data buffer.
    pub return_data: ReturnData,
    /// Create address.
    pub create_address: Option<Address>,
}

impl Into<FinalizationResult> for FrameResult {
    fn into(self) -> FinalizationResult {
        FinalizationResult {
            space: self.space,
            gas_left: self.gas_left,
            apply_state: self.apply_state,
            return_data: self.return_data,
        }
    }
}

impl FrameResult {
    pub(super) fn new(result: FinalizationResult, create_address: Option<Address>) -> Self {
        FrameResult {
            space: result.space,
            gas_left: result.gas_left,
            apply_state: result.apply_state,
            return_data: result.return_data,
            create_address,
        }
    }
}

/// Convert a finalization result into a VM message call result.
pub fn into_message_call_result(result: vm::Result<FrameResult>) -> vm::MessageCallResult {
    match result {
        Ok(FrameResult {
            gas_left,
            return_data,
            apply_state: true,
            ..
        }) => vm::MessageCallResult::Success(gas_left, return_data),
        Ok(FrameResult {
            gas_left,
            return_data,
            apply_state: false,
            ..
        }) => vm::MessageCallResult::Reverted(gas_left, return_data),
        Err(err) => vm::MessageCallResult::Failed(err),
    }
}

/// Convert a finalization result into a VM contract create result.
pub fn into_contract_create_result(result: vm::Result<FrameResult>) -> vm::ContractCreateResult {
    match result {
        Ok(FrameResult {
            space,
            gas_left,
            apply_state: true,
            create_address,
            ..
        }) => {
            // Move the change of contracts_created in substate to
            // process_return.
            let address = create_address.expect("ExecutiveResult for Create frame should be some.");
            let address = AddressWithSpace { address, space };
            vm::ContractCreateResult::Created(address, gas_left)
        }
        Ok(FrameResult {
            gas_left,
            apply_state: false,
            return_data,
            ..
        }) => vm::ContractCreateResult::Reverted(gas_left, return_data),
        Err(err) => vm::ContractCreateResult::Failed(err),
    }
}
