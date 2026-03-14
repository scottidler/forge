# Design Document: Examples Directory & `forge init` Command

**Author:** Scott Idler
**Date:** 2026-03-13
**Status:** In Review
**Review Passes Completed:** 5/5

## Summary

Add an `examples/` directory with generic scaffold files embedded at compile time via `include_dir!`, and a `forge init` command that copies them to `~/.config/forge/`. This gives new users a working forge setup with a single command, while keeping personal/org-specific pipelines out of the embedded defaults.

## Problem Statement

### Background

Forge currently has no way to bootstrap a new installation. Users must manually create `forge.yml`, pipeline definitions, and reference material before they can run anything. The config resolution chain already looks for `~/.config/forge/forge.yml` (XDG), but nothing creates that directory or its contents.

The repo root contains working pipeline definitions (`pipelines/`) and reference material (`references/`), but these include personal pipelines (jira-epic, confluence-doc, status-update) that aren't appropriate as general-purpose examples.

### Problem

There is no onboarding path. A new user who installs forge has to study existing YAML files, copy them manually, and wire up `forge.yml` before they can run their first pipeline. This friction is unnecessary -- forge could ship sensible defaults and scaffold them with a single command.

### Goals

- Provide a `forge init` command that creates a working `~/.config/forge/` setup
- Embed generic, high-quality example files at compile time (no runtime file resolution)
- Keep personal/org-specific pipelines out of the embedded defaults
- Skip files that already exist (don't overwrite user customizations)
- Support `--force` to overwrite everything

### Non-Goals

- Interactive setup wizard or prompts
- Generating custom pipelines based on user input
- Managing updates to scaffold files after initial creation
- Supporting alternative init targets (always `~/.config/forge/`)

## Proposed Solution

### Overview

1. Create an `examples/` directory in the repo containing generic scaffold files
2. Use `include_dir!` to embed them in the binary at compile time
3. Add a `forge init` CLI command that writes embedded files to `~/.config/forge/`
4. Existing files are skipped by default; `--force` overwrites

### Directory Layout

```
examples/
├── forge.yml
├── pipelines/
│   ├── research.yml
│   └── techspec.yml
└── references/
    ├── voice.md
    ├── templates/
    │   └── techspec.md
    └── rubrics/
        └── techspec-rubric.md
```

**Included** (generic, useful to any forge user):
- `forge.yml` -- minimal config pointing to `~/.config/forge` as home
- `research.yml` -- 3-stage gather/analyze/synthesize pipeline
- `techspec.yml` -- 4-stage research/outline/draft/review pipeline
- `voice.md` -- generic writing style guide
- `techspec.md` -- techspec template structure
- `techspec-rubric.md` -- techspec review rubric

**Excluded** (personal/org-specific):
- `confluence-doc.yml`, `jira-epic.yml`, `status-update.yml`
- `research-rubric.md`, `confluence-doc.md`, `status-update.md`

These stay in `pipelines/` and `references/` in the repo for personal use but are not embedded.

### Example File Contents

**`examples/forge.yml`:**
```yaml
forge:
  version: "1"
  home: ~/.config/forge
  store: ~/.local/share/forge
  pipelines:
    - pipelines/
  fabric:
    binary: fabric
    model: ""
  global_references:
    - references/voice.md
```

**`examples/pipelines/research.yml`** and **`examples/pipelines/techspec.yml`:**
Cleaned-up versions of the current pipeline files with MWP commentary stripped down to brief, helpful comments. Identical structure and stages.

**Note:** The repo's `techspec.yml` references `references/rubrics/research-rubric.md` in its research stage, but `research-rubric.md` is excluded from examples (personal material). The example `techspec.yml` should reference `references/rubrics/techspec-rubric.md` in the research stage instead, or omit the stage-level reference entirely (the pipeline-level techspec template reference is sufficient).

**`examples/references/`:**
Same content as current -- already clean and generic.

### Config Resolution Update

The config resolution chain stays the same:
1. `--config` flag
2. `$FORGE_HOME/forge.yml`
3. `~/.config/forge/forge.yml` (XDG -- primary target of `forge init`)
4. `./forge.yml` (cwd fallback)

No changes needed. The `forge init` command populates option 3.

### Architecture

#### Compile-Time Embedding

```rust
// src/init.rs
use include_dir::{include_dir, Dir};

static EXAMPLES: Dir = include_dir!("$CARGO_MANIFEST_DIR/examples");
```

The `include_dir` crate recursively embeds the entire `examples/` directory into the binary at compile time. No runtime file resolution or path dependencies.

`build.rs` should add `cargo:rerun-if-changed=examples/` so that modifying example files triggers recompilation.

#### File Mapping

`forge init` maps the embedded `examples/` tree 1:1 to `~/.config/forge/`:

```
Embedded (compile-time)              → Target (runtime)
examples/forge.yml                   → ~/.config/forge/forge.yml
examples/pipelines/research.yml      → ~/.config/forge/pipelines/research.yml
examples/pipelines/techspec.yml      → ~/.config/forge/pipelines/techspec.yml
examples/references/voice.md         → ~/.config/forge/references/voice.md
examples/references/templates/...    → ~/.config/forge/references/templates/...
examples/references/rubrics/...      → ~/.config/forge/references/rubrics/...
```

The relative path within `examples/` becomes the relative path within `~/.config/forge/`.

#### Init Command

```rust
// In cli.rs
/// Initialize forge configuration in ~/.config/forge/
Init {
    /// Overwrite existing files
    #[arg(long)]
    force: bool,
}
```

#### Init Logic

```rust
pub fn init(force: bool) -> Result<()> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| eyre!("cannot determine config directory"))?
        .join("forge");

    // Recursively walk embedded files. extract_if/as_file filters out
    // directory entries, leaving only files with contents.
    for entry in EXAMPLES.find("**/*").map_err(|e| eyre!("glob error: {}", e))? {
        if let Some(file) = entry.as_file() {
            let dest = config_dir.join(file.path());
            if dest.exists() && !force {
                println!("Skipped {} (already exists)", dest.display());
                continue;
            }
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&dest, file.contents())?;
            println!("Created {}", dest.display());
        }
    }
    Ok(())
}
```

**Note:** The project uses `#![deny(clippy::unwrap_used)]`, so all fallible operations must use `?` or explicit error handling.

### Data Model

No new data structures. The init command works with the filesystem only -- no TaskStore interaction, no config loading (since config may not exist yet).

### API Design

```
forge init [--force]
```

- No arguments: scaffold `~/.config/forge/` from embedded examples, skip existing files
- `--force`: overwrite all files

Output:
```
$ forge init
Created ~/.config/forge/forge.yml
Created ~/.config/forge/pipelines/research.yml
Created ~/.config/forge/pipelines/techspec.yml
Created ~/.config/forge/references/voice.md
Created ~/.config/forge/references/templates/techspec.md
Created ~/.config/forge/references/rubrics/techspec-rubric.md
```

Re-running without `--force`:
```
$ forge init
Skipped ~/.config/forge/forge.yml (already exists)
Skipped ~/.config/forge/pipelines/research.yml (already exists)
...
```

### Implementation Plan

**Phase 1: Create `examples/` directory**
- Create `examples/forge.yml` with generic config
- Create `examples/pipelines/research.yml` -- stripped of verbose MWP commentary
- Create `examples/pipelines/techspec.yml` -- stripped of verbose MWP commentary
- Copy `references/voice.md`, `references/templates/techspec.md`, `references/rubrics/techspec-rubric.md` into `examples/references/`

**Phase 2: Add `include_dir` dependency and `init` module**
- Add `include_dir = "0.7"` to `Cargo.toml` dependencies
- Create `src/init.rs` with `EXAMPLES` static and `init()` function
- Add `pub mod init;` to `lib.rs`

**Phase 3: Wire up CLI**
- Add `Init { force: bool }` variant to `Command` enum in `cli.rs`
- Add match arm in `run_command()` in `lib.rs`
- Special-case: `forge init` must NOT require a valid config (it creates one)

**Phase 4: Handle config-less execution**
- Currently `main.rs:45` calls `ForgeConfig::load()` before `run_command()` -- this fails if no config exists
- `forge init` must run without a config file (it creates one)
- Solution: match on `Command::Init` in `main.rs` before calling `ForgeConfig::load()`, early-return after init completes

```rust
// main.rs -- before config loading
let cli = Cli::parse();

if let Command::Init { force } = &cli.command {
    return forge::init::init(*force);
}

let config = ForgeConfig::load(cli.config.as_ref())?;
forge::run_command(&cli.command, &config)?;
```

This keeps `run_command()` clean (it still requires `&ForgeConfig`) and avoids optional-config plumbing throughout the codebase.

## Alternatives Considered

### Alternative 1: Runtime File Discovery from Repo

- **Description:** Instead of embedding, ship example files alongside the binary and locate them at runtime via the binary's path or a known location.
- **Pros:** Files can be updated without recompiling.
- **Cons:** Requires knowing where the binary was installed. Breaks with `cargo install`. Adds runtime dependency on file layout. Defeats the "single binary" philosophy.
- **Why not chosen:** `include_dir!` is simpler, more reliable, and aligns with Rust's compile-time embedding pattern.

### Alternative 2: Generate Config Programmatically

- **Description:** Build `forge.yml` and pipeline YAML in code using `serde_yaml::to_string()` on default structs.
- **Pros:** No separate example files to maintain. Config is always in sync with struct definitions.
- **Cons:** Loses comments, formatting, and pedagogical value. Generated YAML is harder to learn from. Two sources of truth (code defaults vs. what users see).
- **Why not chosen:** The example files are meant to teach. Comments and structure matter.

### Alternative 3: `~/.forge/` Instead of XDG

- **Description:** Use `~/.forge/` as the default home directory.
- **Pros:** Shorter path. Discoverable.
- **Cons:** Violates XDG Base Directory Specification. Clutters `$HOME`. Modern tools use `~/.config/`.
- **Why not chosen:** XDG is the right convention. The config chain already supports `~/.config/forge/`.

## Technical Considerations

### Dependencies

- **`include_dir` crate** (new dependency): Compile-time directory embedding. Well-maintained, widely used (4M+ downloads). Version `0.7.x`.
- No new runtime dependencies.

### Performance

- Binary size increases by the size of embedded files (~5-10 KB). Negligible.
- `forge init` is a one-time operation with no performance concerns.

### Security

- Embedded files are static and read-only. No user input involved.
- `--force` overwrites files -- user must opt in explicitly.
- No secrets or credentials in example files.

### Testing Strategy

- **Unit test:** `init()` writes expected files to a temp directory
- **Unit test:** `init()` skips existing files when `force=false`
- **Unit test:** `init()` overwrites existing files when `force=true`
- **Integration test:** Verify `forge init && forge pipelines` works end-to-end
- **Build test:** Verify `EXAMPLES` static compiles and contains expected files

### Rollout Plan

Single PR. No migration needed -- this is purely additive. Existing users are unaffected. The `forge init` command creates files only when explicitly invoked.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Example files drift from current best practices | Medium | Low | Examples are in-repo; PRs naturally touch them |
| `include_dir!` increases compile time | Low | Low | Only embeds ~10 small files |
| Users expect `forge init` to update existing config | Medium | Low | Clear output messaging: "Skipped (already exists)" |
| XDG config dir unavailable on non-standard systems | Low | Medium | `dirs::config_dir()` returns `None` → clear error message |
| `--force` destroys heavily customized config | Low | Medium | Flag name is explicit; consider printing count of files that will be overwritten |
| `examples/` directory accidentally emptied | Low | Low | `init()` should warn if zero files were written (no embedded content found) |

## Open Questions

- [ ] Should `forge init` accept a `--target <path>` to scaffold somewhere other than `~/.config/forge/`?
- [ ] Should pipeline YAML comments be stripped entirely or kept minimal in examples?
- [ ] Should `forge init` also create `~/.local/share/forge/` (the store directory)? (Likely no -- `forge unpack` creates it on first use via TaskStore.)

## References

- [`include_dir` crate](https://crates.io/crates/include_dir)
- [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/)
- [Pipeline Directory Discovery design doc](./2026-03-13-pipeline-directory-discovery.md)
- [Stages IndexMap design doc](./2026-03-13-stages-indexmap.md)
