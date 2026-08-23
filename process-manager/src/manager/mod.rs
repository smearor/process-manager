mod dependency;
mod error;
mod process;
mod spawn_result;

pub use error::ProcessManagerError;
pub use error::StopManyError;
pub use process::ProcessManager;

pub(crate) use dependency::all_deps_running;
pub(crate) use dependency::build_dependency_snapshot;
pub(crate) use dependency::detect_cycle;
pub(crate) use dependency::resolve_dependencies;
pub(crate) use spawn_result::SpawnResult;
