// Copyright 2019 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

use super::{
    context::{FrameContext, OriginInfo},
    executive::{BuiltinExec, InternalContractExec, NoopExec},
    result::{into_contract_create_result, into_message_call_result, FrameReturn},
};

use crate::{
    builtin::Builtin,
    evm::Finalize,
    hash::keccak,
    internal_contract::InternalContractTrait,
    machine::Machine,
    observer::VmObserve,
    state::{cleanup_mode, FrameStackInfo, Substate},
    vm::{
        self, ActionParams, ActionValue, CallType, CreateContractAddress, Env, Exec, ExecTrapError,
        ExecTrapResult, GasLeft, ResumeCall, ResumeCreate, Spec, TrapError, TrapResult,
    },
    vm_factory::VmFactory,
};
use cfx_state::{state_trait::StateOpsTrait, StateTrait};
use cfx_statedb::Result as DbResult;
use cfx_types::{Address, AddressSpaceUtil, AddressWithSpace, Space, H256, U256, U64};
use primitives::{storage::STORAGE_LAYOUT_REGULAR_V0, StorageLayout};
use rlp::RlpStream;

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

enum FrameKind<'a> {
    Transfer,
    CallBuiltin(&'a Builtin),
    CallInternalContract(&'a Box<dyn InternalContractTrait>),
    ExecCall,
    ExecCreate,
}

pub struct CallCreateFrame<'a> {
    context: FrameContext<'a>,
    factory: &'a VmFactory,
    status: FrameStatus,
    create_address: Option<Address>,
    kind: FrameKind<'a>,
}

enum FrameStatus {
    Input(ActionParams),
    Running,
    ResumeCall(Box<dyn ResumeCall>),
    ResumeCreate(Box<dyn ResumeCreate>),
    Done,
}

impl<'a> CallCreateFrame<'a> {
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
            FrameKind::CallBuiltin(builtin)
        } else if let Some(internal) = machine.internal_contracts().contract(&code_address, spec) {
            debug!(
                "CallInternalContract: address={:?} data={:?}",
                code_address, params.data
            );
            FrameKind::CallInternalContract(internal)
        } else {
            if params.code.is_some() {
                trace!("ExecCall");
                FrameKind::ExecCall
            } else {
                trace!("Transfer");
                FrameKind::Transfer
            }
        };
        let context = FrameContext::new(
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
            status: FrameStatus::Input(params),
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

        let kind = FrameKind::ExecCreate;

        let substate = Substate::new();

        let context = FrameContext::new(
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
            status: FrameStatus::Input(params),
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
        substate: &mut Substate,
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
        substate: &mut Substate,
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
        state: &mut dyn StateTrait,
        parent_substate: &mut Substate,
        callstack: &mut FrameStackInfo,
        tracer: &mut dyn VmObserve,
    ) -> DbResult<vm::Result<FrameReturn>> {
        let context = self.context.activate(state, callstack);
        // The post execution task in spec is completed here.
        let finalized_result = result.finalize(context);
        let executive_result =
            finalized_result.map(|result| FrameReturn::new(result, self.create_address));

        self.status = FrameStatus::Done;

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
                    .contracts_created
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
    fn process_trap(mut self, trap_err: ExecTrapError) -> FrameTrapError<'a> {
        match trap_err {
            TrapError::Call(subparams, resume) => {
                self.status = FrameStatus::ResumeCall(resume);
                TrapError::Call(subparams, self)
            }
            TrapError::Create(subparams, resume) => {
                self.status = FrameStatus::ResumeCreate(resume);
                TrapError::Create(subparams, self)
            }
        }
    }

    /// Execute the executive. If a sub-call/create action is required, a
    /// resume trap error is returned. The caller is then expected to call
    /// `resume` to continue the execution.
    pub fn exec(
        mut self,
        state: &mut dyn StateTrait,
        parent_substate: &mut Substate,
        callstack: &mut FrameStackInfo,
        tracer: &mut dyn VmObserve,
    ) -> DbResult<FrameTrapResult<'a>> {
        let status = std::mem::replace(&mut self.status, FrameStatus::Running);
        let params = if let FrameStatus::Input(params) = status {
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
            FrameKind::Transfer => Box::new(NoopExec { gas: params.gas }),
            FrameKind::CallBuiltin(builtin) => Box::new(BuiltinExec { builtin, params }),
            FrameKind::CallInternalContract(internal) => {
                Box::new(InternalContractExec { internal, params })
            }
            FrameKind::ExecCall | FrameKind::ExecCreate => {
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
        result: vm::Result<FrameReturn>,
        state: &mut dyn StateTrait,
        parent_substate: &mut Substate,
        callstack: &mut FrameStackInfo,
        tracer: &mut dyn VmObserve,
    ) -> DbResult<FrameTrapResult<'a>> {
        let status = std::mem::replace(&mut self.status, FrameStatus::Running);

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
            FrameStatus::ResumeCreate(resume) => {
                let result = into_contract_create_result(result);
                resume.resume_create(result)
            }
            FrameStatus::ResumeCall(resume) => {
                let result = into_message_call_result(result);
                resume.resume_call(result)
            }
            FrameStatus::Input(_) | FrameStatus::Done | FrameStatus::Running => {
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
        state: &mut dyn StateTrait,
        parent_substate: &mut Substate,
        callstack: &mut FrameStackInfo,
        tracer: &mut dyn VmObserve,
    ) -> DbResult<FrameTrapResult<'a>> {
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

    /// Output callee executive and caller executive from trap kind error.
    pub fn from_trap_error(trap_err: FrameTrapError<'a>) -> (Self, Self) {
        match trap_err {
            TrapError::Call(params, parent) => (
                /* callee */
                CallCreateFrame::new_call_raw(
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
                CallCreateFrame::new_create_raw(
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

/// Trap result returned by executive.
pub type FrameTrapResult<'a> =
    vm::TrapResult<FrameReturn, CallCreateFrame<'a>, CallCreateFrame<'a>>;

pub type FrameTrapError<'a> = vm::TrapError<CallCreateFrame<'a>, CallCreateFrame<'a>>;
