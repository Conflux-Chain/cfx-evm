use super::{gasman::GasMan, tracer::ExecutiveTracer, StateTracer, VmObserve};

pub struct MultiObservers {
    pub tracer: Option<ExecutiveTracer>,
    pub gas_man: Option<GasMan>,
    _noop: (),
}

impl MultiObservers {
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

    pub fn with_tracing() -> Self {
        MultiObservers {
            tracer: Some(ExecutiveTracer::default()),
            gas_man: None,
            _noop: (),
        }
    }

    pub fn with_no_tracing() -> Self {
        MultiObservers {
            tracer: None,
            gas_man: None,
            _noop: (),
        }
    }

    pub fn virtual_call() -> Self {
        MultiObservers {
            tracer: Some(ExecutiveTracer::default()),
            gas_man: Some(GasMan::default()),
            _noop: (),
        }
    }
}
