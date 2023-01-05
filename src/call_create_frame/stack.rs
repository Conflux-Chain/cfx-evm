use super::{
    frame::{CallCreateFrame, FrameTrapResult},
    result::accrue_substate,
    FrameReturn,
};

use crate::{
    evm::FinalizationResult,
    observer::MultiObservers as Observer,
    state::{FrameStackInfo, Substate},
    vm::{self, TrapResult},
};
use cfx_state::StateTrait;
use cfx_statedb::Result as DbResult;
use cfx_types::Space;

pub struct FrameStack<'a> {
    state: &'a mut dyn StateTrait,
    frame_stack: Vec<CallCreateFrame<'a>>,
    callstack: FrameStackInfo,
    tx_substate: Substate,
    observer: Observer,
    base_gas_required: u64,
}

pub struct FrameStackOutput {
    pub result: vm::Result<FinalizationResult>,
    pub substate: Substate,
    pub observer: Observer,
    pub base_gas_required: u64,
}

pub struct CrossVmResult;
#[allow(unreachable_code)]
impl From<CrossVmResult> for vm::Result<FrameReturn> {
    fn from(_: CrossVmResult) -> Self {
        Ok(FrameReturn {
            space: Space::Ethereum,
            gas_left: todo!(),
            apply_state: todo!(),
            return_data: todo!(),
            create_address: None,
            substate: None,
        })
    }
}

impl<'a> FrameStack<'a> {
    pub fn new(
        state: &'a mut dyn StateTrait,
        top_substate: Substate,
        observer: Observer,
        base_gas_required: u64,
    ) -> Self {
        FrameStack {
            state,
            frame_stack: vec![],
            callstack: FrameStackInfo::new(),
            tx_substate: top_substate,
            observer,
            base_gas_required,
        }
    }

    /// Execute the top call-create executive. This function handles resume
    /// traps and sub-level tracing. The caller is expected to handle
    /// current-level tracing.
    pub fn exec(mut self, top_frame: CallCreateFrame<'a>) -> DbResult<FrameStackOutput> {
        let last_res = top_frame.exec(
            self.state,
            &mut self.callstack,
            &mut *self.observer.as_vm_observe(),
        )?;
        self.exec_stack(last_res)
    }

    #[allow(unused)]
    pub fn resume(mut self, cross_vm_result: CrossVmResult) -> DbResult<FrameStackOutput> {
        let first_frame = self.frame_stack.pop().expect("Cannot resume");

        let parent_substate = self
            .frame_stack
            .last_mut()
            .map_or(&mut self.tx_substate, |parent| {
                parent.unconfirmed_substate()
            });
        let last_res = first_frame.resume(
            cross_vm_result.into(),
            self.state,
            &mut self.callstack,
            &mut *self.observer.as_vm_observe(),
        )?;
        self.exec_stack(last_res)
    }

    fn exec_stack(mut self, mut last_res: FrameTrapResult<'a>) -> DbResult<FrameStackOutput> {
        loop {
            last_res = match last_res {
                TrapResult::Return(result) => {
                    let parent = match self.frame_stack.pop() {
                        Some(x) => x,
                        None => {
                            return Ok(self.process_return(result));
                        }
                    };

                    parent.resume(
                        result,
                        self.state,
                        &mut self.callstack,
                        &mut *self.observer.as_vm_observe(),
                    )?
                }
                TrapResult::SubCallCreate(trap_err) => {
                    let (callee, caller) = CallCreateFrame::from_trap_error(trap_err);
                    self.frame_stack.push(caller);

                    callee.exec(
                        self.state,
                        &mut self.callstack,
                        &mut *self.observer.as_vm_observe(),
                    )?
                }
            }
        }
    }

    fn process_return(mut self, mut result: vm::Result<FrameReturn>) -> FrameStackOutput {
        accrue_substate(&mut self.tx_substate, &mut result);
        return FrameStackOutput {
            result: result.map(|result| result.into()),
            substate: self.tx_substate,
            observer: self.observer,
            base_gas_required: self.base_gas_required,
        };
    }
}
