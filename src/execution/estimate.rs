use super::executed::{ExecutionError, ExecutionOutcome};
use super::TXExecutor;
use super::TransactOptions;

use cfx_parameters::consensus::ONE_CFX_IN_DRIP;
use cfx_state::CleanupMode;
use cfx_statedb::Result as DbResult;
use cfx_types::{Address, AddressSpaceUtil, U256};
use primitives::SignedTransaction;
use std::{
    cmp::{max, min},
    ops::Shl,
};

#[derive(Debug, Clone, Copy)]
pub struct EstimateRequest {
    pub has_sender: bool,
    pub has_gas_limit: bool,
    pub has_gas_price: bool,
    pub has_nonce: bool,
    pub has_storage_limit: bool,
}

impl EstimateRequest {
    fn recheck_gas_fee(&self) -> bool {
        self.has_sender && self.has_gas_price
    }

    pub(super) fn charge_gas(&self) -> bool {
        self.has_sender && self.has_gas_limit && self.has_gas_price
    }
}

impl<'a> TXExecutor<'a> {
    pub fn transact_virtual(
        &mut self,
        mut tx: SignedTransaction,
        request: EstimateRequest,
    ) -> DbResult<ExecutionOutcome> {
        if !request.has_sender {
            let random_hex = Address::random();

            tx.sender = random_hex;
            tx.public = None;

            // If the sender is not specified, give it enough balance: 1 billion
            // CFX.
            let balance_inc = min(
                tx.value()
                    .saturating_add(U256::from(1_000_000_000) * ONE_CFX_IN_DRIP),
                U256::one().shl(128),
            );

            self.state.add_balance(
                &random_hex.with_space(tx.space()),
                &balance_inc,
                CleanupMode::NoEmpty,
                self.spec.account_start_nonce,
            )?;
            // Make sure statistics are also correct and will not violate any
            // underlying assumptions.
            self.state.add_total_issued(balance_inc);
        }

        if request.has_nonce {
            self.state.set_nonce(&tx.sender(), &tx.nonce())?;
        } else {
            *tx.nonce_mut() = self.state.nonce(&tx.sender())?;
        }

        let balance = self.state.balance(&tx.sender())?;

        // For the same transaction, the storage limit paid by user and the
        // storage limit paid by the sponsor are different values. So
        // this function will
        //
        // 1. First Pass: Assuming the sponsor pays for storage collateral,
        // check if the transaction will fail for
        // NotEnoughBalanceForStorage.
        //
        // 2. Second Pass: If it does, executes the transaction again assuming
        // the user pays for the storage collateral. The resultant
        // storage limit must be larger than the maximum storage limit
        // can be afford by the sponsor, to guarantee the user pays for
        // the storage limit.

        // First pass
        self.state.checkpoint();
        let sender_pay_executed =
            match self.transact(&tx, TransactOptions::estimate_first_pass(request))? {
                ExecutionOutcome::Finished(executed) => executed,
                res => {
                    return Ok(res);
                }
            };
        debug!(
            "Transaction estimate first pass outcome {:?}",
            sender_pay_executed
        );
        self.state.revert_to_checkpoint();

        let mut executed = sender_pay_executed;

        // Revise the gas used in result, if we estimate the transaction with a
        // default large enough gas.
        if !request.has_gas_limit {
            let estimated_gas_limit = executed.estimated_gas_limit.unwrap();
            executed.gas_charged = max(
                estimated_gas_limit - estimated_gas_limit / 4,
                executed.gas_used,
            );
            executed.fee = executed.gas_charged.saturating_mul(*tx.gas_price());
        }

        // If the request has a sender, recheck the balance requirement matched.
        if request.has_sender {
            // Unwrap safety: in given TransactOptions, this value must be
            // `Some(_)`.
            let gas_fee = if request.recheck_gas_fee() {
                executed
                    .estimated_gas_limit
                    .unwrap()
                    .saturating_mul(*tx.gas_price())
            } else {
                0.into()
            };
            let value_and_fee = tx.value().saturating_add(gas_fee);
            if balance < value_and_fee {
                return Ok(ExecutionOutcome::ExecutionErrorBumpNonce(
                    ExecutionError::NotEnoughCash {
                        required: value_and_fee.into(),
                        got: balance.into(),
                        actual_gas_cost: min(balance, gas_fee),
                    },
                    executed,
                ));
            }
        }

        assert!(!request.has_storage_limit);

        return Ok(ExecutionOutcome::Finished(executed));
    }
}
