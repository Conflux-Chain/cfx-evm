use super::estimate::EstimateRequest;
use crate::observer::MultiObservers as Observer;

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
            check_settings: TransactCheckSettings::from_estimate_request(request),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TransactCheckSettings {
    pub charge_gas: bool,
    pub real_execution: bool,
}

impl TransactCheckSettings {
    fn all_checks() -> Self {
        Self {
            charge_gas: true,
            real_execution: true,
        }
    }

    fn from_estimate_request(request: EstimateRequest) -> Self {
        Self {
            charge_gas: request.charge_gas(),
            real_execution: false,
        }
    }
}
