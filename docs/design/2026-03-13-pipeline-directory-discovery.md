# Design Document: Pipeline Directory Discovery

**Author:** Scott Idler
**Date:** 2026-03-13
**Status:** Implemented
**Review Passes Completed:** 4/4

## Summary

Replace the explicit name→file mapping in `forge.yml` `pipelines` with an ordered list of directories. Pipeline names are derived from filename stems (`techspec.yml` → `techspec`). Resolution follows `$PATH` semantics: first directory wins on collision.

## Problem Statement

### Background

Currently, `forge.yml` requires every pipeline to be listed explicitly:

```yaml
pipelines:
  techspec: pipelines/techspec.yml
  research: pipelines/research.yml
  confluence-doc: pipelines/confluence-doc.yml
  status-update: pipelines/status-update.yml
  jira-epic: pipelines/jira-epic.yml
```

The key always matches the filename stem. Adding a new pipeline requires editing both the YAML file and dropping the pipeline definition -- a redundant two-step process.

### Problem

The explicit mapping is pure boilerplate. It violates DRY, creates friction when adding pipelines, and doesn't enable any behavior the filesystem doesn't already provide.

### Goals

- Eliminate redundant name→file mapping
- Support multiple pipeline directories (local override, shared libraries)
- Derive pipeline names from filenames by convention
- Maintain backward compatibility during transition (optional)

### Non-Goals

- Glob/regex filtering within directories
- Nested subdirectory organization (pipelines must be top-level `.yml` files in each directory)
- Pipeline aliasing (e.g., `ts` → `techspec`) -- additive future work
- Hot-reloading or watching for new pipeline files

## Proposed Solution

### Overview

Change `pipelines` from `HashMap<String, String>` to `Vec<String>` where each entry is a directory path (relative to `home` or absolute). At resolution time, scan directories in order for `*.yml` files. Pipeline name = file stem. First match wins.

### Config Format

```yaml
pipelines:
  - pipelines/
  - ~/shared-pipelines/
```

### Name Resolution

Given `forge unpack techspec`:

1. Expand each directory relative to `home` (or absolute if it starts with `/` or `~`)
2. For each directory in order, check if `{dir}/techspec.yml` exists
3. First hit → load that file
4. No hit → error: `unknown pipeline: techspec`

### Listing All Pipelines

For `forge pipelines`:

1. Scan each directory for `*.yml` files
2. For each file, derive name from stem
3. Earlier directories win on name collision (later duplicates are shadowed)
4. Display source directory alongside each pipeline name

### Data Model

**Before:**
```rust
pub struct ForgeConfig {
    pub pipelines: HashMap<String, String>,
    // ...
}
```

**After:**
```rust
pub struct ForgeConfig {
    #[serde(default)]
    pub pipelines: Vec<String>,
    // ...
}
```

### API Changes

**Helper: `ForgeConfig::resolve_pipeline_dir(&self, dir: &str) -> Result<PathBuf>`**

Shared logic for expanding a pipeline directory entry to an absolute path:

```rust
fn resolve_pipeline_dir(&self, dir: &str) -> Result<PathBuf> {
    let expanded = shellexpand::tilde(dir);
    let path = Path::new(expanded.as_ref());
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(self.home_dir()?.join(path))
    }
}
```

**`ForgeConfig::pipeline_path(&self, name: &str) -> Result<PathBuf>`**

Currently does a HashMap lookup. New implementation iterates directories:

```rust
pub fn pipeline_path(&self, name: &str) -> Result<PathBuf> {
    let filename = format!("{}.yml", name);
    for dir in &self.pipelines {
        let dir_path = self.resolve_pipeline_dir(dir)?;
        let candidate = dir_path.join(&filename);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(eyre!("unknown pipeline: {} (searched: {:?})", name, self.pipelines))
}
```

**New: `ForgeConfig::list_pipelines(&self) -> Result<Vec<(String, PathBuf)>>`**

Returns all discovered pipelines as `(name, path)` pairs, respecting first-match-wins shadowing:

```rust
pub fn list_pipelines(&self) -> Result<Vec<(String, PathBuf)>> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for dir in &self.pipelines {
        let dir_path = self.resolve_pipeline_dir(dir)?;
        if !dir_path.is_dir() {
            log::warn!("pipeline directory not found: {}", dir_path.display());
            continue;
        }
        for entry in fs::read_dir(&dir_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "yml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if seen.insert(stem.to_string()) {
                        result.push((stem.to_string(), path));
                    }
                }
            }
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}
```

### Implementation Plan

**Phase 1: Config change**
- Change `ForgeConfig.pipelines` from `HashMap<String, String>` to `Vec<String>`
- Implement new `pipeline_path()` with directory scanning
- Add `list_pipelines()` method

**Phase 2: Consumer updates**
- Update `cmd_pipelines()` in `lib.rs` to use `list_pipelines()`
- Update `test_config()` helpers in tests
- Update `forge.yml` to new format

**Phase 3: Tests**
- Unit tests for `pipeline_path()` with single and multiple directories
- Unit tests for shadowing behavior
- Unit tests for missing directory handling
- Unit tests for `list_pipelines()` ordering

## Alternatives Considered

### Alternative 1: Single directory string
- **Description:** `pipelines: pipelines/`
- **Pros:** Simplest possible config
- **Cons:** No multi-directory support; would need a breaking change to add later
- **Why not chosen:** Multi-directory is a near-term need (local vs shared pipelines)

### Alternative 2: Named directories with glob
- **Description:** `pipelines: { pipelines: "*" }`
- **Pros:** Allows per-directory filtering
- **Cons:** More complex for no current use case; glob filter is unlikely to be needed
- **Why not chosen:** YAGNI -- filtering adds complexity with no demonstrated need

### Alternative 3: Keep explicit map, add directory shorthand
- **Description:** Support both `name: path` entries and bare directory strings in a mixed list
- **Pros:** Full backward compatibility
- **Cons:** Two code paths, confusing semantics, serde complexity with untagged enum
- **Why not chosen:** The explicit mapping provides no value over convention; cleaner to just migrate

## Technical Considerations

### Dependencies

No new crate dependencies. Uses `std::fs::read_dir` and existing `shellexpand`.

### Performance

Directory scanning is negligible -- typically 5-20 files per directory, done once at command startup. No caching needed.

### Testing Strategy

- Unit tests with `tempfile` directories containing `.yml` files
- Test first-match-wins shadowing across multiple directories
- Test graceful handling of missing/empty directories
- Test tilde expansion and absolute paths
- Integration: existing pipeline tests continue to pass after `forge.yml` migration

### Rollout Plan

Single commit. Update config parsing, consumer code, `forge.yml`, and tests together. No backward compatibility needed -- forge is pre-release.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Accidental shadowing (two dirs have same-named file) | Low | Medium | `forge pipelines` shows source dir; `log::warn` on shadow |
| Non-`.yml` files in pipeline dirs | Low | Low | Only scan for `.yml` extension |
| Empty pipeline dirs on fresh clone | Medium | Low | Warn but don't error when a directory doesn't exist |

## Design Decisions

- **Shadowed entries:** `forge pipelines` shows only the winning entry. Shadowed pipelines are not displayed -- this matches `$PATH` behavior (you don't see every `ls` binary on your system). Users who need to debug can check the directories directly.
- **Non-existent directories:** Warning, not error. A shared directory (e.g., `~/shared-pipelines/`) may not exist on every machine. Erroring would break portability of `forge.yml` across environments. `log::warn` ensures visibility for debugging.
- **Invalid `.yml` files:** Discovery only reads filenames, not file contents. A malformed pipeline file is only an error when you try to use it (`forge unpack bad-pipeline`), not when listing. This keeps `forge pipelines` resilient.
- **Empty pipelines list:** Valid config. `forge pipelines` shows "No pipelines configured." and `forge unpack <anything>` errors with "unknown pipeline" listing the (empty) search path.
