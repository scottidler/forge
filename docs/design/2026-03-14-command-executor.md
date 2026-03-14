# Design Document: Executor-Agnostic Command Model

**Author:** Scott Idler
**Date:** 2026-03-14
**Status:** Implemented
**Supersedes:** `2026-03-14-shell-executor.md`

## Summary

Replace the `fabric-pattern` stage field with a generic `command` + `args` model. Stages become executor-agnostic -- they can run fabric, shell scripts, python, curl, or any command that reads stdin and writes stdout. This removes forge's tight coupling to the fabric binary and makes pipelines truly tool-independent.

## Problem Statement

### Background

Forge stages are currently hard-wired to fabric. Every stage must declare a `fabric-pattern`, which is a specific argument to the `fabric` CLI binary. The config has a `fabric:` block with binary path and model override. The executor has a dedicated `call_fabric()` function. The `--help` output checks if fabric is installed.

A prior design doc (`2026-03-14-shell-executor.md`) proposed adding `shell-command` as a second executor type alongside `fabric-pattern`. But this creates a pattern where every new tool needs its own executor field -- `fabric-pattern`, `shell-command`, `api-call`, `python-script`, etc. Each addition means new struct fields, new validation rules, new dispatch logic, and new config blocks.

The real problem isn't "how do we also run shell scripts" -- it's that forge shouldn't know or care what tool a stage uses.

### Problem

1. **Tight coupling to fabric**: The `Stage` struct, executor, config, CLI, and validation all assume fabric is the only tool. Adding any other tool requires changes across the entire codebase.

2. **Pipeline YAML is not self-describing**: `fabric-pattern: extract_article_wisdom` only makes sense if you know fabric's CLI interface. The pipeline YAML should describe what runs, not reference internal arguments of a specific tool.

3. **Runtime argument generation is impossible**: Some stages need args that are determined at execution time -- reference file paths, run IDs, stage context, user input. A single string field like `fabric-pattern` can't accommodate this.

4. **The foundational-initiatives pipeline can't load**: It has `shell-command` stages that the current parser rejects because `fabric-pattern` is required on every stage.

### Goals

- Single `command` + `args` model replaces all executor-specific fields
- `args` supports template variable expansion for runtime-generated values
- Forge environment variables (`FORGE_DIR`, `FORGE_STAGE`, etc.) injected into every command
- stdin/stdout contract unchanged (MWP's plain text interface)
- Stage sequencing, review gates, references, briefcase pattern all unchanged
- Forge has zero knowledge of any specific tool (fabric, sh, python, etc.)
- Clean break -- no backward compatibility with `fabric-pattern` (pre-1.0)

### Non-Goals

- Parallel stage execution
- Built-in support for any specific tool
- Sandboxing or security restrictions on commands
- Changing the context composition model (TASK + PREVIOUS OUTPUT + REFERENCE stdin)
- Plugin/extension API for executor types

## Proposed Solution

### YAML Syntax

```yaml
stages:
  # Fabric pattern (the common case)
  research:
    description: "Gather context and background"
    command: fabric
    args:
      - "-p"
      - "extract_article_wisdom"
    review: false

  # Shell script
  gather-jira:
    description: "Pull foundational initiative epics from Jira"
    command: scripts/gather-jira.sh
    args:
      - "--project"
      - "{input}"
    review: false

  # Fabric with model override
  draft:
    description: "Write full draft"
    command: fabric
    args:
      - "-p"
      - "create_design_document"
      - "-m"
      - "claude-sonnet-4-6"
    review: true

  # Python script
  analyze:
    description: "Run custom analysis"
    command: python3
    args:
      - "scripts/analyze.py"
      - "--format"
      - "markdown"
    review: true

  # Inline shell (pipes, redirects)
  fetch:
    description: "Fetch and filter API data"
    command: sh
    args:
      - "-c"
      - "curl -s https://api.example.com/data | jq '.items[]'"
    review: false
```

A stage has a `command` (required) and `args` (optional list). Forge runs the command with the args, piping composed context to stdin and capturing stdout as the stage output. That's it.

### Data Model Changes

#### `Stage` struct (`src/pipeline.rs`)

```rust
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Stage {
    #[serde(skip_deserializing)]
    pub name: String,
    pub description: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub review: bool,
}
```

`fabric_pattern: String` is gone. `command: String` and `args: Vec<String>` replace it.

#### Validation (`src/pipeline.rs`)

```rust
pub fn validate(&self) -> Result<()> {
    if self.name.is_empty() {
        return Err(eyre::eyre!("pipeline name is empty"));
    }
    if self.stages.is_empty() {
        return Err(eyre::eyre!("pipeline '{}' has no stages", self.name));
    }
    for (name, stage) in &self.stages {
        if name.is_empty() {
            return Err(eyre::eyre!("stage has empty name in pipeline '{}'", self.name));
        }
        if stage.command.is_empty() {
            return Err(eyre::eyre!(
                "stage '{}' has no command in pipeline '{}'",
                name, self.name
            ));
        }
    }
    Ok(())
}
```

Simpler than before -- the only requirement is a non-empty `command`.

#### Config changes (`src/config.rs`)

Remove `FabricConfig` entirely:

```rust
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ForgeConfig {
    pub version: String,
    pub home: String,
    pub store: String,
    #[serde(default)]
    pub pipelines: Vec<String>,
    #[serde(default)]
    pub global_references: Vec<String>,
}
```

The `fabric:` block in `forge.yml` is gone. Forge has no tool-specific configuration.

### Template Variables

Args support `{variable}` expansion at execution time:

| Variable | Value | Example |
|----------|-------|---------|
| `{stage}` | Current stage name | `research` |
| `{stage_num}` | 1-indexed stage number | `1` |
| `{forge_dir}` | Absolute path to `.forge/` | `/home/user/project/.forge` |
| `{run_id}` | UUID of current pipeline run | `019...` |
| `{pipeline}` | Pipeline name | `techspec` |
| `{prev_output}` | Path to previous stage output file | `.forge/01-research.md` |

Expansion is a simple string-replace pass. Unrecognized `{...}` tokens are left as-is -- this avoids collisions with commands that use literal braces (jq, shell parameter expansion, etc.).

```rust
fn expand_template(arg: &str, vars: &HashMap<&str, String>) -> String {
    let mut result = arg.to_string();
    for (key, value) in vars {
        result = result.replace(&format!("{{{}}}", key), value);
    }
    result
}
```

### Executor Changes

#### Single `call_command()` replaces `call_fabric()` and `check_fabric_available()`

```rust
fn call_command(
    command: &str,
    args: &[String],
    input: &str,
    working_dir: &Path,
    env_vars: &HashMap<String, String>,
) -> Result<String> {
    let mut cmd = Command::new(command);
    cmd.args(args);
    cmd.current_dir(working_dir);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    for (k, v) in env_vars {
        cmd.env(k, v);
    }

    let mut child = cmd.spawn()
        .context(format!("failed to start command: {} {}", command, args.join(" ")))?;

    {
        let stdin = child.stdin.take()
            .ok_or_else(|| eyre!("failed to open stdin"))?;
        let mut writer = std::io::BufWriter::new(stdin);
        writer.write_all(input.as_bytes())
            .context("failed to write to command stdin")?;
    } // stdin dropped here, sending EOF

    let output = child.wait_with_output()
        .context("failed to wait for command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!(
            "command failed (exit {}): {}\nCommand: {} {}",
            output.status, stderr, command, args.join(" ")
        ));
    }

    // Pass through stderr (commands may log progress there)
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        eprintln!("{}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
```

#### `execute_stage()` dispatch

```rust
// Build template variables
let mut vars = HashMap::new();
vars.insert("stage", stage_def.name.clone());
vars.insert("stage_num", stage_num.to_string());
vars.insert("forge_dir", forge_dir.to_string_lossy().to_string());
vars.insert("run_id", run.id.clone());
vars.insert("pipeline", run.pipeline.clone());
if stage_index > 0 {
    if let Some((_, prev)) = pipeline.stages.get_index(stage_index - 1) {
        let prev_file = forge_dir.join(format!("{:02}-{}.md", stage_index, prev.name));
        vars.insert("prev_output", prev_file.to_string_lossy().to_string());
    }
}

// Expand template variables in args
let expanded_args: Vec<String> = stage_def.args.iter()
    .map(|arg| expand_template(arg, &vars))
    .collect();

// Build environment variables
let mut env_vars = HashMap::new();
env_vars.insert("FORGE_DIR".to_string(), forge_dir.to_string_lossy().to_string());
env_vars.insert("FORGE_STAGE".to_string(), stage_def.name.clone());
env_vars.insert("FORGE_RUN_ID".to_string(), run.id.clone());
env_vars.insert("FORGE_PIPELINE".to_string(), run.pipeline.clone());

let working_dir = forge_dir.parent()
    .ok_or_else(|| eyre!("cannot determine working directory"))?;

let output = call_command(
    &stage_def.command,
    &expanded_args,
    &stage_input,
    working_dir,
    &env_vars,
)?;
```

### Context Composition -- Unchanged

`compose_stage_input()` stays exactly as it is. The composed markdown (TASK + INPUT/PREVIOUS OUTPUT + REFERENCE sections) goes to stdin regardless of what command runs. This is the MWP contract.

Commands that don't need the structured markdown can ignore the section headers. The `{prev_output}` template variable gives file-path access to the raw previous output for commands that prefer to read files instead of parsing stdin.

### Display Changes

`forge describe` shows the full command:

```
Stages:
  1. gather-jira -- Pull foundational initiative epics from Jira
     command: scripts/gather-jira.sh
  2. research -- Gather context and background
     command: fabric -p extract_article_wisdom
  3. outline -- Create structural outline [review gate]
     command: fabric -p create_outline
```

### Briefcase Changes

- Remove `executor::check_fabric_available()` call from `briefcase::unpack()` -- forge doesn't know what tools the pipeline uses
- Everything else unchanged (references, symlinks, store records, gitignore)

### CLI Changes

- Remove `REQUIRED TOOLS` section from `--help`
- Remove `ToolStatus`, `check_tool_version()`, `get_tool_validation_help()`
- The `LazyLock<String>` for help text simplifies to just the log path info

### Implementation Plan

**Phase 1: Data model and validation** (one commit)
1. Change `Stage` struct: `fabric_pattern: String` -> `command: String` + `args: Vec<String>`
2. Update custom deserializer
3. Update `validate()` -- require non-empty `command`
4. Fix all compiler errors from the struct change (executor, lib, tests)
5. `otto ci`

**Phase 2: Config simplification** (one commit)
1. Remove `FabricConfig` from config.rs
2. Remove `fabric` field from `ForgeConfig`
3. Update config YAML parsing and tests
4. `otto ci`

**Phase 3: Executor rewrite** (one commit)
1. Replace `call_fabric()` and `check_fabric_available()` with `call_command()`
2. Add `expand_template()` function
3. Update `execute_stage()` to build vars, expand args, dispatch
4. Remove fabric check from `briefcase::unpack()`
5. Add tests for template expansion, command execution (with `echo`/`cat`)
6. `otto ci`

**Phase 4: CLI and display cleanup** (one commit)
1. Remove tool validation from `cli.rs`
2. Update `cmd_describe()` in `lib.rs` to show `command` + `args`
3. `otto ci`

**Phase 5: Pipeline YAML migration** (one commit)
1. Rewrite all pipeline YAMLs: `fabric-pattern: X` -> `command: fabric` + `args: ["-p", "X"]`
2. Rewrite `foundational-initiatives.yml` with `command:` for shell stages
3. Update `forge.yml` -- remove `fabric:` block, bump version to `"2"`
4. Update `examples/` directory
5. `otto ci`

**Phase 6: Test updates** (one commit)
1. Update MWP conformance tests
2. Add tests for template expansion, command invocation, validation edge cases
3. `otto ci`

## Alternatives Considered

### Alternative 1: Two executor types (shell-executor design doc)

- **Description:** Add `shell-command` alongside `fabric-pattern` as optional fields. Validate exactly one is set.
- **Pros:** Backward compatible. Small diff.
- **Cons:** Every new tool needs a new field, new dispatch logic, new config. Forge accumulates tool-specific knowledge. The two-optional-fields pattern scales poorly.
- **Why not chosen:** Solves the immediate problem but creates a pattern that fights the MWP philosophy. The paper's principle is "local scripts handle the parts that do not need AI" -- forge shouldn't know the difference.

### Alternative 2: Single `command` string (no `args` list)

- **Description:** `command: "fabric -p extract_article_wisdom"` -- one string, parsed by the shell.
- **Pros:** Simpler YAML. One field.
- **Cons:** Can't generate args at runtime. Can't expand template variables cleanly (shell quoting issues). Can't distinguish the executable from its arguments for error messages or path resolution.
- **Why not chosen:** Fails the runtime arg generation requirement. Template expansion inside a shell-parsed string is fragile and error-prone.

### Alternative 3: Keep `fabric-pattern` as convenience shorthand

- **Description:** Add generic `command` + `args`, but also keep `fabric-pattern` as syntactic sugar that expands to `command: fabric, args: ["-p", pattern]`.
- **Pros:** Less migration work. Familiar syntax.
- **Cons:** Two ways to do the same thing. Confusion about which to use. The sugar hides the real command, making `forge describe` output inconsistent. If fabric's CLI changes, the sugar breaks.
- **Why not chosen:** Pre-1.0, clean break is better than accumulated compatibility layers.

### Alternative 4: Executor enum (tagged union)

```yaml
stages:
  gather:
    executor:
      type: shell
      command: "scripts/gather.sh"
```

- **Pros:** Extensible. Type-safe.
- **Cons:** More verbose. Over-designed -- every stage has the same executor model (command + args + stdin/stdout). There's no behavioral difference between "types" of executors.
- **Why not chosen:** The Unix insight is that all commands share the same interface. There are no types -- there are just programs that read stdin and write stdout.

## Technical Considerations

### Dependencies

- No new crate dependencies. `std::process::Command` handles all execution.
- `FabricConfig` removal is a net deletion of code.

### Performance

- No performance change. Command spawning is the same whether we call it `call_fabric()` or `call_command()`.

### Security

- Commands run with the user's full permissions. This is intentional -- forge is a local tool.
- Pipeline YAMLs are user-authored, so `command` values are trusted.
- Template expansion does not execute code -- it's string replacement only.

### Testing Strategy

- **Template expansion**: Unit tests for all variables, unrecognized tokens, edge cases
- **Command execution**: Integration tests with `echo` and `cat` (no external tools needed)
- **Validation**: Empty command, empty args, valid stages
- **Pipeline parsing**: Updated YAML samples parse correctly
- **MWP conformance**: Update the existing 33 conformance tests
- **Manual**: End-to-end run of a fabric pipeline + the foundational-initiatives pipeline

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Wide compiler ripple from `fabric_pattern` removal | High | Low | Compiler catches every site. Phase 1 is mechanical. |
| Template `{var}` collides with literal braces in args | Low | Low | Only known variable names are expanded. Unrecognized tokens pass through. |
| Verbose YAML (2 extra lines per fabric stage) | High | Low | Acceptable tradeoff for tool independence. Still self-documenting. |
| No default fabric model | Med | Low | Users set model in fabric's own config, or add `-m` to stage args. Not forge's concern. |
| Scripts fail because PATH doesn't include expected binaries | Med | Low | Error message shows the command that failed. Standard debugging. |

## Open Questions

- [ ] Should `command` support tilde expansion (`~/scripts/foo.sh`)? Currently only `args` would get template expansion. Tilde in `command` could be expanded via `shellexpand`.
- [ ] Should there be environment variable expansion in `args` beyond forge-specific template vars? e.g., `$JIRA_PROJECT` or `{env.JIRA_PROJECT}`. Not blocking for v1.
- [ ] Should forge resolve `command` paths relative to forge home (like references)? This would let `command: scripts/gather.sh` resolve to `~/.config/forge/scripts/gather.sh`. Useful for reusable scripts.

## References

- Superseded design doc: `docs/design/2026-03-14-shell-executor.md`
- MWP paper: `docs/interpretable-context-methodology.pdf`
- Forge executor implementation: `src/executor.rs`
- Forge pipeline model: `src/pipeline.rs`
