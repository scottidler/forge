# Pipeline Validation -- Current State and Gaps

All six YAML files are now commented with MWP layer annotations.

## Current pipeline validation

Forge has **some** guards but they're minimal. Here's what exists:

**`pipeline.rs:43-63`** -- `validate()` checks three things:
1. Pipeline name is not empty
2. Pipeline has at least one stage
3. Each stage has a name and a pattern

```rust
pub fn validate(&self) -> Result<()> {
    if self.name.is_empty() { ... }
    if self.stages.is_empty() { ... }
    for (i, stage) in self.stages.iter().enumerate() {
        if stage.name.is_empty() { ... }
        if stage.pattern.is_empty() { ... }
    }
}
```

**`briefcase.rs:33`** -- `check_fabric_available()` verifies the fabric binary exists in PATH before unpacking.

**`briefcase.rs:56-59`** -- during unpack, missing reference files get a `log::warn!` but don't halt execution.

## What's NOT validated (gaps)

- **No YAML schema validation** -- if you misspell `stages` as `stage` or `pattern` as `patttern`, serde will either silently ignore the field (if it has a `#[serde(default)]`) or produce a cryptic deserialization error
- **No check that referenced fabric patterns exist** -- you can put `pattern: totally_fake_pattern` and it won't error until `forge run` calls fabric
- **No duplicate stage name detection**
- **No check that `output.destination` or `output.filename` contain valid template variables**
- **No warning when a pipeline references files that don't exist** (only logged, not surfaced to user)
- **No `forge validate` or `forge check` subcommand**
