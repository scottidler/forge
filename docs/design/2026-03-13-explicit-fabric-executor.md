# Design Document: Explicit Fabric Executor & Required Tools

**Author:** Scott Idler
**Date:** 2026-03-13
**Status:** Draft
**Review Passes Completed:** 2/5

## Summary

Rename the `pattern` field in forge's pipeline stage definitions to `fabric-pattern`, making the Fabric coupling explicit rather than hiding it behind a generic name. Add gx-style `REQUIRED TOOLS` validation to `forge --help` so users get immediate feedback about whether `fabric` is installed and working.

## Problem Statement

### Background

Forge pipelines define stages that execute via Fabric, a CLI tool that wraps LLM calls in reusable "patterns." The current YAML schema uses `pattern` as the field name:

```yaml
stages:
  research:
    description: "Gather context"
    pattern: extract_article_wisdom
```

The word "pattern" is Fabric's terminology. It does not appear in the MWP paper (by Van Clief & McDermott) that inspired forge's architecture. MWP talks about stages, layers, and reference material -- not patterns.

### Problem

1. **Vocabulary coupling**: `pattern` silently ties forge's data model to Fabric. Anyone reading a pipeline YAML would assume "pattern" is a forge concept, not realizing it's a Fabric-specific term. This makes it harder to reason about adding alternative executors.

2. **No tool validation**: Forge requires `fabric` at runtime but doesn't surface this dependency until execution fails. Users get a cryptic error from `Command::new("fabric")` rather than a clear diagnostic. The gx CLI already solves this with a `REQUIRED TOOLS` section in `--help` output.

### Goals

- Make the Fabric dependency explicit in field naming so it's obvious which executor runs each stage
- Establish a naming convention that naturally extends to future executor types (`shell-command`, `api-call`, etc.)
- Add `REQUIRED TOOLS` validation to `forge --help` following the gx pattern
- Keep the `fabric:` config block in `forge.yml` for binary path override and model selection

### Non-Goals

- Implementing a multi-executor dispatch system (future work)
- Removing the `fabric:` section from `forge.yml`
- Adding support for any executor type other than Fabric
- Changing how `call_fabric()` works internally

## Proposed Solution

### Overview

Two changes shipped together:

1. **Rename `pattern` → `fabric-pattern`** across YAML and Rust, with `#[serde(rename = "fabric-pattern")]` on the struct field.
2. **Add `REQUIRED TOOLS` to `--help`** using gx's proven `LazyLock` + `after_help` pattern.

### Part 1: Field Rename

#### Rust struct (`src/pipeline.rs`)

```rust
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Stage {
    #[serde(skip_deserializing)]
    pub name: String,
    pub description: String,
    #[serde(rename = "fabric-pattern")]
    pub fabric_pattern: String,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub review: bool,
}
```

#### Validation (`src/pipeline.rs`)

```rust
if stage.fabric_pattern.is_empty() {
    return Err(eyre::eyre!(
        "stage '{}' has no fabric-pattern in pipeline '{}'",
        name,
        self.name
    ));
}
```

#### Executor (`src/executor.rs`)

```rust
let output = call_fabric(
    &config.fabric.binary,
    &stage_def.fabric_pattern,  // was: stage_def.pattern
    &config.fabric.model,
    &fabric_input,
)?;
```

#### Display (`src/lib.rs`)

```rust
println!("     fabric-pattern: {}", stage.fabric_pattern.dimmed());
```

#### Pipeline YAML files

All pipeline files change `pattern:` → `fabric-pattern:`:

```yaml
stages:
  research:
    description: "Gather context, explore prior art, identify constraints and dependencies"
    fabric-pattern: extract_article_wisdom
    references:
      - references/rubrics/research-rubric.md
    review: false
```

**Files:**
- `pipelines/techspec.yml`
- `pipelines/confluence-doc.yml`
- `pipelines/research.yml`
- `pipelines/status-update.yml`
- `pipelines/jira-epic.yml`

#### Tests

All test code that constructs `Stage` structs or inline YAML strings must be updated:
- `src/pipeline.rs` tests -- YAML strings and struct construction
- `src/executor.rs` tests -- `test_pipeline()` helper
- `src/lib.rs` tests -- inline YAML string

### Part 2: Required Tools Validation

#### Data model (`src/cli.rs`)

```rust
use std::process::Command;
use std::sync::LazyLock;

static HELP_TEXT: LazyLock<String> = LazyLock::new(get_tool_validation_help);

#[derive(Debug)]
struct ToolStatus {
    version: String,
    status_icon: String,
}
```

#### Tool checking (`src/cli.rs`)

```rust
fn get_tool_validation_help() -> String {
    let mut help = String::new();
    help.push_str("REQUIRED TOOLS:\n");

    let fabric_status = check_tool_version("fabric", "--version");
    help.push_str(&format!(
        "  {} {:<10} {}\n",
        fabric_status.status_icon, "fabric", fabric_status.version
    ));

    help.push_str(&format!(
        "\nLogs are written to: ~/.local/share/forge/logs/forge.log"
    ));
    help
}

fn check_tool_version(tool: &str, version_arg: &str) -> ToolStatus {
    match Command::new(tool).arg(version_arg).output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            ToolStatus {
                version: if version.is_empty() { "unknown".into() } else { version },
                status_icon: "✅".into(),
            }
        }
        _ => ToolStatus {
            version: "not found".into(),
            status_icon: "❌".into(),
        },
    }
}
```

Note: Unlike gx, we do **not** enforce a minimum version for fabric -- just check presence. Fabric outputs a bare version string (`1.4.376`), so no parsing needed; we display it as-is.

#### CLI integration (`src/cli.rs`)

```rust
#[derive(Parser)]
#[command(
    name = "forge",
    about = "MWP Pipeline Runner -- portable briefcase pattern for content pipelines",
    version = env!("GIT_DESCRIBE"),
    after_help = HELP_TEXT.as_str()
)]
pub struct Cli { ... }
```

#### Expected output

```
$ forge --help
MWP Pipeline Runner -- portable briefcase pattern for content pipelines

Usage: forge [OPTIONS] <COMMAND>
...

REQUIRED TOOLS:
  ✅ fabric     1.4.376

Logs are written to: ~/.local/share/forge/logs/forge.log
```

### Implementation Plan

**Phase 1: Field rename** (all in one commit)
1. Update `Stage` struct in `pipeline.rs`
2. Update validation in `pipeline.rs`
3. Update executor call in `executor.rs`
4. Update display in `lib.rs`
5. Update all pipeline YAML files
6. Update all test code
7. Run `otto ci` to verify

**Phase 2: Required tools** (separate commit)
1. Add `ToolStatus`, `check_tool_version`, `get_tool_validation_help` to `cli.rs`
2. Add `LazyLock` static and wire into `after_help`
3. Remove the existing `after_help` string (logs line moves into the new function)
4. Run `otto ci` to verify

## Alternatives Considered

### Alternative 1: Rename to generic `action`
- **Description:** Rename `pattern` to `action` -- a generic term that could mean anything
- **Pros:** Executor-agnostic naming
- **Cons:** Hides the Fabric coupling instead of making it explicit. A field called `action: extract_article_wisdom` gives no hint that this runs via Fabric. When a second executor type is added, we'd still need to disambiguate.
- **Why not chosen:** The user specifically identified that hiding the coupling is the wrong move. Making it explicit via `fabric-pattern` is honest about what's happening and naturally extends to `shell-command`, `api-call`, etc.

### Alternative 2: Executor enum now
- **Description:** Implement a full executor dispatch system with an enum (`FabricPattern(String)`, `ShellCommand(String)`, etc.)
- **Pros:** Future-proof, clean abstraction
- **Cons:** Over-engineering -- there's only one executor type today. The enum and dispatch logic would be dead code until a second executor is added.
- **Why not chosen:** Violates YAGNI. The rename to `fabric-pattern` establishes the naming convention; the enum can be added when a second executor type is actually needed.

### Alternative 3: Keep `pattern`, add `executor: fabric` field
- **Description:** Add an `executor` field alongside `pattern` to declare the executor type
- **Pros:** Backwards compatible
- **Cons:** Two fields where one suffices. The `executor` field would always be `fabric` and defaulted, adding noise. The field name itself should carry the executor identity.
- **Why not chosen:** The field name convention (`fabric-pattern`, `shell-command`) is cleaner and more self-documenting than a separate executor tag.

## Technical Considerations

### Dependencies

- No new crate dependencies for the rename
- `std::process::Command` and `std::sync::LazyLock` are already available (LazyLock is stable since Rust 1.80)

### Performance

- `LazyLock` ensures tool validation runs at most once per process, and only when `--help` is displayed
- No runtime performance impact on normal `forge` commands

### Testing Strategy

- Existing tests updated in-place (field renames)
- `otto ci` validates: `cargo check`, `cargo test`, `cargo clippy`, `cargo fmt --check`
- Manual verification: `forge --help` shows REQUIRED TOOLS section
- Manual verification: `forge describe techspec` shows `fabric-pattern:` in output

### Rollout Plan

- Both changes land on `main` directly (this is a personal project, no staging)
- Pipeline YAML changes are backwards-incompatible -- old `pattern:` field will fail to deserialize (stage would have empty `fabric_pattern`)
- This is fine: no other consumers of these YAML files exist

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Missed `pattern` reference in code | Low | Low | `otto ci` catches compile errors; grep for `pattern` after |
| `fabric --version` output format changes | Low | Low | We display the raw output; no parsing to break |
| `LazyLock` not available on user's Rust toolchain | Low | Med | Requires Rust 1.80+; forge already uses recent features |

## Open Questions

- [ ] Should the `fabric:` config section in `forge.yml` be renamed or restructured given the per-stage naming?

## References

- gx required tools implementation: `~/repos/scottidler/gx/src/cli.rs` (lines 422-522)
- MWP paper: `docs/interpretable-context-methodology.pdf`
- Fabric CLI: https://github.com/danielmiessler/fabric
