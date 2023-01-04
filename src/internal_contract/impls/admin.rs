// Copyright 2019 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

use crate::{
    observer::{AddressPocket, VmObserve},
    state::{cleanup_mode, Substate},
    vm::{self, Spec},
};
use cfx_state::state_trait::StateOpsTrait;
use cfx_types::{AddressWithSpace, U256};

/// The Actual Implementation of `suicide`.
/// The contract which has non zero `collateral_for_storage` cannot suicide,
/// otherwise it will:
///   1. refund collateral for code
///   2. refund sponsor balance
///   3. refund contract balance
///   4. kill the contract
pub fn suicide(
    contract_address: &AddressWithSpace,
    refund_address: &AddressWithSpace,
    state: &mut dyn StateOpsTrait,
    spec: &Spec,
    substate: &mut Substate,
    tracer: &mut dyn VmObserve,
    account_start_nonce: U256,
) -> vm::Result<()> {
    substate.suicides.insert(contract_address.clone());
    let balance = state.balance(contract_address)?;

    if refund_address == contract_address {
        tracer.trace_internal_transfer(
            AddressPocket::Balance(*contract_address),
            AddressPocket::MintBurn,
            balance,
        );
        // When destroying, the balance will be burnt.
        state.sub_balance(
            contract_address,
            &balance,
            &mut cleanup_mode(substate, spec),
        )?;
        state.subtract_total_issued(balance);
    } else {
        trace!(target: "context", "Destroying {} -> {} (xfer: {})", contract_address.address, refund_address.address, balance);
        tracer.trace_internal_transfer(
            AddressPocket::Balance(*contract_address),
            AddressPocket::Balance(*refund_address),
            balance,
        );
        state.transfer_balance(
            contract_address,
            refund_address,
            &balance,
            cleanup_mode(substate, spec),
            account_start_nonce,
        )?;
    }

    Ok(())
}
