// Copyright 2019 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

use super::{context::OriginInfo, Executed, ExecutionError};
use crate::{
    builtin::Builtin,
    bytes::Bytes,
    evm::{FinalizationResult, Finalize},
    executive::{
        context::LocalContext,
        executed::{ExecutionOutcome, ToRepackError, TxDropError},
        internal_contract::InternalContractTrait,
        vm_exec::{BuiltinExec, InternalContractExec, NoopExec},
    },
    hash::keccak,
    machine::Machine,
    observer::{tracer::ExecutiveTracer, AddressPocket, GasMan, StateTracer, VmObserve},
    state::{cleanup_mode, CallStackInfo, Substate},
    // verification::VerificationConfig,
    vm::{
        self, ActionParams, ActionValue, CallType, CreateContractAddress, CreateType, Env, Exec,
        ExecTrapError, ExecTrapResult, GasLeft, ResumeCall, ResumeCreate, ReturnData, Spec,
        TrapError, TrapResult,
    },
    vm_factory::VmFactory,
};
use cfx_parameters::{consensus::ONE_CFX_IN_DRIP, staking::*};
use cfx_state::{
    state_trait::StateOpsTrait, substate_trait::SubstateMngTrait, CleanupMode, StateTrait,
    SubstateTrait,
};
use cfx_statedb::Result as DbResult;
use cfx_types::{Address, AddressSpaceUtil, AddressWithSpace, Space, H256, U256, U512, U64};
use primitives::{
    receipt::StorageChange, storage::STORAGE_LAYOUT_REGULAR_V0, transaction::Action,
    SignedTransaction, StorageLayout,
};
use rlp::RlpStream;
use std::{
    cmp::{max, min},
    collections::HashSet,
    convert::{TryFrom, TryInto},
    ops::Shl,
    sync::Arc,
};

/// Calculate new contract address.
pub fn contract_address(
    address_scheme: CreateContractAddress,
    _block_number: U64,
    sender: &AddressWithSpace,
    nonce: &U256,
    code: &[u8],
) -> (AddressWithSpace, Option<H256>) {
    let code_hash = keccak(code);
    let (address, code_hash) = match address_scheme {
        CreateContractAddress::FromSenderNonce => {
            assert_eq!(sender.space, Space::Ethereum);
            let mut rlp = RlpStream::new_list(2);
            rlp.append(&sender.address);
            rlp.append(nonce);
            let h = Address::from(keccak(rlp.as_raw()));
            (h, Some(code_hash))
        }
        CreateContractAddress::FromSenderSaltAndCodeHash(salt) => {
            let mut buffer = [0u8; 1 + 20 + 32 + 32];
            buffer[0] = 0xff;
            buffer[1..(1 + 20)].copy_from_slice(&sender.address[..]);
            buffer[(1 + 20)..(1 + 20 + 32)].copy_from_slice(&salt[..]);
            buffer[(1 + 20 + 32)..].copy_from_slice(&code_hash[..]);
            // In Conflux, we use the first bit to indicate the type of the
            // address. For contract address, the bits will be set to 0x8.
            let h = Address::from(keccak(&buffer[..]));
            (h, Some(code_hash))
        }
    };
    return (address.with_space(sender.space), code_hash);
}

/// Convert a finalization result into a VM message call result.
pub fn into_message_call_result(result: vm::Result<ExecutiveResult>) -> vm::MessageCallResult {
    match result {
        Ok(ExecutiveResult {
            gas_left,
            return_data,
            apply_state: true,
            ..
        }) => vm::MessageCallResult::Success(gas_left, return_data),
        Ok(ExecutiveResult {
            gas_left,
            return_data,
            apply_state: false,
            ..
        }) => vm::MessageCallResult::Reverted(gas_left, return_data),
        Err(err) => vm::MessageCallResult::Failed(err),
    }
}

/// Convert a finalization result into a VM contract create result.
pub fn into_contract_create_result(
    result: vm::Result<ExecutiveResult>,
) -> vm::ContractCreateResult {
    match result {
        Ok(ExecutiveResult {
            space,
            gas_left,
            apply_state: true,
            create_address,
            ..
        }) => {
            // Move the change of contracts_created in substate to
            // process_return.
            let address =
                create_address.expect("ExecutiveResult for Create executive should be some.");
            let address = AddressWithSpace { address, space };
            vm::ContractCreateResult::Created(address, gas_left)
        }
        Ok(ExecutiveResult {
            gas_left,
            apply_state: false,
            return_data,
            ..
        }) => vm::ContractCreateResult::Reverted(gas_left, return_data),
        Err(err) => vm::ContractCreateResult::Failed(err),
    }
}

/// Transaction execution options.
pub struct TransactOptions {
    pub observer: Observer,
    pub check_settings: TransactCheckSettings,
}

impl TransactOptions {
    pub fn exec_with_tracing() -> Self {
        Self {
            observer: Observer::with_tracing(),
            check_settings: TransactCheckSettings::all_checks(),
        }
    }

    pub fn exec_with_no_tracing() -> Self {
        Self {
            observer: Observer::with_no_tracing(),
            check_settings: TransactCheckSettings::all_checks(),
        }
    }

    pub fn estimate_first_pass(request: EstimateRequest) -> Self {
        Self {
            observer: Observer::virtual_call(),
            check_settings: TransactCheckSettings::from_estimate_request(
                request,
                ChargeCollateral::EstimateSender,
            ),
        }
    }

    pub fn estimate_second_pass(request: EstimateRequest) -> Self {
        Self {
            observer: Observer::virtual_call(),
            check_settings: TransactCheckSettings::from_estimate_request(
                request,
                ChargeCollateral::EstimateSponsor,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ChargeCollateral {
    Normal,
    EstimateSender,
    EstimateSponsor,
}

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

    fn charge_gas(&self) -> bool {
        self.has_sender && self.has_gas_limit && self.has_gas_price
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TransactCheckSettings {
    pub charge_collateral: ChargeCollateral,
    pub charge_gas: bool,
    pub real_execution: bool,
    pub check_epoch_height: bool,
}

impl TransactCheckSettings {
    fn all_checks() -> Self {
        Self {
            charge_collateral: ChargeCollateral::Normal,
            charge_gas: true,
            real_execution: true,
            check_epoch_height: true,
        }
    }

    fn from_estimate_request(
        request: EstimateRequest,
        charge_collateral: ChargeCollateral,
    ) -> Self {
        Self {
            charge_collateral,
            charge_gas: request.charge_gas(),
            real_execution: false,
            check_epoch_height: false,
        }
    }
}

pub struct Observer {
    pub tracer: Option<ExecutiveTracer>,
    pub gas_man: Option<GasMan>,
    _noop: (),
}

impl Observer {
    pub fn as_vm_observe<'a>(&'a mut self) -> Box<dyn VmObserve + 'a> {
        match (self.tracer.as_mut(), self.gas_man.as_mut()) {
            (Some(tracer), Some(gas_man)) => Box::new((tracer, gas_man)),
            (Some(tracer), None) => Box::new(tracer),
            (None, Some(gas_man)) => Box::new(gas_man),
            (None, None) => Box::new(&mut self._noop),
        }
    }

    pub fn as_state_tracer(&mut self) -> &mut dyn StateTracer {
        match self.tracer.as_mut() {
            None => &mut self._noop,
            Some(tracer) => tracer,
        }
    }

    fn with_tracing() -> Self {
        Observer {
            tracer: Some(ExecutiveTracer::default()),
            gas_man: None,
            _noop: (),
        }
    }

    fn with_no_tracing() -> Self {
        Observer {
            tracer: None,
            gas_man: None,
            _noop: (),
        }
    }

    fn virtual_call() -> Self {
        Observer {
            tracer: Some(ExecutiveTracer::default()),
            gas_man: Some(GasMan::default()),
            _noop: (),
        }
    }
}

enum CallCreateExecutiveKind<'a> {
    Transfer,
    CallBuiltin(&'a Builtin),
    CallInternalContract(&'a Box<dyn InternalContractTrait>),
    ExecCall,
    ExecCreate,
}

pub struct CallCreateExecutive<'a, Substate: SubstateMngTrait> {
    context: LocalContext<'a, Substate>,
    factory: &'a VmFactory,
    status: ExecutiveStatus,
    create_address: Option<Address>,
    kind: CallCreateExecutiveKind<'a>,
}

pub enum ExecutiveStatus {
    Input(ActionParams),
    Running,
    ResumeCall(Box<dyn ResumeCall>),
    ResumeCreate(Box<dyn ResumeCreate>),
    Done,
}

impl<'a, Substate: SubstateMngTrait> CallCreateExecutive<'a, Substate> {
    /// Create a new call executive using raw data.
    pub fn new_call_raw(
        params: ActionParams,
        env: &'a Env,
        machine: &'a Machine,
        spec: &'a Spec,
        factory: &'a VmFactory,
        depth: usize,
        parent_static_flag: bool,
    ) -> Self {
        trace!(
            "Executive::call(params={:?}) self.env={:?}, parent_static={}",
            params,
            env,
            parent_static_flag,
        );

        let static_flag = parent_static_flag || params.call_type == CallType::StaticCall;

        let substate = Substate::new();
        // This logic is moved from function exec.
        let origin = OriginInfo::from(&params);
        let code_address = AddressWithSpace {
            address: params.code_address,
            space: params.space,
        };

        // Builtin is located for both Conflux Space and EVM Space.
        let kind = if let Some(builtin) = machine.builtin(&code_address, env.number) {
            trace!("CallBuiltin");
            CallCreateExecutiveKind::CallBuiltin(builtin)
        } else if let Some(internal) = machine.internal_contracts().contract(&code_address, spec) {
            debug!(
                "CallInternalContract: address={:?} data={:?}",
                code_address, params.data
            );
            CallCreateExecutiveKind::CallInternalContract(internal)
        } else {
            if params.code.is_some() {
                trace!("ExecCall");
                CallCreateExecutiveKind::ExecCall
            } else {
                trace!("Transfer");
                CallCreateExecutiveKind::Transfer
            }
        };
        let context = LocalContext::new(
            params.space,
            env,
            machine,
            spec,
            depth,
            origin,
            substate,
            /* is_create: */ false,
            static_flag,
        );
        Self {
            context,
            factory,
            // Instead of put params to Exective kind, we put it into status.
            status: ExecutiveStatus::Input(params),
            create_address: None,
            kind,
        }
    }

    /// Create a new create executive using raw data.
    pub fn new_create_raw(
        params: ActionParams,
        env: &'a Env,
        machine: &'a Machine,
        spec: &'a Spec,
        factory: &'a VmFactory,
        depth: usize,
        static_flag: bool,
    ) -> Self {
        trace!(
            "Executive::create(params={:?}) self.env={:?}, static={}",
            params,
            env,
            static_flag
        );

        let origin = OriginInfo::from(&params);

        let kind = CallCreateExecutiveKind::ExecCreate;

        let substate = Substate::new();

        let context = LocalContext::new(
            params.space,
            env,
            machine,
            spec,
            depth,
            origin,
            substate,
            /* is_create */ true,
            static_flag,
        );

        Self {
            context,
            create_address: Some(params.code_address),
            status: ExecutiveStatus::Input(params),
            factory,
            kind,
        }
    }

    /// This executive always contain an unconfirmed substate, returns a mutable
    /// reference to it.
    pub fn unconfirmed_substate(&mut self) -> &mut Substate {
        &mut self.context.substate
    }

    /// Get the recipient of this executive. The recipient is the address whose
    /// state will change.
    pub fn get_recipient(&self) -> &Address {
        &self.context.origin.recipient()
    }

    fn check_static_flag(
        params: &ActionParams,
        static_flag: bool,
        is_create: bool,
    ) -> vm::Result<()> {
        // This is the function check whether contract creation or value
        // transferring happens in static context at callee executive. However,
        // it is meaningless because the caller has checked this constraint
        // before message call. Currently, if we panic when this
        // function returns error, all the tests can still pass.
        // So we no longer check the logic for reentrancy here,
        // TODO: and later we will check if we can safely remove this function.
        if is_create {
            if static_flag {
                return Err(vm::Error::MutableCallInStaticContext);
            }
        } else {
            if static_flag
                && (params.call_type == CallType::StaticCall || params.call_type == CallType::Call)
                && params.value.value() > U256::zero()
            {
                return Err(vm::Error::MutableCallInStaticContext);
            }
        }

        Ok(())
    }

    fn transfer_exec_balance(
        params: &ActionParams,
        spec: &Spec,
        state: &mut dyn StateOpsTrait,
        substate: &mut dyn SubstateTrait,
        account_start_nonce: U256,
    ) -> DbResult<()> {
        let sender = AddressWithSpace {
            address: params.sender,
            space: params.space,
        };
        let receiver = AddressWithSpace {
            address: params.address,
            space: params.space,
        };
        if let ActionValue::Transfer(val) = params.value {
            state.transfer_balance(
                &sender,
                &receiver,
                &val,
                cleanup_mode(substate, &spec),
                account_start_nonce,
            )?;
        }

        Ok(())
    }

    fn transfer_exec_balance_and_init_contract(
        params: &ActionParams,
        spec: &Spec,
        state: &mut dyn StateOpsTrait,
        substate: &mut dyn SubstateTrait,
        storage_layout: Option<StorageLayout>,
    ) -> DbResult<()> {
        let sender = AddressWithSpace {
            address: params.sender,
            space: params.space,
        };
        let receiver = AddressWithSpace {
            address: params.address,
            space: params.space,
        };
        if let ActionValue::Transfer(val) = params.value {
            // It is possible to first send money to a pre-calculated
            // contract address.
            let prev_balance = state.balance(&receiver)?;
            state.sub_balance(&sender, &val, &mut cleanup_mode(substate, &spec))?;
            let nonce = U256::from(1);
            state.new_contract(
                &receiver,
                val.saturating_add(prev_balance),
                nonce,
                storage_layout,
            )?;
        } else {
            // In contract creation, the `params.value` should never be
            // `Apparent`.
            unreachable!();
        }

        Ok(())
    }

    /// When the executive (the inner EVM) returns, this function will process
    /// the rest tasks: If the execution successes, this function collects
    /// storage collateral change from the cache to substate, merge substate to
    /// its parent and settles down bytecode for newly created contract. If the
    /// execution fails, this function reverts state and drops substate.
    fn process_return(
        mut self,
        result: vm::Result<GasLeft>,
        state: &mut dyn StateTrait<Substate = Substate>,
        parent_substate: &mut Substate,
        callstack: &mut CallStackInfo,
        tracer: &mut dyn VmObserve,
    ) -> DbResult<vm::Result<ExecutiveResult>> {
        let context = self.context.activate(state, callstack);
        // The post execution task in spec is completed here.
        let finalized_result = result.finalize(context);
        let executive_result =
            finalized_result.map(|result| ExecutiveResult::new(result, self.create_address));

        self.status = ExecutiveStatus::Done;

        let executive_result = vm::separate_out_db_error(executive_result)?;

        if self.context.is_create {
            tracer.record_create_result(&executive_result);
        } else {
            tracer.record_call_result(&executive_result);
        }

        let apply_state = executive_result.as_ref().map_or(false, |r| r.apply_state);
        if apply_state {
            let mut substate = self.context.substate;
            if let Some(create_address) = self.create_address {
                substate
                    .contracts_created_mut()
                    .push(create_address.with_space(self.context.space));
            }

            state.discard_checkpoint();
            // See my comments in resume function.
            parent_substate.accrue(substate);
        } else {
            state.revert_to_checkpoint();
        }
        callstack.pop();

        Ok(executive_result)
    }

    /// If the executive triggers a sub-call during execution, this function
    /// outputs a trap error with sub-call parameters and return point.
    fn process_trap(mut self, trap_err: ExecTrapError) -> ExecutiveTrapError<'a, Substate> {
        match trap_err {
            TrapError::Call(subparams, resume) => {
                self.status = ExecutiveStatus::ResumeCall(resume);
                TrapError::Call(subparams, self)
            }
            TrapError::Create(subparams, resume) => {
                self.status = ExecutiveStatus::ResumeCreate(resume);
                TrapError::Create(subparams, self)
            }
        }
    }

    /// Execute the executive. If a sub-call/create action is required, a
    /// resume trap error is returned. The caller is then expected to call
    /// `resume` to continue the execution.
    pub fn exec(
        mut self,
        state: &mut dyn StateTrait<Substate = Substate>,
        parent_substate: &mut Substate,
        callstack: &mut CallStackInfo,
        tracer: &mut dyn VmObserve,
    ) -> DbResult<ExecutiveTrapResult<'a, ExecutiveResult, Substate>> {
        let status = std::mem::replace(&mut self.status, ExecutiveStatus::Running);
        let params = if let ExecutiveStatus::Input(params) = status {
            params
        } else {
            panic!("Status should be input parameter")
        };

        let is_create = self.create_address.is_some();
        assert_eq!(is_create, self.context.is_create);

        // By technical specification and current implementation, the EVM should
        // guarantee the current executive satisfies static_flag.
        Self::check_static_flag(&params, self.context.static_flag, is_create)
            .expect("check_static_flag should always success because EVM has checked it.");

        // Trace task
        if is_create {
            debug!(
                "CallCreateExecutiveKind::ExecCreate: contract_addr = {:?}",
                params.address
            );
            tracer.record_create(&params);
        } else {
            tracer.record_call(&params);
        }

        // Make checkpoint for this executive, callstack is always maintained
        // with checkpoint.
        state.checkpoint();

        let contract_address = self.get_recipient().clone();
        callstack.push(contract_address.with_space(self.context.space), is_create);

        // Pre execution: transfer value and init contract.
        let spec = self.context.spec;
        if is_create {
            Self::transfer_exec_balance_and_init_contract(
                &params,
                spec,
                state.as_mut_state_ops(),
                // It is a bug in the Parity version.
                &mut self.context.substate,
                Some(STORAGE_LAYOUT_REGULAR_V0),
            )?
        } else {
            Self::transfer_exec_balance(
                &params,
                spec,
                state.as_mut_state_ops(),
                &mut self.context.substate,
                spec.account_start_nonce,
            )?
        };

        // Fetch execution model and execute
        let exec: Box<dyn Exec> = match self.kind {
            CallCreateExecutiveKind::Transfer => Box::new(NoopExec { gas: params.gas }),
            CallCreateExecutiveKind::CallBuiltin(builtin) => {
                Box::new(BuiltinExec { builtin, params })
            }
            CallCreateExecutiveKind::CallInternalContract(internal) => {
                Box::new(InternalContractExec { internal, params })
            }
            CallCreateExecutiveKind::ExecCall | CallCreateExecutiveKind::ExecCreate => {
                let factory = self.context.machine.vm_factory();
                factory.create(params, self.context.spec, self.context.depth)
            }
        };
        let mut context = self.context.activate(state, callstack);
        let output = exec.exec(&mut context, tracer);

        // Post execution.
        self.process_output(output, state, parent_substate, callstack, tracer)
    }

    pub fn resume(
        mut self,
        result: vm::Result<ExecutiveResult>,
        state: &mut dyn StateTrait<Substate = Substate>,
        parent_substate: &mut Substate,
        callstack: &mut CallStackInfo,
        tracer: &mut dyn VmObserve,
    ) -> DbResult<ExecutiveTrapResult<'a, ExecutiveResult, Substate>> {
        let status = std::mem::replace(&mut self.status, ExecutiveStatus::Running);

        // TODO: Substate from sub-call should have been merged here by
        // specification. But we have merged it in function `process_return`.
        // If we put `substate.accrue` back to here, we can save the maintenance
        // for `parent_substate` in `exec`, `resume`, `process_return` and
        // `consume`. It will also make the implementation with
        // specification: substate is in return value and its caller's duty to
        // merge callee's substate. However, Substate is a trait
        // currently, such change will make too many functions has generic
        // parameters or trait parameter. So I put off this plan until
        // substate is no longer a trait.

        // Process resume tasks, which is defined in Instruction Set
        // Specification of tech-specification.
        let exec = match status {
            ExecutiveStatus::ResumeCreate(resume) => {
                let result = into_contract_create_result(result);
                resume.resume_create(result)
            }
            ExecutiveStatus::ResumeCall(resume) => {
                let result = into_message_call_result(result);
                resume.resume_call(result)
            }
            ExecutiveStatus::Input(_) | ExecutiveStatus::Done | ExecutiveStatus::Running => {
                panic!("Incorrect executive status in resume");
            }
        };

        let mut context = self.context.activate(state, callstack);
        let output = exec.exec(&mut context, tracer);

        // Post execution.
        self.process_output(output, state, parent_substate, callstack, tracer)
    }

    #[inline]
    fn process_output(
        self,
        output: ExecTrapResult<GasLeft>,
        state: &mut dyn StateTrait<Substate = Substate>,
        parent_substate: &mut Substate,
        callstack: &mut CallStackInfo,
        tracer: &mut dyn VmObserve,
    ) -> DbResult<ExecutiveTrapResult<'a, ExecutiveResult, Substate>> {
        // Convert the `ExecTrapResult` (result of evm) to `ExecutiveTrapResult`
        // (result of self).
        let trap_result = match output {
            TrapResult::Return(result) => TrapResult::Return(self.process_return(
                result,
                state,
                parent_substate,
                callstack,
                tracer,
            )?),
            TrapResult::SubCallCreate(trap_err) => {
                TrapResult::SubCallCreate(self.process_trap(trap_err))
            }
        };
        Ok(trap_result)
    }

    /// Execute the top call-create executive. This function handles resume
    /// traps and sub-level tracing. The caller is expected to handle
    /// current-level tracing.
    pub fn consume(
        self,
        state: &'a mut dyn StateTrait<Substate = Substate>,
        top_substate: &mut Substate,
        tracer: &mut dyn VmObserve,
    ) -> DbResult<vm::Result<FinalizationResult>> {
        let mut callstack = CallStackInfo::new();
        let mut executive_stack: Vec<Self> = Vec::new();

        let mut last_res = self.exec(state, top_substate, &mut callstack, tracer)?;

        loop {
            match last_res {
                TrapResult::Return(result) => {
                    let parent = match executive_stack.pop() {
                        Some(x) => x,
                        None => {
                            return Ok(result.map(|result| result.into()));
                        }
                    };

                    let parent_substate = executive_stack
                        .last_mut()
                        .map_or(&mut *top_substate, |parent| parent.unconfirmed_substate());

                    last_res =
                        parent.resume(result, state, parent_substate, &mut callstack, tracer)?;
                }
                TrapResult::SubCallCreate(trap_err) => {
                    let (callee, caller) = Self::from_trap_error(trap_err);
                    executive_stack.push(caller);

                    let parent_substate = executive_stack
                        .last_mut()
                        .expect("Last executive is `caller`, it will never be None")
                        .unconfirmed_substate();

                    last_res = callee.exec(state, parent_substate, &mut callstack, tracer)?;
                }
            }
        }
    }

    /// Output callee executive and caller executive from trap kind error.
    pub fn from_trap_error(trap_err: ExecutiveTrapError<'a, Substate>) -> (Self, Self) {
        match trap_err {
            TrapError::Call(params, parent) => (
                /* callee */
                CallCreateExecutive::new_call_raw(
                    params,
                    parent.context.env,
                    parent.context.machine,
                    parent.context.spec,
                    parent.factory,
                    parent.context.depth + 1,
                    parent.context.static_flag,
                ),
                /* caller */ parent,
            ),
            TrapError::Create(params, parent) => (
                /* callee */
                CallCreateExecutive::new_create_raw(
                    params,
                    parent.context.env,
                    parent.context.machine,
                    parent.context.spec,
                    parent.factory,
                    parent.context.depth + 1,
                    parent.context.static_flag,
                ),
                /* callee */ parent,
            ),
        }
    }
}

/// The result contains more data than finalization result.
#[derive(Debug)]
pub struct ExecutiveResult {
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

impl Into<FinalizationResult> for ExecutiveResult {
    fn into(self) -> FinalizationResult {
        FinalizationResult {
            space: self.space,
            gas_left: self.gas_left,
            apply_state: self.apply_state,
            return_data: self.return_data,
        }
    }
}

impl ExecutiveResult {
    fn new(result: FinalizationResult, create_address: Option<Address>) -> Self {
        ExecutiveResult {
            space: result.space,
            gas_left: result.gas_left,
            apply_state: result.apply_state,
            return_data: result.return_data,
            create_address,
        }
    }
}

/// Trap result returned by executive.
pub type ExecutiveTrapResult<'a, T, Substate> =
    vm::TrapResult<T, CallCreateExecutive<'a, Substate>, CallCreateExecutive<'a, Substate>>;

pub type ExecutiveTrapError<'a, Substate> =
    vm::TrapError<CallCreateExecutive<'a, Substate>, CallCreateExecutive<'a, Substate>>;

pub type Executive<'a> = ExecutiveGeneric<'a, Substate>;

/// Transaction executor.
pub struct ExecutiveGeneric<'a, Substate: SubstateTrait> {
    pub state: &'a mut dyn StateTrait<Substate = Substate>,
    env: &'a Env,
    machine: &'a Machine,
    spec: &'a Spec,
    depth: usize,
    static_flag: bool,
}

struct SponsorCheckOutput {
    sender_intended_cost: U512,
    total_cost: U512,
    gas_sponsored: bool,
    storage_sponsored: bool,
    storage_sponsor_eligible: bool,
}

pub fn gas_required_for(is_create: bool, data: &[u8], spec: &Spec) -> u64 {
    data.iter().fold(
        (if is_create {
            spec.tx_create_gas
        } else {
            spec.tx_gas
        }) as u64,
        |g, b| {
            g + (match *b {
                0 => spec.tx_data_zero_gas,
                _ => spec.tx_data_non_zero_gas,
            }) as u64
        },
    )
}

impl<'a, Substate: SubstateMngTrait> ExecutiveGeneric<'a, Substate> {
    /// Basic constructor.
    pub fn new(
        state: &'a mut dyn StateTrait<Substate = Substate>,
        env: &'a Env,
        machine: &'a Machine,
        spec: &'a Spec,
    ) -> Self {
        ExecutiveGeneric {
            state,
            env,
            machine,
            spec,
            depth: 0,
            static_flag: false,
        }
    }

    pub fn create(
        &mut self,
        params: ActionParams,
        substate: &mut Substate,
        tracer: &mut dyn VmObserve,
    ) -> DbResult<vm::Result<FinalizationResult>> {
        let vm_factory = self.machine.vm_factory();
        let result = CallCreateExecutive::new_create_raw(
            params,
            self.env,
            self.machine,
            self.spec,
            &vm_factory,
            self.depth,
            self.static_flag,
        )
        .consume(self.state, substate, tracer)?;

        Ok(result)
    }

    pub fn call(
        &mut self,
        params: ActionParams,
        substate: &mut Substate,
        tracer: &mut dyn VmObserve,
    ) -> DbResult<vm::Result<FinalizationResult>> {
        let vm_factory = self.machine.vm_factory();
        let result = CallCreateExecutive::new_call_raw(
            params,
            self.env,
            self.machine,
            self.spec,
            &vm_factory,
            self.depth,
            self.static_flag,
        )
        .consume(self.state, substate, tracer)?;

        Ok(result)
    }

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
            let gas_fee = if request.recheck_gas_fee() && !executed.gas_sponsor_paid {
                executed
                    .estimated_gas_limit
                    .unwrap()
                    .saturating_mul(*tx.gas_price())
            } else {
                0.into()
            };
            let storage_collateral = if !executed.storage_sponsor_paid {
                U256::from(executed.estimated_storage_limit) * *DRIPS_PER_STORAGE_COLLATERAL_UNIT
            } else {
                0.into()
            };
            let value_and_fee = tx
                .value()
                .saturating_add(gas_fee)
                .saturating_add(storage_collateral);
            if balance < value_and_fee {
                return Ok(ExecutionOutcome::ExecutionErrorBumpNonce(
                    ExecutionError::NotEnoughCash {
                        required: value_and_fee.into(),
                        got: balance.into(),
                        actual_gas_cost: min(balance, gas_fee),
                        max_storage_limit_cost: storage_collateral,
                    },
                    executed,
                ));
            }
        }

        assert!(!request.has_storage_limit);

        return Ok(ExecutionOutcome::Finished(executed));
    }
    pub fn transact(
        &mut self,
        tx: &SignedTransaction,
        options: TransactOptions,
    ) -> DbResult<ExecutionOutcome> {
        let TransactOptions {
            mut observer,
            check_settings,
        } = options;

        let spec = &self.spec;
        let sender = tx.sender();
        let nonce = self.state.nonce(&sender)?;

        // Validate transaction nonce
        if *tx.nonce() < nonce {
            return Ok(ExecutionOutcome::NotExecutedDrop(TxDropError::OldNonce(
                nonce,
                *tx.nonce(),
            )));
        } else if *tx.nonce() > nonce {
            return Ok(ExecutionOutcome::NotExecutedToReconsiderPacking(
                ToRepackError::InvalidNonce {
                    expected: nonce,
                    got: *tx.nonce(),
                },
            ));
        }

        let base_gas_required = gas_required_for(tx.action() == &Action::Create, &tx.data(), spec);
        assert!(
            *tx.gas() >= base_gas_required.into(),
            "We have already checked the base gas requirement when we received the block."
        );

        let balance = self.state.balance(&sender)?;
        let gas_cost = if check_settings.charge_gas {
            tx.gas().full_mul(*tx.gas_price())
        } else {
            0.into()
        };
        let storage_cost = U256::zero();

        let sender_balance = U512::from(balance);

        let SponsorCheckOutput {
            sender_intended_cost,
            total_cost,
            gas_sponsored,
            storage_sponsored,
            storage_sponsor_eligible,
        } = {
            let sender_cost = U512::from(tx.value()) + gas_cost;
            SponsorCheckOutput {
                sender_intended_cost: sender_cost,
                total_cost: sender_cost,
                gas_sponsored: false,
                storage_sponsored: false,
                storage_sponsor_eligible: false,
            }
        };

        let mut tx_substate = Substate::new();
        if sender_balance < sender_intended_cost {
            // Sender is responsible for the insufficient balance.
            // Sub tx fee if not enough cash, and substitute all remaining
            // balance if balance is not enough to pay the tx fee
            let actual_gas_cost: U256 = U512::min(gas_cost, sender_balance).try_into().unwrap();

            // We don't want to bump nonce for non-existent account when we
            // can't charge gas fee. In this case, the sender account will
            // not be created if it does not exist.
            if !self.state.exists(&sender)? && check_settings.real_execution {
                return Ok(ExecutionOutcome::NotExecutedToReconsiderPacking(
                    ToRepackError::SenderDoesNotExist,
                ));
            }
            self.state
                .inc_nonce(&sender, &self.spec.account_start_nonce)?;
            self.state.sub_balance(
                &sender,
                &actual_gas_cost,
                &mut cleanup_mode(&mut tx_substate, &spec),
            )?;
            observer.as_state_tracer().trace_internal_transfer(
                AddressPocket::Balance(sender.address.with_space(tx.space())),
                AddressPocket::GasPayment,
                actual_gas_cost,
            );

            return Ok(ExecutionOutcome::ExecutionErrorBumpNonce(
                ExecutionError::NotEnoughCash {
                    required: total_cost,
                    got: sender_balance,
                    actual_gas_cost: actual_gas_cost.clone(),
                    max_storage_limit_cost: storage_cost,
                },
                Executed::not_enough_balance_fee_charged(
                    tx,
                    &actual_gas_cost,
                    gas_sponsored,
                    storage_sponsored,
                    observer.tracer.map_or(Default::default(), |t| t.drain()),
                    &self.spec,
                ),
            ));
        } else {
            // From now on sender balance >= total_cost, even if the sender
            // account does not exist (since she may be sponsored). Transaction
            // execution is guaranteed. Note that inc_nonce() will create a
            // new account if the account does not exist.
            self.state
                .inc_nonce(&sender, &self.spec.account_start_nonce)?;
        }

        // Subtract the transaction fee from sender or contract.
        let gas_cost = U256::try_from(gas_cost).unwrap();

        {
            observer.as_state_tracer().trace_internal_transfer(
                AddressPocket::Balance(sender.address.with_space(tx.space())),
                AddressPocket::GasPayment,
                gas_cost,
            );
            self.state.sub_balance(
                &sender,
                &U256::try_from(gas_cost).unwrap(),
                &mut cleanup_mode(&mut tx_substate, &spec),
            )?;
        }

        let init_gas = tx.gas() - base_gas_required;

        // Initialize the checkpoint for transaction execution. This checkpoint
        // can be reverted by "deploying contract on conflict address" or "not
        // enough balance for storage".
        self.state.checkpoint();
        observer.as_state_tracer().checkpoint();
        let mut substate = Substate::new();

        let res = match tx.action() {
            Action::Create => {
                let address_scheme = match tx.space() {
                    Space::Ethereum => CreateContractAddress::FromSenderNonce,
                };
                let (new_address, _code_hash) = contract_address(
                    address_scheme,
                    self.env.number.into(),
                    &sender,
                    &nonce,
                    &tx.data(),
                );

                let params = ActionParams {
                    space: sender.space,
                    code_address: new_address.address,
                    code_hash: None,
                    address: new_address.address,
                    sender: sender.address,
                    original_sender: sender.address,
                    gas: init_gas,
                    gas_price: *tx.gas_price(),
                    value: ActionValue::Transfer(*tx.value()),
                    code: Some(Arc::new(tx.data().clone())),
                    data: None,
                    call_type: CallType::None,
                    create_type: CreateType::CREATE,
                    params_type: vm::ParamsType::Embedded,
                };
                self.create(params, &mut substate, &mut *observer.as_vm_observe())?
            }
            Action::Call(ref address) => {
                let address = address.with_space(sender.space);
                let params = ActionParams {
                    space: sender.space,
                    code_address: address.address,
                    address: address.address,
                    sender: sender.address,
                    original_sender: sender.address,
                    gas: init_gas,
                    gas_price: *tx.gas_price(),
                    value: ActionValue::Transfer(*tx.value()),
                    code: self.state.code(&address)?,
                    code_hash: self.state.code_hash(&address)?,
                    data: Some(tx.data().clone()),
                    call_type: CallType::Call,
                    create_type: CreateType::None,
                    params_type: vm::ParamsType::Separate,
                };
                self.call(params, &mut substate, &mut *observer.as_vm_observe())?
            }
        };

        // Charge collateral and process the checkpoint.
        let (result, output) = {
            let res = res.and_then(|finalize_res| Ok(finalize_res));
            let out = match &res {
                Ok(res) => {
                    observer.as_state_tracer().discard_checkpoint();
                    self.state.discard_checkpoint();
                    tx_substate.accrue(substate);
                    res.return_data.to_vec()
                }
                Err(vm::Error::StateDbError(_)) => {
                    // The whole epoch execution fails. No need to revert state.
                    Vec::new()
                }
                Err(_) => {
                    observer.as_state_tracer().revert_to_checkpoint();
                    self.state.revert_to_checkpoint();
                    Vec::new()
                }
            };
            (res, out)
        };

        let estimated_gas_limit = observer
            .gas_man
            .as_ref()
            .map(|g| g.gas_required() * 7 / 6 + base_gas_required);

        Ok(self.finalize(
            tx,
            tx_substate,
            result,
            output,
            /* Storage sponsor paid */
            if self.spec.cip78a {
                storage_sponsored
            } else {
                storage_sponsor_eligible
            },
            observer,
            estimated_gas_limit,
        )?)
    }

    // TODO: maybe we can find a better interface for doing the suicide
    // post-processing.
    fn kill_process(
        &mut self,
        suicides: &HashSet<AddressWithSpace>,
        tracer: &mut dyn StateTracer,
    ) -> DbResult<Substate> {
        let substate = Substate::new();

        for contract_address in suicides {
            let contract_balance = self.state.balance(contract_address)?;
            tracer.trace_internal_transfer(
                AddressPocket::Balance(*contract_address),
                AddressPocket::MintBurn,
                contract_balance.clone(),
            );

            self.state.remove_contract(contract_address)?;
            self.state.subtract_total_issued(contract_balance);
        }

        Ok(substate)
    }

    /// Finalizes the transaction (does refunds and suicides).
    fn finalize(
        &mut self,
        tx: &SignedTransaction,
        mut substate: Substate,
        result: vm::Result<FinalizationResult>,
        output: Bytes,
        storage_sponsor_paid: bool,
        mut observer: Observer,
        estimated_gas_limit: Option<U256>,
    ) -> DbResult<ExecutionOutcome> {
        let gas_left = match result {
            Ok(FinalizationResult { gas_left, .. }) => gas_left,
            _ => 0.into(),
        };

        // gas_used is only used to estimate gas needed
        let gas_used = tx.gas() - gas_left;
        // gas_left should be smaller than 1/4 of gas_limit, otherwise
        // 3/4 of gas_limit is charged.
        let charge_all = (gas_left + gas_left + gas_left) >= gas_used;
        let (gas_charged, fees_value, refund_value) = if charge_all {
            let gas_refunded = tx.gas() >> 2;
            let gas_charged = tx.gas() - gas_refunded;
            (
                gas_charged,
                gas_charged.saturating_mul(*tx.gas_price()),
                gas_refunded.saturating_mul(*tx.gas_price()),
            )
        } else {
            (
                gas_used,
                gas_used.saturating_mul(*tx.gas_price()),
                gas_left.saturating_mul(*tx.gas_price()),
            )
        };

        {
            observer.as_state_tracer().trace_internal_transfer(
                AddressPocket::GasPayment,
                AddressPocket::Balance(tx.sender()),
                refund_value.clone(),
            );
            self.state.add_balance(
                &tx.sender(),
                &refund_value,
                cleanup_mode(&mut substate, self.spec),
                self.spec.account_start_nonce,
            )?;
        };

        // perform suicides

        let subsubstate = self.kill_process(&substate.suicides(), observer.as_state_tracer())?;
        substate.accrue(subsubstate);

        // TODO should be added back after enabling dust collection
        // Should be executed once per block, instead of per transaction?
        //
        // When enabling this feature, remember to check touched set in
        // functions like "add_collateral_for_storage()" in "State"
        // struct.

        //        // perform garbage-collection
        //        let min_balance = if spec.kill_dust != CleanDustMode::Off {
        //            Some(U256::from(spec.tx_gas) * tx.gas_price())
        //        } else {
        //            None
        //        };
        //
        //        self.state.kill_garbage(
        //            &substate.touched,
        //            spec.kill_empty,
        //            &min_balance,
        //            spec.kill_dust == CleanDustMode::WithCodeAndStorage,
        //        )?;

        match result {
            Err(vm::Error::StateDbError(e)) => bail!(e.0),
            Err(exception) => Ok(ExecutionOutcome::ExecutionErrorBumpNonce(
                ExecutionError::VmError(exception),
                Executed::execution_error_fully_charged(
                    tx,
                    false,
                    storage_sponsor_paid,
                    observer.tracer.map_or(Default::default(), |t| t.drain()),
                    &self.spec,
                ),
            )),
            Ok(r) => {
                let mut storage_collateralized = Vec::new();
                let mut storage_released = Vec::new();

                if r.apply_state {
                    let mut affected_address: Vec<_> = substate
                        .keys_for_collateral_changed()
                        .iter()
                        .cloned()
                        .collect();
                    affected_address.sort();
                    for address in affected_address {
                        let (inc, sub) = substate.get_collateral_change(&address);
                        if inc > 0 {
                            storage_collateralized.push(StorageChange {
                                address: *address,
                                collaterals: inc.into(),
                            });
                        } else if sub > 0 {
                            storage_released.push(StorageChange {
                                address: *address,
                                collaterals: sub.into(),
                            });
                        }
                    }
                }

                let trace = observer.tracer.map_or(Default::default(), |t| t.drain());

                let estimated_storage_limit = if let Some(x) = storage_collateralized.first() {
                    x.collaterals.as_u64()
                } else {
                    0
                };

                let executed = Executed {
                    gas_used,
                    gas_charged,
                    fee: fees_value,
                    gas_sponsor_paid: false,
                    logs: substate.logs().to_vec(),
                    contracts_created: substate.contracts_created().to_vec(),
                    storage_sponsor_paid,
                    storage_collateralized,
                    storage_released,
                    output,
                    trace,
                    estimated_gas_limit,
                    estimated_storage_limit,
                };

                if r.apply_state {
                    Ok(ExecutionOutcome::Finished(executed))
                } else {
                    // Transaction reverted by vm instruction.
                    Ok(ExecutionOutcome::ExecutionErrorBumpNonce(
                        ExecutionError::VmError(vm::Error::Reverted),
                        executed,
                    ))
                }
            }
        }
    }
}
