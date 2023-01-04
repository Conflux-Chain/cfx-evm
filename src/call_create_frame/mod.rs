mod context;
mod exec;
mod frame;
mod result;

#[cfg(test)]
mod tests;

pub use frame::{contract_address, start_exec_frames, CallCreateFrame};
pub use result::FrameResult;
