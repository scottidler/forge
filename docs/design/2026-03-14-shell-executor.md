# Design Document: Shell Executor for Forge Pipelines

**Author:** Scott Idler
**Date:** 2026-03-14
**Status:** Superseded by `2026-03-14-command-executor.md`
**Review Passes Completed:** 5/5

## Summary

Add `shell-command` as a second executor type alongside `fabric-pattern` in forge pipeline stages. This allows pipelines to include data-gathering stages that run arbitrary shell commands (scripts, CLI tools, API calls) with their output flowing into subsequent LLM-powered stages. The motivating use case is a foundational initiatives reporting pipeline that gathers data from Jira, GitHub, Confluence, and Slack, then uses fabric patterns to synthesize human-readable descriptions and impact statements.

## Problem Statement

### Background

Forge pipelines today are purely LLM pipelines -- every stage executes a fabric pattern. The input model is: human provides initial context (text or file), fabric patterns transform it through stages, humans review at gates. This works well for content creation (design docs, emails, slide decks) where the starting material is human-authored.

A growing class of pipelines needs to start with machine-gathered data. The foundational initiatives reporting pipeline at Tatari is a concrete example: the raw material lives in Jira epics, GitHub PRs, Confluence pages, and Slack threads. An engineer shouldn't have to manually copy-paste this data into `input.md` -- the pipeline should gather it.

The explicit-fabric-executor design doc (2026-03-13) anticipated this by renaming `pattern` to `fabric-pattern`, establishing a naming convention that "naturally extends to `shell-command`, `api-call`, etc."

### Problem

1. **No non-LLM executor**: Every stage must have a `fabric-pattern`. There's no way to run a script, CLI tool, or API call as a pipeline stage.

2. **Data gathering requires manual pre-work**: For pipelines that consume external data (Jira, GitHub, Confluence, Slack), someone must manually gather that data and feed it as `--input`. This defeats the purpose of automation.

3. **Pipeline definitions are incomplete**: A pipeline YAML that starts at "synthesize" and assumes the data is already gathered doesn't capture the full workflow. The gathering steps are undocumented, manual, and fragile.

### Goals

- Add `shell-command` as a stage executor type alongside `fabric-pattern`
- Each stage has exactly one executor: either `fabric-pattern` or `shell-command`
- Shell stages participate in the same linear data flow (stdin from previous stage, stdout as output)
- Shell stages support the same review gates as fabric stages
- Pipeline YAML remains self-describing -- a single file captures the complete workflow from data gathering through synthesis
- Keep the implementation minimal -- shell out to `sh -c`, no embedded scripting engine

### Non-Goals

- Parallel stage execution (gather Jira and GitHub concurrently) -- future work
- A plugin/extension API for custom executor types beyond shell and fabric
- Sandboxing or security restrictions on shell commands
- Built-in API clients for Jira, GitHub, etc. -- scripts handle this
- Changing how `fabric-pattern` stages work
- Environment variable injection beyond a few forge-specific vars

## Proposed Solution

### Overview

Make `fabric-pattern` optional on `Stage`, add an optional `shell-command` field, and validate that exactly one is set. Add a `call_shell()` function alongside `call_fabric()` in the executor, dispatching based on which field is present. Shell commands run via `sh -c`, receive the composed stage input on stdin, and their stdout becomes the stage output.

### YAML Syntax

```yaml
stages:
  gather-jira:
    description: "Pull foundational initiative epics from Jira"
    shell-command: "scripts/gather-jira.sh"

  gather-context:
    description: "Enrich epics with GitHub PRs, Confluence pages, Slack threads"
    shell-command: "scripts/gather-context.sh"

  synthesize:
    description: "Generate descriptions and impact statements from raw data"
    fabric-pattern: create_initiative_report
    review: true
```

A stage has either `fabric-pattern` or `shell-command`, never both, never neither. This is enforced at validation time.

### Data Model Changes

#### `Stage` struct (`src/pipeline.rs`)

```rust
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Stage {
    #[serde(skip_deserializing)]
    pub name: String,
    pub description: String,
    #[serde(rename = "fabric-pattern", default)]
    pub fabric_pattern: Option<String>,
    #[serde(rename = "shell-command", default)]
    pub shell_command: Option<String>,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub review: bool,
}
```

Key change: `fabric_pattern` goes from `String` (required) to `Option<String>` (optional). `shell_command` is added as `Option<String>`.

#### Validation (`src/pipeline.rs`)

```rust
pub fn validate(&self) -> Result<()> {
    // ... existing name checks ...
    for (name, stage) in &self.stages {
        let has_fabric = stage.fabric_pattern.as_ref().is_some_and(|s| !s.is_empty());
        let has_shell = stage.shell_command.as_ref().is_some_and(|s| !s.is_empty());
        match (has_fabric, has_shell) {
            (true, true) => return Err(eyre!(
                "stage '{}' has both fabric-pattern and shell-command -- pick one",
                name
            )),
            (false, false) => return Err(eyre!(
                "stage '{}' has no executor -- set fabric-pattern or shell-command",
                name
            )),
            _ => {}
        }
    }
    Ok(())
}
```

### Executor Changes

#### Dispatch (`src/executor.rs`)

`execute_stage()` currently unconditionally calls `call_fabric()`. After this change, it inspects the stage definition:

```rust
// Derive working directory from forge_dir (its parent is the project root)
let working_dir = forge_dir.parent()
    .ok_or_else(|| eyre!("cannot determine working directory from .forge/"))?;

// Execute the appropriate executor
let output = if let Some(ref pattern) = stage_def.fabric_pattern {
    call_fabric(&config.fabric.binary, pattern, &config.fabric.model, &fabric_input)?
} else if let Some(ref command) = stage_def.shell_command {
    let resolved = config.resolve_script_path(command)?;
    call_shell(&resolved, &stage_def.name, &fabric_input, working_dir)?
} else {
    // Validation should prevent this, but belt-and-suspenders
    return Err(eyre!("stage '{}' has no executor", stage_def.name));
};
```

#### Shell execution (`src/executor.rs`)

```rust
fn call_shell(command: &str, stage_name: &str, input: &str, working_dir: &Path) -> Result<String> {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command);
    cmd.current_dir(working_dir);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Inject forge environment variables
    cmd.env("FORGE_DIR", working_dir.join(".forge"));
    cmd.env("FORGE_STAGE", stage_name);

    let mut child = cmd.spawn()
        .context(format!("failed to start shell command: {}", command))?;

    // Explicitly drop stdin after writing to signal EOF and avoid deadlock
    // on large inputs (wait_with_output also closes stdin, but explicit is safer)
    {
        let stdin = child.stdin.take()
            .ok_or_else(|| eyre!("failed to open stdin for shell command"))?;
        let mut writer = std::io::BufWriter::new(stdin);
        writer.write_all(input.as_bytes())
            .context("failed to write to shell command stdin")?;
    } // stdin dropped here, sending EOF

    let output = child.wait_with_output()
        .context("failed to wait for shell command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!(
            "shell command failed (exit {}): {}\nCommand: {}",
            output.status, stderr, command
        ));
    }

    // Print stderr as informational (scripts may log progress there)
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        eprintln!("{}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
```

Key design decisions:

1. **`sh -c`**: Commands run through the shell, so pipes, redirects, and variable expansion work. A command like `jira issue list --project DAT | jq '.[] | ...'` works naturally.

2. **Working directory**: The project root (where `.forge/` lives), not inside `.forge/`. Scripts can find `.forge/references/` and previous stage outputs relative to this.

3. **stdin/stdout contract**: Same as fabric -- previous stage output comes in on stdin, this stage's output goes to stdout. This means shell scripts are composable with fabric stages in the same pipeline.

4. **stderr passthrough**: Shell scripts often log progress to stderr. We print it but don't capture it as output. Non-zero exit is an error.

5. **Environment variables**: A small set of forge-specific vars are injected so scripts can locate the forge working directory without hardcoding paths.

### Input Composition for Shell Stages

Shell stages use the same `compose_stage_input()` function as fabric stages. The composed markdown (with TASK, PREVIOUS OUTPUT, and REFERENCE sections) is piped to stdin. This means:

- **Stage 0 shell command**: Receives `input.md` content (or `--input` CLI arg) on stdin. This is the "seed" -- e.g., a Jira filter or project key.
- **Stage N shell command**: Receives previous stage's output on stdin. E.g., the Jira gather script's output flows into the GitHub enrichment script.
- **Fabric stage after shell**: Receives the shell script's stdout as its input, formatted with the usual TASK/PREVIOUS OUTPUT/REFERENCE sections.

This is the simplest design -- shell scripts get the same input as fabric patterns would. Scripts that don't need the structured markdown format can just parse or ignore the section headers.

### Script Location Convention

Shell commands in pipeline YAML are paths relative to the forge home directory (`~/.config/forge/`). This parallels how reference paths work:

```yaml
shell-command: "scripts/gather-jira.sh"
# Resolves to: ~/.config/forge/scripts/gather-jira.sh
```

This means:
- Scripts live alongside pipelines and references in the forge config directory
- Scripts are reusable across pipelines
- No absolute paths in pipeline YAML

The resolution logic mirrors `reference_path()` in `config.rs`:

```rust
// In config.rs
pub fn resolve_script_path(&self, rel: &str) -> Result<String> {
    let home = self.home_dir()?;
    let resolved = home.join(rel);
    if resolved.exists() {
        Ok(resolved.to_string_lossy().to_string())
    } else {
        // Fall back to treating it as a literal command (e.g. inline shell)
        Ok(rel.to_string())
    }
}
```

If the path resolves to a file in forge home, use the absolute path. Otherwise, pass the command through as-is -- this allows inline commands like `shell-command: "curl -s https://api.example.com | jq '.'"` alongside script file references.

### Fabric Availability Check

Currently, `forge unpack` calls `check_fabric_available()` for every pipeline. With shell-command stages, a pipeline might not use fabric at all. The check should only run if the pipeline has at least one `fabric-pattern` stage:

```rust
// In briefcase::unpack()
let has_fabric_stages = pipeline.stages.values()
    .any(|s| s.fabric_pattern.is_some());
if has_fabric_stages {
    executor::check_fabric_available(&config.fabric.binary)?;
}
```

### Display Changes

`forge describe` and `forge show` should display the executor type:

```
Stages:
  1. gather-jira -- Pull foundational initiative epics from Jira
     shell-command: scripts/gather-jira.sh
  2. synthesize -- Generate descriptions and impact statements [review gate]
     fabric-pattern: create_initiative_report
```

### End-to-End Example

To make the full flow concrete, here's what happens when running a pipeline with mixed executor types:

```
$ forge unpack foundational-initiatives --input "DAT" --slug "sre-march-2026"
ok Pipeline 'foundational-initiatives' unpacked
   Run ID: 019...
   Stages: 5
   Next: run `forge run` to execute the first stage

$ forge run
>> Running stage 1/5: gather-jira
   [shell-command: scripts/gather-jira.sh]
   Querying Jira project DAT...
   Found 12 epics with label 'foundational'
ok Output written to .forge/01-gather-jira.md

$ forge run
>> Running stage 2/5: gather-context
   [shell-command: scripts/gather-context.sh]
   Enriching 12 epics with GitHub/Confluence/Slack data...
ok Output written to .forge/02-gather-context.md

$ forge run
>> Running stage 3/5: synthesize
   [fabric-pattern: create_initiative_report]
ok Output written to .forge/03-synthesize.md
--- Stage Output ---
[AI-generated descriptions and impact statements for 12 initiatives]
--- End Output ---
review Stage 'synthesize' is waiting for review.
   Edit .forge/03-synthesize.md if needed, then run `forge run` to approve.

$ vi .forge/03-synthesize.md   # engineer corrects/refines AI drafts
$ forge run                     # approve and continue to next stage
```

The key insight: shell stages (1-2) gather data automatically, fabric stages (3-5) do the LLM synthesis, and review gates let engineers course-correct the AI output. The pipeline YAML captures the entire workflow.

### Implementation Plan

**Phase 1: Data model** (one commit)
1. Make `fabric_pattern` optional (`Option<String>`) in `Stage`
2. Add `shell_command: Option<String>` to `Stage`
3. Update `validate()` to enforce exactly-one-executor rule
4. Update all code that accesses `stage.fabric_pattern` (add `.as_ref()`, `.unwrap()`, etc.)
5. Update tests
6. Run `otto ci`

**Phase 2: Executor dispatch** (one commit)
1. Add `call_shell()` function to `executor.rs`
2. Update `execute_stage()` to dispatch based on executor type
3. Pass stage name and working dir to `call_shell()`
4. Update `compose_stage_input()` if needed (should work as-is)
5. Add tests for `call_shell()`
6. Run `otto ci`

**Phase 3: Conditional fabric check** (one commit)
1. Update `briefcase::unpack()` to only check fabric availability when needed
2. Update `--help` REQUIRED TOOLS to note fabric is "required for fabric-pattern stages"
3. Update display code in `lib.rs` for `forge describe`
4. Run `otto ci`

**Phase 4: Foundational initiatives pipeline** (one commit)
1. Create `~/.config/forge/scripts/` directory
2. Create gather scripts (Jira, GitHub, Confluence, Slack)
3. Create `foundational-initiatives.yml` pipeline
4. Create reference template for the output format
5. End-to-end test with real data

## Alternatives Considered

### Alternative 1: External wrapper script
- **Description:** Keep forge as LLM-only. Write a bash script that gathers data, writes it to a file, then runs `forge unpack ... --input gathered-data.md`.
- **Pros:** No forge changes needed. Works today.
- **Cons:** Pipeline definition is split across two places (the wrapper script and the pipeline YAML). The gathering steps are invisible to forge -- no state tracking, no review gates, no artifact archival. The pipeline YAML doesn't document the full workflow.
- **Why not chosen:** Defeats the purpose of having self-describing pipeline definitions. The wrapper script becomes tribal knowledge.

### Alternative 2: MCP-based data collection
- **Description:** Add MCP tool invocation as an executor type. Stages would call MCP servers (Jira, Slack, GitHub) directly.
- **Pros:** Structured, typed data. Reuses existing MCP infrastructure.
- **Cons:** Forge would need an MCP client. MCP servers return JSON, not markdown -- a transformation layer is needed. More complex than shelling out. Not all data sources have MCP servers.
- **Why not chosen:** Over-engineering for v1. Shell scripts can call MCP servers or any other tool. If MCP becomes the dominant integration pattern, it can be added as a third executor type later.

### Alternative 3: Embedded scripting (Lua, Rhai)
- **Description:** Embed a scripting language so stages can run inline scripts without shelling out.
- **Pros:** Portable, no external script files. Better error handling.
- **Cons:** New dependency. Another language for users to learn. Shell scripts are already universally understood.
- **Why not chosen:** Unnecessary complexity. The target users (engineers) already write shell scripts. `sh -c` is the lightest possible integration.

### Alternative 4: Executor enum (tagged union in YAML)
- **Description:** Instead of two optional fields, use a tagged union:
  ```yaml
  stages:
    gather:
      executor:
        type: shell
        command: "scripts/gather.sh"
  ```
- **Pros:** Cleaner schema. Extensible to N executor types without adding N optional fields.
- **Cons:** More verbose YAML. Breaking change to all existing pipelines. Over-designed for two executor types.
- **Why not chosen:** The two-optional-fields approach is simpler and follows the precedent established by `fabric-pattern`. The naming convention (`fabric-pattern`, `shell-command`) makes each field self-documenting. If a third executor type is ever added, that's the time to consider an enum -- but two is fine with optional fields.

## Technical Considerations

### Dependencies

- No new crate dependencies. `std::process::Command` already handles shell execution (used by `call_fabric()`).
- `sh` must be available on the system (universal on Linux/macOS).

### Performance

- Shell commands are I/O-bound (API calls), not CPU-bound. No performance concerns.
- Sequential execution means gather stages run one at a time. For the foundational initiatives pipeline, this is acceptable -- the bottleneck is API rate limits, not parallelism.

### Security

- Shell commands run with the user's full permissions. This is intentional -- forge is a local tool, not a multi-tenant platform.
- Pipeline YAML files are authored by the user, so `shell-command` values are trusted.
- Scripts that call external APIs will need credentials (Jira tokens, GitHub tokens, Slack tokens). These should come from environment variables or credential stores, never from pipeline YAML or reference files.

### Testing Strategy

- **Unit tests**: `call_shell()` with simple commands (`echo`, `cat`, `exit 1`)
- **Validation tests**: Pipelines with both executors set, neither set, one set
- **Integration tests**: A test pipeline with shell stages that run simple commands, followed by a fabric-pattern stage (mocked)
- **`otto ci`**: `cargo check`, `cargo test`, `cargo clippy`, `cargo fmt --check`
- **Manual**: End-to-end run of the foundational initiatives pipeline

### Rollout Plan

- Land on `main` directly (personal project)
- Existing pipelines are unaffected -- `fabric_pattern` being `Option<String>` is backwards compatible at the YAML level (the field is still present, just now Optional internally)
- Validation change: a stage with `fabric-pattern: ""` that previously passed will now fail. This is correct behavior -- an empty pattern was always a bug.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| `Option<String>` changes ripple through all code touching `stage.fabric_pattern` | High | Low | Compiler catches every site. Methodical update in Phase 1. |
| Shell command fails silently (exit 0 but empty stdout) | Med | Med | Log a warning if stdout is empty after a shell stage. Don't fail -- empty output may be intentional. |
| Scripts assume specific working directory | Med | Low | Document that `cwd` is the project root. Inject `FORGE_DIR` env var for `.forge/` access. |
| Existing pipeline YAML breaks due to validation change | Low | Low | Only breaks if a stage had `fabric-pattern: ""` which was already broken at runtime. |
| Shell commands not portable across OS | Med | Med | Document that scripts should target `sh` (POSIX). Forge is Linux/macOS only. |
| `compose_stage_input()` markdown format is awkward for shell scripts to parse | Med | Low | Scripts can ignore the structure and process raw text, or use simple `sed`/`awk` to extract sections. Future work: add a `raw-input: true` option for shell stages that skips the markdown wrapper. |
| Shell command hangs forever (network timeout, deadlock) | Med | High | Future work: add optional `timeout` field per stage. For now, users can add timeouts in their scripts (`timeout 60 curl ...`) or use Ctrl-C to kill the forge process. |
| Script file not executable or not found | Med | Low | `call_shell` runs via `sh -c`, so the script doesn't need `+x` -- `sh scripts/foo.sh` works. If the resolved path doesn't exist and falls through to literal command, `sh -c` will fail with a clear "not found" error. |
| Large shell output fills memory | Low | Med | Same risk as fabric output today -- forge reads the full stdout into a String. Acceptable for text-based pipelines; not designed for binary data. |

## Open Questions

- [ ] Should shell stages receive raw input (just previous output) or the full composed markdown (with TASK/REFERENCE sections)? Starting with full composed input for simplicity -- can add a `raw-input: true` option later if scripts find the markdown wrapper annoying.
- [ ] Should `shell-command` support a list of commands (multi-line script) or just a single command string? Starting with single string -- multi-step logic belongs in a script file.
- [ ] Should forge resolve `shell-command` paths relative to forge home, the pipeline file's directory, or the working directory? Starting with forge home (parallels reference resolution) -- can revisit if this proves inconvenient.
- [ ] Should there be a per-stage `timeout` field? Useful for shell commands that call external APIs. Not blocking for v1 -- scripts can handle their own timeouts.
- [ ] Should `resolve_script_path()` fall back to literal command, or should script paths and inline commands be distinguished syntactically? The fallback approach is simpler but could mask typos in script paths.

## References

- Explicit fabric executor design doc: `docs/design/2026-03-13-explicit-fabric-executor.md`
- MWP paper: `docs/interpretable-context-methodology.pdf`
- Forge executor implementation: `src/executor.rs`
- Forge pipeline model: `src/pipeline.rs`
