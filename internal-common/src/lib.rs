extern crate serde;
extern crate serde_derive;

pub struct StateRootWithAuxInfo;

pub mod chain_id;

pub mod debug {
    use serde_derive::{Serialize, Deserialize};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct ComputeEpochDebugRecord;
}

pub use self::chain_id::{ChainIdParams, ChainIdParamsInner};