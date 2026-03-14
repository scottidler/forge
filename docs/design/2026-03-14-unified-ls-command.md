# Design Document: Unified `ls` Command

**Author:** Scott Idler
**Date:** 2026-03-14
**Status:** Implemented
**Review Passes Completed:** 5/5

## Summary

Merge `forge pipelines` and `forge ls` into a single `forge ls` command that lists all available pipeline definitions by default, with active run counts inline. Positional arguments enable a detailed view for matching pipelines (with substring matching), and `--all` expands the detailed view to every pipeline.

## Problem Statement

### Background

Forge currently has three separate listing commands:
- `forge ls` - lists active pipeline runs (Unpacked/InProgress status)
- `forge ls --all` - lists all pipeline runs including completed/abandoned
- `forge pipelines` - lists available pipeline definitions with their paths

A user who wants to answer "what pipelines exist and which ones have active work?" must run two commands and mentally correlate the results.

### Problem

1. **Discovery is split across commands**: New users don't know whether to run `ls` or `pipelines`. The most natural first command (`forge ls`) shows nothing if no runs are active, giving the impression forge has no pipelines configured.

2. **No unified view**: There is no single command that shows both the catalog of available pipelines and the state of active work. The user must cross-reference `forge pipelines` output with `forge ls` output.

3. **`forge pipelines` is a dead-end**: It shows names and paths but no status, no stage count, no description. It's useful only for debugging config, not for workflow.

### Goals

- Single `forge ls` command replaces both `forge pipelines` and the old `forge ls`
- Default (no args) shows all pipelines with active run counts inline
- Positional args select specific pipelines for a detailed view, with substring matching
- `--all` flag shows the detailed view for every pipeline
- Detailed view includes pipeline metadata (description, stages) and nested active runs
- Remove the `Pipelines` subcommand variant from the CLI

### Non-Goals

- Changing the `forge history` command (it serves a different purpose - time-ordered view)
- Adding filtering by run status (Completed, Abandoned, etc.) to `ls`
- Changing the data model or store schema
- Interactive/TUI pipeline selection

## Proposed Solution

### Overview

The `forge ls` command operates in two modes:

**Compact mode** (no positional args): lists every pipeline definition on one line, appending `(N)` when active runs exist.

**Detailed mode** (positional args or `--all`): for each matched pipeline, shows pipeline metadata and nests active runs underneath.

### CLI Design

```
forge ls [PIPELINES...] [--all]
```

- `forge ls` - compact list of all pipelines
- `forge ls techspec` - detailed view for techspec
- `forge ls tech` - detailed view for any pipeline matching "tech" (substring)
- `forge ls tech blog` - detailed view for pipelines matching "tech" or "blog"
- `forge ls --all` - detailed view for every pipeline

`--all` and positional args are independent: `forge ls tech --all` is the same as `forge ls tech` (positional args already trigger detailed mode, `--all` just means "all pipelines"). If both are given, `--all` wins and positional args are ignored.

### Output Format

**Compact mode:**

```
Pipelines:
  blog-post   - Blog post writing pipeline
  research    - Deep research pipeline
  techspec    - Research, outline, draft, and review a technical specification (2)
```

Rules:
- Sorted alphabetically by pipeline name
- Pipeline name in cyan, description after dash
- Active run count in parens after description, only shown when > 0
- No run details, no stage info - kept terse

**Detailed mode:**

```
techspec (3 stages) - Research, outline, draft, and review a technical specification
  output: docs/design/{date}-{slug}.md
  stages: research -> outline [review] -> draft [review]
  runs:
    a1b2c3d4  [InProgress]  stage 2/3 (outline)   ~/writing/api-redesign
    f9e8d7c6  [Unpacked]    stage 1/3 (research)   ~/writing/auth-revamp

research (4 stages) - Deep research pipeline
  output: research/{date}-{slug}.md
  stages: gather -> analyze -> synthesize -> summarize
  (no active runs)
```

Rules:
- Pipeline name bold + cyan, stage count in parens, description after dash
- Output destination and filename from pipeline definition
- Stage chain shown as `name -> name [review]` with review gates marked
- Active runs (Unpacked/InProgress) nested underneath with run ID prefix, status, stage progress, working dir
- Pipelines without active runs show `(no active runs)` in dimmed text
- Multiple matched pipelines separated by blank line

### Substring Matching

Pipeline name matching uses case-insensitive substring containment:

```rust
fn matches_pipeline(name: &str, pattern: &str) -> bool {
    name.to_lowercase().contains(&pattern.to_lowercase())
}
```

If a positional arg matches zero pipelines, print a warning:
```
No pipeline matching 'xyz' found.
```

If multiple args are given, union the matches. Deduplicate so a pipeline matched by multiple patterns only appears once.

### Architecture

**Changes to `cli.rs`:**

Remove the `Pipelines` variant from the `Command` enum. Update `Ls` to accept positional args:

```rust
/// List pipelines and active runs
Ls {
    /// Pipeline names or substrings to show in detail
    pipelines: Vec<String>,

    /// Show detailed view for all pipelines
    #[arg(long)]
    all: bool,
},
```

**Changes to `lib.rs`:**

- Remove `cmd_pipelines()` function
- Update `run_command()` match arm: `Command::Ls { pipelines, all } => cmd_ls(config, &pipelines, *all)`
- Remove the `Command::Pipelines => cmd_pipelines(config)` match arm
- Rewrite `cmd_ls()` to implement both compact and detailed modes
- Add helper functions: `format_stage_chain()`, `matches_pipeline()`, `load_active_runs()`, `filter_pipelines()`

The new `cmd_ls` dispatches based on whether pipelines are specified:

```rust
fn cmd_ls(config: &ForgeConfig, pipelines: &[String], all: bool) -> Result<()> {
    let available = config.list_pipelines()?;
    let store_dir = config.store_dir()?;
    let active_runs = load_active_runs(&store_dir)?;

    if pipelines.is_empty() && !all {
        cmd_ls_compact(&available, &active_runs)
    } else {
        let matched = if all {
            available.clone()
        } else {
            filter_pipelines(&available, pipelines)?
        };
        cmd_ls_detailed(config, &matched, &active_runs)
    }
}
```

**Helper: `load_active_runs`** - queries the store for all Unpacked + InProgress runs, returns them grouped by pipeline name in a `HashMap<String, Vec<PipelineRun>>`. If the store directory doesn't exist (fresh install, no runs ever created), returns an empty map rather than erroring.

**Helper: `filter_pipelines`** - given the full pipeline list and user patterns, returns the subset that match. Warns on patterns that match nothing.

**Helper: `format_stage_chain`** - loads a Pipeline definition and formats its stages as `name -> name [review] -> name`.

### Data Flow

```
forge ls tech
  |
  v
list_pipelines() -> [(name, path), ...]       # from config/filesystem
  |
  v
load_active_runs(&store_dir) -> HashMap       # query Unpacked + InProgress, group by pipeline name
  |
  v
filter_pipelines(patterns=["tech"])            # substring match on names
  |
  v
Pipeline::load(path) for each matched entry    # load YAML for description + stages
  |
  v
cmd_ls_detailed()                              # format output with pipeline info + nested runs
```

Both compact and detailed modes load each `Pipeline` YAML. The difference is that compact only uses the description while detailed also formats the stage chain and run details.

### Implementation Plan

**Phase 1: Core refactor**
1. Update `Command::Ls` in `cli.rs` to accept `pipelines: Vec<String>` positional arg
2. Remove `Command::Pipelines` variant and its match arm in `run_command()`
3. Rewrite `cmd_ls()` with compact/detailed dispatch
4. Implement `load_active_runs()`, `filter_pipelines()`, `matches_pipeline()`, `format_stage_chain()`

**Phase 2: Output formatting**
1. Implement `cmd_ls_compact()` with aligned columns and run counts
2. Implement `cmd_ls_detailed()` with pipeline metadata + nested runs
3. Apply colored output consistent with existing forge style

**Phase 3: Tests**
1. Update existing `test_cmd_ls_*` tests for new signature
2. Add tests for substring matching (exact, partial, case-insensitive, no-match)
3. Add tests for compact mode with and without active runs
4. Add tests for detailed mode with mixed active/inactive pipelines
5. Remove `test_cmd_pipelines_*` tests

## Alternatives Considered

### Alternative 1: Keep `pipelines` as separate command
- **Description:** Leave `forge pipelines` and `forge ls` as distinct commands
- **Pros:** No breaking change, each command has a single responsibility
- **Cons:** Discovery problem remains, users must correlate two outputs
- **Why not chosen:** The whole point is unifying the view

### Alternative 2: Subcommands under `ls` (`forge ls runs`, `forge ls pipelines`)
- **Description:** Make `ls` a parent command with `runs` and `pipelines` subcommands
- **Pros:** Explicit, extensible
- **Cons:** More typing for the common case, `forge ls` alone would need a default, feels over-structured for a CLI tool
- **Why not chosen:** Positional args with substring matching is more ergonomic

### Alternative 3: Emoji markers for active pipelines
- **Description:** Use emoji (e.g., green circle) instead of `(N)` count for active runs
- **Pros:** Visually scannable
- **Cons:** Terminal compatibility issues, harder to parse programmatically, doesn't convey count
- **Why not chosen:** `(N)` is more informative and universally renderable

## Technical Considerations

### Dependencies

No new dependencies. Uses existing `colored`, `taskstore`, `serde_yaml`, and `Pipeline::load()`.

### Performance

The compact view needs to load each pipeline YAML to get descriptions (see note below). The detailed view additionally uses the loaded `Pipeline` for stage info. For typical pipeline counts (< 20), this is negligible.

The store query (`load_active_runs`) fetches all Unpacked + InProgress runs (two queries, same as current `cmd_ls`), then groups them by pipeline name in a `HashMap` via a single iteration. No new store indexes needed.

**Note on `list_pipelines()`:** The current `config.list_pipelines()` returns `Vec<(String, PathBuf)>` - name and path only, no description. The compact view needs descriptions. Two options:
1. **Load each Pipeline YAML** in the compact view to extract the description. For < 20 pipelines this is fast.
2. **Extend `list_pipelines()`** to return a richer struct that includes the description.

Option 1 is simpler and avoids changing the config module. The compact view calls `Pipeline::load()` for each entry and extracts the description. If a YAML fails to load, print the pipeline name with a `(load error)` note and continue.

### Testing Strategy

- Unit tests for `matches_pipeline()` - exact, substring, case sensitivity, empty pattern
- Unit tests for `filter_pipelines()` - single match, multi-match, no match, overlapping patterns
- Unit tests for `format_stage_chain()` - with and without review gates
- Integration tests for `cmd_ls()` - compact mode empty, compact with runs, detailed with runs, detailed without runs, `--all`
- Existing tests for `cmd_ls_empty_store` and `cmd_ls_with_runs` updated for new signature

### Migration

The `Pipelines` command is removed. Since forge is pre-1.0, this is a clean break with no backward compatibility needed (consistent with the approach taken in the command-executor design doc).

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Substring match is too greedy (e.g., `s` matches everything) | Low | Low | User can be more specific; not destructive |
| Pipeline YAML load failure in detailed mode breaks entire listing | Medium | Medium | Catch per-pipeline load errors, print warning, continue with remaining |
| Removing `Pipelines` command breaks muscle memory | Low | Low | Pre-1.0 tool, small user base |

## Open Questions

- [ ] Should `forge ls` show the pipeline path (from config) in compact mode, or only in detailed mode?
- [ ] Should `--all` in detailed mode also include completed/abandoned runs, or only active ones?
- [ ] Should the description in compact mode be truncated at a max width for alignment?

## References

- Current `cmd_ls()` implementation: `src/lib.rs:130-182`
- Current `cmd_pipelines()` implementation: `src/lib.rs:33-48`
- Pipeline definition: `src/pipeline.rs`
- Store/run model: `src/store.rs`
- Prior design doc style: `docs/design/2026-03-14-command-executor.md`
