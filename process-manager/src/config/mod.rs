mod dependency_ref;
mod error;
mod process;
mod restart_policy;
mod restart_trigger;
mod stdio;
mod supervisor_strategy;

pub use dependency_ref::DependencyRef;
pub use error::ProcessConfigError;
pub use process::ProcessConfig;
pub use restart_policy::BackoffConfig;
pub use restart_policy::RestartPolicy;
pub use restart_trigger::RestartTrigger;
pub use stdio::StdioConfig;
pub use supervisor_strategy::SupervisorStrategy;
