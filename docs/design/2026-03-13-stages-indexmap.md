# Design Document: Pipeline Stages as IndexMap

**Author:** Scott Idler
**Date:** 2026-03-13
**Status:** Implemented
**Review Passes Completed:** 4/5

## Summary

Convert the pipeline `stages` field from `Vec<Stage>` (list-of-maps with explicit `name`) to `IndexMap<String, Stage>` (map keyed by stage name). The YAML key is backfilled into `Stage.name` via a custom serde `Visitor`, following the established pattern used in `otto` (tasks) and `aka` (aliases). This gives cleaner YAML ergonomics while preserving insertion-order iteration and keeping `stage.name` available throughout the codebase.

## Problem Statement

### Background

Pipeline YAML files define stages as an ordered list where each entry carries a redundant `name` field:

```yaml
stages:
  - name: research
    description: "Gather context"
    pattern: extract_article_wisdom
    review: false
  - name: outline
    description: "Create outline"
    pattern: create_outline
    review: true
```

The `name` field is redundant -- it is the identity of the stage but lives inside the stage body rather than serving as the key.

### Problem

1. **Redundancy** -- The stage name appears as a field inside the map, not as the map key. This is the "list-of-maps with name field" anti-pattern.
2. **YAML noise** -- Every stage definition requires an extra `- name:` line that adds nothing conceptually.
3. **Lookup friction** -- Finding a stage by name requires `.iter().position(|s| s.name == name)` (O(n) scan) instead of direct key lookup.

### Goals

- Replace `Vec<Stage>` with `IndexMap<String, Stage>` in the `Pipeline` struct
- Keep `Stage.name` field but mark it `#[serde(skip_deserializing)]` -- backfilled from map key via custom `Visitor`
- Preserve insertion-order iteration (stages execute sequentially in YAML order)
- Enable O(1) stage lookup by name
- Minimize code churn -- `stage.name` still works everywhere, no access pattern changes needed
- Update all pipeline YAML files to the new map-keyed format
- Keep `PipelineRun.stages: Vec<StageRecord>` unchanged (runtime state, different concern)

### Non-Goals

- Changing the `StageRecord` or `PipelineRun` structs in `store.rs` -- these track runtime execution state with position-based indexing and are a separate concern
- Adding parallel stage execution -- stages remain sequential
- Changing the `.forge/{NN}-{name}.md` file naming convention

## Proposed Solution

### Overview

Add `indexmap` as a dependency, change `Pipeline.stages` from `Vec<Stage>` to `IndexMap<String, Stage>`, and add a custom serde `Visitor` that backfills `Stage.name` from the map key. This follows the established pattern from `otto` (`deserialize_task_map`) and `aka` (`deserialize_alias_map`).

### Data Model

**Before:**
```rust
pub struct Pipeline {
    pub stages: Vec<Stage>,
    // ...
}

pub struct Stage {
    pub name: String,          // redundant -- user must type it in YAML
    pub description: String,
    pub pattern: String,
    pub references: Vec<String>,
    pub review: bool,
}
```

**After:**
```rust
use indexmap::IndexMap;

pub type StageMap = IndexMap<String, Stage>;

pub struct Pipeline {
    #[serde(deserialize_with = "deserialize_stage_map")]
    pub stages: StageMap,
    // ...
}

pub struct Stage {
    #[serde(skip_deserializing)]  // backfilled from map key by custom deserializer
    pub name: String,
    pub description: String,
    pub pattern: String,
    pub references: Vec<String>,
    pub review: bool,
}
```

**Key:** `Stage.name` stays in the struct but is `#[serde(skip_deserializing)]` -- serde ignores it during YAML parsing. The custom `deserialize_stage_map` visitor iterates map entries and sets `stage.name = key` for each one, exactly like otto and aka do.

### Custom Deserializer

Following the otto/aka pattern:

```rust
pub fn deserialize_stage_map<'de, D>(deserializer: D) -> Result<StageMap, D::Error>
where
    D: Deserializer<'de>,
{
    struct StageMapVisitor;
    impl<'de> Visitor<'de> for StageMapVisitor {
        type Value = StageMap;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a map of stage names to stage definitions")
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut stages = StageMap::new();
            while let Some((name, mut stage)) = map.next_entry::<String, Stage>()? {
                stage.name = name.clone();  // backfill name from key
                stages.insert(name, stage);
            }
            Ok(stages)
        }
    }
    deserializer.deserialize_map(StageMapVisitor)
}
```

This is the same `Visitor` + `visit_map` + `next_entry` pattern used in:
- `otto/src/cfg/task.rs:515-541` (`deserialize_task_map`)
- `aka/src/cfg/spec.rs:40-108` (`deserialize_alias_map`)

**YAML format before:**
```yaml
stages:
  - name: research
    description: "Gather context"
    pattern: extract_article_wisdom
    review: false
```

**YAML format after:**
```yaml
stages:
  research:
    description: "Gather context"
    pattern: extract_article_wisdom
    review: false
```

### Dependency

```toml
# Cargo.toml
indexmap = { version = "2", features = ["serde"] }
```

`IndexMap` with the `serde` feature deserializes YAML maps in insertion order. This is well-tested behavior in both `serde_yaml` and `serde_yml`.

### Access Pattern Changes

Because `Stage.name` is preserved, most existing code continues to work unchanged. The main differences are in how the `IndexMap` is iterated vs the previous `Vec`:

#### Pattern 1: Indexed iteration -- unchanged

`stage.name` is still available, so enumerate loops work as before:

```rust
// Before AND after -- identical
for (i, stage) in pipeline.stages.values().enumerate() {
    println!("{}. {} -- {}", i + 1, stage.name, stage.description);
}
```

The only change is `.values()` instead of `.iter()` when you only need stage values (not keys). Or use `.iter()` to get `(name, stage)` tuples if preferred.

#### Pattern 2: Direct index access

**Before:**
```rust
let stage_def = &pipeline.stages[stage_index];
```

**After (using IndexMap positional access):**
```rust
let (_, stage_def) = pipeline.stages.get_index(stage_index)
    .ok_or_else(|| eyre!("stage index {} out of bounds", stage_index))?;
// stage_def.name still works
```

#### Pattern 3: Lookup by name -- improved

**Before:**
```rust
pipeline.stages.iter().position(|s| s.name == name)
    .ok_or_else(|| eyre!("stage '{}' not found", name))
```

**After:**
```rust
pipeline.stages.get_index_of(name)
    .ok_or_else(|| eyre!("stage '{}' not found", name))
```

#### Pattern 4: Value-only iteration (briefcase.rs)

```rust
// Before: for stage in &pipeline.stages { ... }
// After:
for stage in pipeline.stages.values() {
    all_refs.extend(stage.references.clone());
}
```

### Change Impact Summary

| File | Changes | Risk |
|------|---------|------|
| `Cargo.toml` | Add `indexmap` dependency | None |
| `src/pipeline.rs` | Add custom deserializer, `#[serde(skip_deserializing)]` on name, type alias, validate()/all_references_for_stage() iteration | Medium -- new deserializer code |
| `src/executor.rs` | `get_index()` for positional access, `get_index_of()` for name lookup; `stage.name` still works so most code unchanged | Low -- `stage.name` preserved |
| `src/briefcase.rs` | `.values()` / `.keys()` in unpack() only (pack uses StageRecord) | Low |
| `src/lib.rs` | `.values().enumerate()` in cmd_describe(), cmd_refs() | Low |
| `pipelines/*.yml` (5 files) | YAML format migration | Low -- no runtime effect |

### Implementation Plan

#### Phase 1: Add dependency and update structs
- Add `indexmap = { version = "2", features = ["serde"] }` to Cargo.toml
- Add `#[serde(skip_deserializing)]` to `Stage.name`
- Change `Pipeline.stages` from `Vec<Stage>` to `IndexMap<String, Stage>` (type alias `StageMap`)
- Add `#[serde(deserialize_with = "deserialize_stage_map")]` to `Pipeline.stages`

#### Phase 2: Implement custom deserializer in pipeline.rs
- Write `deserialize_stage_map` following the otto/aka `Visitor` + `visit_map` pattern
- Update `Pipeline::validate()` -- iterate over `.values()` for stage validation
- Update `Pipeline::all_references_for_stage()` -- use `get_index()` for positional access
- Update tests and `sample_pipeline_yaml()` to use new YAML format

#### Phase 3: Update executor.rs
- Update `execute_stage()` -- use `get_index()` for positional stage access
- Update `compose_stage_input()` -- use `get_index()` for previous stage lookup
- Update `determine_stage_index()` -- use `get_index_of()` for name lookup
- Update `test_pipeline()` helper -- construct `IndexMap` instead of `Vec`
- Note: `stage.name` still works, so most test assertions are unchanged

#### Phase 4: Update briefcase.rs
- Update `unpack()` -- use `.values()` for reference collection, `.keys()` for stage name collection
- Note: `pack()` / `find_last_stage_output()` use `run.stages` (StageRecord Vec), not `pipeline.stages` -- unchanged

#### Phase 5: Update lib.rs
- Update `cmd_describe()`, `cmd_refs()` -- use `.values().enumerate()` for iteration

#### Phase 6: Update YAML files
- Convert all 5 pipeline YAML files from list format to map-keyed format
- Remove `- name:` entries, promote name to map key

## Alternatives Considered

### Alternative 1: HashMap<String, Stage>
- **Description:** Standard Rust HashMap keyed by stage name
- **Pros:** O(1) lookup, standard library
- **Cons:** No order guarantee -- stages would execute in arbitrary order
- **Why not chosen:** Stage ordering is semantically critical; breaking it would be a correctness bug

### Alternative 2: BTreeMap<String, Stage>
- **Description:** Sorted map keyed by stage name
- **Pros:** Deterministic order, standard library
- **Cons:** Sorts alphabetically, NOT insertion order -- "draft" would come before "research"
- **Why not chosen:** Alphabetical order is wrong; stages must run in the order defined in YAML

### Alternative 3: Vec<(String, Stage)>
- **Description:** Vector of tuples preserving order
- **Pros:** Insertion order, no extra dependency
- **Cons:** O(n) lookup by name, awkward serde (YAML doesn't naturally deserialize to this), no `.get()` method
- **Why not chosen:** Worse ergonomics than IndexMap with no benefits

### Alternative 4: Keep Vec<Stage> with name field (status quo)
- **Description:** Don't change anything
- **Pros:** Zero effort, no migration risk
- **Cons:** Keeps the redundant name field, O(n) lookup, noisier YAML
- **Why not chosen:** The improvement in YAML clarity and code ergonomics justifies the change

## Technical Considerations

### Dependencies

- **indexmap 2.x** -- widely used (200M+ downloads), maintained, serde-compatible. Already a transitive dependency of many Rust crates (serde_yaml itself uses it internally).

### Performance

- IndexMap O(1) amortized insert/lookup vs Vec O(n) scan for name lookup
- No measurable difference for the stage counts in practice (2-5 stages per pipeline)
- The change is motivated by ergonomics, not performance

### Serialization Roundtrip

`IndexMap` with `features = ["serde"]` serializes as a YAML map and deserializes preserving insertion order. Roundtrip: YAML → IndexMap → YAML produces identical key order.

### Testing Strategy

- Update all existing unit tests in `pipeline.rs`, `executor.rs`, `briefcase.rs`, `lib.rs`
- Verify YAML deserialization preserves stage order (existing `test_load_pipeline` covers this)
- Verify `get_index()`, `get_index_of()` return correct positions
- Ensure `cargo test` passes with zero regressions

### Migration

No runtime migration needed. This is a YAML format change -- existing `.forge/` directories use `StageRecord` (Vec-based, unchanged). Only the pipeline definition files change format, and those are checked into the repo.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| serde_yaml stops preserving map order | Very Low | High | IndexMap order preservation is a documented, tested contract; serde_yaml uses IndexMap internally |
| Duplicate stage names in YAML silently overwrite | Low | Medium | Add validation in `Pipeline::validate()` -- but YAML maps inherently reject duplicate keys (serde_yaml errors on them) |
| `get_index()` returns `Option` where `[]` panicked | Low | Low | Safety improvement -- forces explicit error handling |
| Empty string as YAML map key | Very Low | Medium | `Pipeline::validate()` already checks for empty stage names; iterate keys instead of `stage.name` |
| Test construction verbosity | Certain | None | Use `IndexMap::from([("name".into(), Stage { .. }), ...])` -- slightly more verbose but clear |

## Open Questions

None -- all questions resolved during design.

## References

- [indexmap crate](https://docs.rs/indexmap)
- [serde_yaml map ordering](https://docs.rs/serde_yaml) -- uses IndexMap internally
- `otto/src/cfg/task.rs:515-541` -- `deserialize_task_map` (same pattern, HashMap)
- `aka/src/cfg/spec.rs:40-108` -- `deserialize_alias_map` (same pattern, HashMap)
- Both use `#[serde(skip_deserializing)]` on `name` + custom `Visitor` with `visit_map`
