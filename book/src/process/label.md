# Label

A type-safe label for grouping and identifying processes.

## Overview

`Label` is a newtype wrapping `String`, following the same pattern as `ProcessId`. It provides type safety for label values used in grouped operations and dependency references.

Labels are **not** unique identifiers — multiple processes can share the same label. Use `ProcessId` for individual process operations and `Label` for grouped operations.

## Construction

```rust
use process_manager::Label;

let label = Label::new("compositor");
let label2: Label = "panel".into();
```

## Trait Implementations

- `Display` - formats as the inner string
- `PartialEq`, `Eq`, `Hash` - comparison and hashing (usable as `HashMap` key)
- `Clone` - lightweight clone (not `Copy` because it wraps `String`)
- `AsRef<str>` - interoperability with `&str` APIs
- `From<&str>`, `From<String>`, `From<&String>` - convenient construction
- `Serialize`, `Deserialize` - serde support via `#[serde(transparent)]` (serializes as the inner string across all formats, not just JSON)

## Usage

```rust
use process_manager::{Label, ProcessConfig, ProcessManager, StdioConfig};

let manager = ProcessManager::new();
let config = ProcessConfig::builder()
    .command("worker".to_string())
    .stdout(StdioConfig::Null)
    .build();

// Start processes with a label — &str is accepted via Into<Label>
let id = manager.start("worker-pool", &config)?;

// Group operations by label
manager.stop_label("worker-pool")?;

// Or construct a Label explicitly
let label = Label::new("worker-pool");
manager.start(label, &config)?;
```

## DependencyRef

`Label` is used in `DependencyRef::Label` for declaring dependencies by name:

```rust
use process_manager::{DependencyRef, Label};

let dep = DependencyRef::label("compositor");
// or equivalently:
let dep = DependencyRef::Label(Label::new("compositor"));
```
