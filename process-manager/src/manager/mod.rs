mod error;
mod process;
mod spawn_result;

pub use error::ProcessManagerError;
pub use error::StopManyError;
pub use process::ProcessManager;

pub(crate) use spawn_result::SpawnResult;
