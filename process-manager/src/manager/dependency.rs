use crate::config::DependencyRef;
use crate::manager::ProcessManagerError;
use crate::process::ProcessId;
use crate::process::ProcessState;
use dashmap::DashMap;
use std::collections::HashMap;
use std::collections::HashSet;

/// Snapshot of the dependency graph for cycle detection.
///
/// Maps label -> list of `DependencyRef` for each known process.
/// Also maps `ProcessId` -> label for resolving `DependencyRef::Id`.
type DepGraph = HashMap<String, Vec<DependencyRef>>;

/// Build a snapshot of the current dependency graph from the `DashMap`.
///
/// Acquires a short-lived read lock via `iter()`, clones each process's
/// `label` and `depends_on` into a local `HashMap`, and releases the lock
/// immediately. The new process's own `label` and `depends_on` are added
/// to the snapshot.
///
/// This snapshot approach prevents deadlocks that could occur if we
/// traversed the `DashMap` while holding read locks on shards.
pub(crate) fn build_dependency_snapshot(
    processes: &DashMap<ProcessId, crate::process::Process>,
    new_label: &str,
    new_depends_on: &[DependencyRef],
) -> DepGraph {
    let mut graph: DepGraph = HashMap::new();
    for entry in processes.iter() {
        let process = entry.value();
        graph.insert(process.label.clone(), process.config.depends_on.clone());
    }
    graph.insert(new_label.to_string(), new_depends_on.to_vec());
    graph
}

/// Detect cycles in the dependency graph using DFS.
///
/// Returns `Ok(())` if no cycle is found, or `Err(DependencyCycle)` with
/// the cycle path.
pub(crate) fn detect_cycle(graph: &DepGraph, start_label: &str, start_depends_on: &[DependencyRef]) -> Result<(), ProcessManagerError> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut path: Vec<DependencyRef> = Vec::new();
    let mut path_labels: HashSet<String> = HashSet::new();

    dfs_cycle(graph, start_label, start_depends_on, &mut visited, &mut path, &mut path_labels)
}

/// Recursive DFS helper for cycle detection.
fn dfs_cycle(
    graph: &DepGraph,
    current_label: &str,
    depends_on: &[DependencyRef],
    visited: &mut HashSet<String>,
    path: &mut Vec<DependencyRef>,
    path_labels: &mut HashSet<String>,
) -> Result<(), ProcessManagerError> {
    path_labels.insert(current_label.to_string());

    for dep in depends_on {
        match dep {
            DependencyRef::Label(target_label) => {
                // Check if this label is already on the current path (cycle)
                if path_labels.contains(target_label) {
                    path.push(dep.clone());
                    return Err(ProcessManagerError::DependencyCycle { cycle: path.clone() });
                }

                // Skip if already fully visited (no cycle through this node)
                if visited.contains(target_label) {
                    continue;
                }

                path.push(dep.clone());

                // Get the dependencies of the target label from the graph
                if let Some(target_deps) = graph.get(target_label) {
                    dfs_cycle(graph, target_label, target_deps, visited, path, path_labels)?;
                }

                path.pop();
            }
            DependencyRef::Id(_) => {
                // `Id` dependencies don't create label-based cycles in the graph
                // snapshot. They reference a specific `ProcessId` which may or
                // may not exist. We skip them in cycle detection since they
                // can't form a cycle through label resolution.
            }
        }
    }

    path_labels.remove(current_label);
    visited.insert(current_label.to_string());
    Ok(())
}

/// Resolve `DependencyRef::Label` to a concrete `ProcessId` by finding the
/// first process with that label in `Running` or `Starting` state.
///
/// `Starting` is accepted because the process is alive and will soon
/// transition to `Running`. The caller (`all_deps_running`) will call
/// `state()` to perform the `Starting → Running` transition and verify.
///
/// Returns `Ok(resolved_ids)` if all dependencies resolve, or
/// `Err(DependencyNotFound)` if a label dependency cannot be resolved.
pub(crate) fn resolve_dependencies(
    processes: &DashMap<ProcessId, crate::process::Process>,
    depends_on: &[DependencyRef],
) -> Result<Vec<ProcessId>, ProcessManagerError> {
    let mut resolved: Vec<ProcessId> = Vec::new();
    for dep in depends_on {
        match dep {
            DependencyRef::Label(label) => {
                let found = processes
                    .iter()
                    .find(|entry| {
                        entry.value().label == *label && (entry.value().state == ProcessState::Running || entry.value().state == ProcessState::Starting)
                    })
                    .map(|entry| *entry.key());
                match found {
                    Some(id) => resolved.push(id),
                    None => return Err(ProcessManagerError::DependencyNotFound { dependency: dep.clone() }),
                }
            }
            DependencyRef::Id(id) => {
                resolved.push(*id);
            }
        }
    }
    Ok(resolved)
}

/// Check whether all resolved dependency `ProcessId`s are in `Running` state.
/// Calls `state()` on each non-Waiting dependency to perform a `try_wait()`
/// poll, transitioning `Starting` → `Running` if the child process is alive.
/// Waiting processes are checked by their stored state only, since calling
/// `state()` on a process with no child would incorrectly transition it to
/// `Stopped`.
pub(crate) fn all_deps_running(processes: &DashMap<ProcessId, crate::process::Process>, resolved_deps: &[ProcessId]) -> bool {
    resolved_deps.iter().all(|dep_id| {
        processes.get_mut(dep_id).is_some_and(|mut entry| {
            let process = entry.value_mut();
            if process.state == ProcessState::Waiting {
                false
            } else {
                process.state() == ProcessState::Running
            }
        })
    })
}

/// Check whether any resolved dependency is in a terminal state.
///
/// Terminal states are: `Failed` (rate-limited, spawn failure, or force-kill
/// failure) and `Stopped` without `restart_on_exit` (clean exit, no restart).
/// A dependency in `Crashed` or `Restarting` state is **not** terminal - it
/// may recover via automatic restart.
#[allow(dead_code)]
fn any_dep_terminal(processes: &DashMap<ProcessId, crate::process::Process>, resolved_deps: &[ProcessId]) -> Option<ProcessId> {
    for dep_id in resolved_deps {
        if let Some(entry) = processes.get(dep_id) {
            let process = entry.value();
            if process.state == ProcessState::Failed {
                return Some(*dep_id);
            }
            if process.state == ProcessState::Stopped && !process.config.restart_on_exit {
                return Some(*dep_id);
            }
        } else {
            // Dependency was removed from the manager - treat as terminal
            return Some(*dep_id);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_cycle_no_cycle() {
        let mut graph: DepGraph = HashMap::new();
        graph.insert("a".to_string(), vec![DependencyRef::Label("b".to_string())]);
        graph.insert("b".to_string(), vec![]);

        let result = detect_cycle(&graph, "a", &[DependencyRef::Label("b".to_string())]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_detect_cycle_direct() {
        let mut graph: DepGraph = HashMap::new();
        graph.insert("a".to_string(), vec![DependencyRef::Label("b".to_string())]);
        graph.insert("b".to_string(), vec![DependencyRef::Label("a".to_string())]);

        let result = detect_cycle(&graph, "a", &[DependencyRef::Label("b".to_string())]);
        assert!(matches!(result, Err(ProcessManagerError::DependencyCycle { .. })));
    }

    #[test]
    fn test_detect_cycle_indirect() {
        let mut graph: DepGraph = HashMap::new();
        graph.insert("a".to_string(), vec![DependencyRef::Label("b".to_string())]);
        graph.insert("b".to_string(), vec![DependencyRef::Label("c".to_string())]);
        graph.insert("c".to_string(), vec![DependencyRef::Label("a".to_string())]);

        let result = detect_cycle(&graph, "a", &[DependencyRef::Label("b".to_string())]);
        assert!(matches!(result, Err(ProcessManagerError::DependencyCycle { .. })));
    }

    #[test]
    fn test_detect_cycle_self_dependency() {
        let mut graph: DepGraph = HashMap::new();
        graph.insert("a".to_string(), vec![DependencyRef::Label("a".to_string())]);

        let result = detect_cycle(&graph, "a", &[DependencyRef::Label("a".to_string())]);
        assert!(matches!(result, Err(ProcessManagerError::DependencyCycle { .. })));
    }

    #[test]
    fn test_detect_cycle_diamond_no_false_positive() {
        let mut graph: DepGraph = HashMap::new();
        graph.insert("a".to_string(), vec![DependencyRef::Label("b".to_string()), DependencyRef::Label("c".to_string())]);
        graph.insert("b".to_string(), vec![DependencyRef::Label("d".to_string())]);
        graph.insert("c".to_string(), vec![DependencyRef::Label("d".to_string())]);
        graph.insert("d".to_string(), vec![]);

        let result = detect_cycle(&graph, "a", &[DependencyRef::Label("b".to_string()), DependencyRef::Label("c".to_string())]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_detect_cycle_id_dependency_ignored() {
        let mut graph: DepGraph = HashMap::new();
        graph.insert("a".to_string(), vec![DependencyRef::Id(ProcessId::new(1))]);

        let result = detect_cycle(&graph, "a", &[DependencyRef::Id(ProcessId::new(1))]);
        assert!(result.is_ok());
    }
}
