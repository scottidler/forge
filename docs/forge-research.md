# Forge Design Research -- Dependency Exploration

Gathered 2026-03-12 as input for the forge design document.

---

## 1. TaskStore (`~/repos/scottidler/taskstore`)

### Data Model -- Record Trait

All storable types implement the `Record` trait:

```rust
trait Record {
    fn id(&self) -> &str;                           // unique identifier
    fn updated_at(&self) -> i64;                    // millis since epoch (for sync/merge)
    fn collection_name() -> &'static str;           // determines JSONL filename (e.g. "tasks" → tasks.jsonl)
    fn indexed_fields(&self) -> HashMap<String, IndexValue>;  // fields queryable via SQLite
}

enum IndexValue {
    String(String),
    Int(i64),
    Bool(bool),
}
```

JSONL format: one JSON object per line, must have `id` and `updated_at`. Tombstones: `{"id": "...", "deleted": true, "updated_at": ...}`. Multiple versions of same ID can exist; sync keeps the latest (`updated_at` wins).

### SQLite Index Architecture

```sql
-- Main records table (generic, holds any JSON)
CREATE TABLE records (
    collection TEXT NOT NULL,
    id TEXT NOT NULL,
    data_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (collection, id)
);

CREATE INDEX idx_records_collection ON records(collection);
CREATE INDEX idx_records_updated_at ON records(collection, updated_at);

-- Separate indexes table for filtering
CREATE TABLE record_indexes (
    collection TEXT NOT NULL,
    id TEXT NOT NULL,
    field_name TEXT NOT NULL,
    field_value_str TEXT,
    field_value_int INTEGER,
    field_value_bool INTEGER,
    PRIMARY KEY (collection, id, field_name)
);

CREATE INDEX idx_record_indexes_field_str ON record_indexes(collection, field_name, field_value_str);
CREATE INDEX idx_record_indexes_field_int ON record_indexes(collection, field_name, field_value_int);
CREATE INDEX idx_record_indexes_field_bool ON record_indexes(collection, field_name, field_value_bool);

-- Staleness detection
CREATE TABLE sync_metadata (
    collection TEXT PRIMARY KEY,
    last_sync_time INTEGER NOT NULL,
    file_mtime INTEGER NOT NULL
);
```

Query strategy:
- No-filter: simple SELECT ordered by `updated_at DESC`
- Filtered: JOIN with `record_indexes` using EXISTS subqueries
- Operators: `=`, `!=`, `>`, `<`, `>=`, `<=`, `LIKE` (Contains)
- Multiple filters: AND logic (intersection)

### Store API (Library Interface)

```rust
impl Store {
    // Lifecycle
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self>
    pub fn base_path(&self) -> &Path
    pub fn db(&self) -> &Connection  // Direct SQLite access

    // CRUD (generic over Record type)
    pub fn create<T: Record>(&mut self, record: T) -> Result<String>
    pub fn get<T: Record>(&self, id: &str) -> Result<Option<T>>
    pub fn update<T: Record>(&mut self, record: T) -> Result<()>
    pub fn delete<T: Record>(&mut self, id: &str) -> Result<()>
    pub fn delete_by_index<T: Record>(&mut self, field: &str, value: IndexValue) -> Result<usize>

    // Querying
    pub fn list<T: Record>(&self, filters: &[Filter]) -> Result<Vec<T>>

    // Sync & Rebuild
    pub fn sync(&mut self) -> Result<()>
    pub fn rebuild_indexes<T: Record>(&mut self) -> Result<usize>
    pub fn is_stale(&self) -> Result<bool>

    // Git Integration
    pub fn install_git_hooks(&self) -> Result<()>
}

pub fn now_ms() -> i64  // Current time in milliseconds
```

### Filter API

```rust
pub struct Filter {
    pub field: String,
    pub op: FilterOp,
    pub value: IndexValue,
}

pub enum FilterOp {
    Eq, Ne, Gt, Lt, Gte, Lte, Contains
}

// Usage:
let results: Vec<MyRecord> = store.list(&[
    Filter {
        field: "status".to_string(),
        op: FilterOp::Eq,
        value: IndexValue::String("active".to_string()),
    },
])?;
```

### CLI Commands

```bash
taskstore [--store-path <PATH>] <COMMAND>

Commands:
  sync              Sync SQLite from JSONL files
  install-hooks     Install git hooks for automatic syncing
  collections       List all collections
  list              List records (--filter field=value, --limit N)
  get               Get record by ID
  indexes           Show indexes for a collection
  sql               Run raw SQL query (read-only)
```

### Cargo.toml

- **Version**: 0.2.1 (edition 2024)
- **Key deps**: rusqlite 0.38.0 (bundled), serde_json 1.0.149, serde 1.0.228, clap 4.5.54, uuid 1.19.0 (v7), chrono 0.4, fs2 0.4, eyre 0.6.12, tracing 0.1.44, colored 3.0.0, dirs 6.0.0

### Git Integration -- Custom Merge Driver

Binary: `taskstore-merge` (installed via `.gitattributes`):

```
.taskstore/*.jsonl merge=taskstore-merge
```

Three-way merge logic:
- Parses ancestor/ours/theirs JSONL files
- Builds ID → latest-record maps by `updated_at`
- Added in one branch → use that version
- Deleted in one branch → keep deletion
- Modified in both → newer `updated_at` wins
- Same timestamp → conflict markers

Git hooks (installed by `install_git_hooks()`): pre-commit, post-merge, post-rebase, pre-push, post-checkout -- all run `taskstore sync`.

### File Structure on Disk

```
.taskstore/
├── taskstore.db           # SQLite database (queryable, ephemeral)
├── taskstore.db-shm       # SQLite shared memory (temp)
├── taskstore.db-wal       # SQLite WAL (temp)
├── .gitignore             # Auto-created, ignores db files
├── .version               # Schema version
├── tasks.jsonl            # Source of truth
├── plans.jsonl
├── notes.jsonl
└── [collection].jsonl     # One file per collection
```

Git tracks only `.jsonl` files. SQLite DB is ephemeral/rebuildable.

### Validation Rules

- Collection names: non-empty, max 64 chars, alphanumeric + underscore + hyphen
- Field names: non-empty, max 64 chars, alphanumeric + underscore only
- Record IDs: non-empty, max 256 chars, trimmed

---

## 2. Otto (`~/repos/scottidler/otto`)

### .otto.yml Schema

Two-section YAML:

```yaml
otto:
  api: "1"                    # API version (required)
  name: "otto"                # Project name
  about: "A task runner"      # Description
  jobs: 4                     # Parallel job count (default: CPU count)
  home: ~/.otto               # Otto home directory
  tasks: [default_task]       # Default tasks (default: ["*"])
  verbosity: 1                # Logging verbosity
  envs:                       # Global env vars (supports shell expansion: $(cmd), ${VAR})
    KEY: "value"

tasks:
  task_name:
    help: "Task description"
    params:                    # Task parameters/flags
    before: [dep_task]        # Must run first
    after: [dependent_task]   # Depends on this
    input: [glob/paths]       # Input file dependencies
    output: [paths]           # Output file tracking
    envs: {}                  # Task-specific env vars
    bash: |                   # Bash script (or python:, action:)
      script content
    foreach:                  # Subtask generation
      items: [...]           # Explicit list
      # OR
      glob: "pattern/*.sh"    # File glob
      # OR
      range: "1-10"           # Numeric range
      as: variable_name       # Variable name (default: "item")
      parallel: true          # Run subtasks in parallel (default: true)
      max_items: 1000         # Safety limit
```

### Parameter Definition

```yaml
params:
  -v|--verbose:              # Short|long flags
    default: false
    help: "Enable verbose"
    choices: [val1, val2]    # Restrict values
    metavar: VAR             # Display name
    dest: env_var_name       # Output env var

  input_file:                # Positional (no dash prefix)
    help: "Input file"
    metavar: FILE
    default: "default.txt"
```

### CLI Structure

```
otto [OPTIONS] [TASKS]... [TASK_ARGS]...

Options:
  -o, --ottofile <PATH>      Ottofile path/directory (env: OTTO_OTTOFILE)
  -j, --jobs <NUM>           Parallel jobs (env: OTTO_JOBS)
  --tui                      Terminal UI mode
  --list-subtasks            List all tasks including foreach subtasks

Subcommands:
  Clean     Clean up run artifacts
  Convert   Convert Makefile to otto.yml
  History   View execution history
  Stats     Show task statistics
  Upgrade   Upgrade otto
```

### File Discovery

Searches for ottofiles in order: `otto.yml`, `.otto.yml`, `otto.yaml`, `.otto.yaml`, `Ottofile`, `OTTOFILE`. Walks up directory tree toward root.

### Task Execution Flow

1. **Parse**: Extract ottofile, resolve defaults, parse task arguments
2. **Expansion**: Expand foreach tasks into concrete subtasks
3. **Dependency**: Build task DAG from `before`, `after`, file/output dependencies
4. **Filtering**: Transitively collect all dependencies of requested tasks
5. **Execution**: Run respecting parallelism (`--jobs`) and dependency order

### Foreach Expansion

- Non-foreach tasks pass through unchanged
- Foreach tasks create virtual parent (no action) + N concrete subtasks
- Subtasks named `parent:identifier` (e.g., `deploy:staging`)
- Virtual parent inherits `after`/`before` relationships
- Auto-provides: `${variable_name}`, `$OTTO_FOREACH_ITEM`, `$OTTO_FOREACH_INDEX`

### Built-in Task Functions

```bash
otto_set_output "key" "value"     # Pass data to dependents
otto_get_input "task.key"         # Retrieve data from dependencies
```

### Environment in Task Context

- All global + task-level env vars
- Parameter values as `${param_name}` (hyphens → underscores)
- Otto-provided colors: `$RED`, `$GREEN`, `$YELLOW`, `$BLUE`, `$CYAN`, `$MAGENTA`, `$BOLD`, `$DIM`, `$NC`
- `$OTTO_TASK_DIR` (scratch space)

### Cargo.toml

- **Key deps**: tokio 1.48, serde_yaml 0.9, clap 4.5, daggy 0.9 (DAG graph), sha2 0.10, eyre 0.6, glob 0.3, regex 1.12, colored 3.0, ratatui 0.29, rusqlite 0.37

### Key Source Files

| File | Purpose |
|------|---------|
| `src/cli/parser.rs` | Main CLI parser |
| `src/cfg/task.rs` | Task definitions |
| `src/cfg/param.rs` | Parameters |
| `src/cfg/config.rs` | Config loading |
| `src/cfg/otto.rs` | Otto section |
| `src/executor/task.rs` | Task execution |

---

## 3. obsidian-borg (`~/repos/scottidler/obsidian-borg`)

### Pipeline Stages (7-stage sequential flow in `src/pipeline.rs`)

```
process_url_inner()
  ├─ Stage 1: URL Normalization
  │  └─ hygiene::normalize_url() → clean_url() then canonicalize_url()
  │     • Strips UTM/tracking params (utm_*, fbclid, gclid, etc.)
  │     • Config-driven canonicalization rules (regex-based)
  │     • Examples: youtu.be/ID → youtube.com/watch?v=ID
  │
  ├─ Stage 2: Deduplication
  │  └─ Two-tier dedup:
  │     • In-memory: INFLIGHT<Mutex<HashSet<String>>> prevents races
  │     • Ledger: reads Borg Log markdown table for ✅ entries
  │     • --force bypasses both
  │
  ├─ Stage 3: URL Classification
  │  └─ router::classify_url() matches against LinkConfig patterns
  │     • Output: UrlMatch { url, link_name, folder, width, height }
  │     • Built-in defaults: "shorts", "youtube", "default"
  │
  ├─ Stage 4: Content Fetching & Summarization
  │  └─ YouTube path:
  │     • fabric -y <url> --metadata, then --transcript
  │     • Fallback: yt-dlp → transcriber (Groq whisper)
  │     • Summarize via fabric pattern
  │  └─ Article path:
  │     • fabric -u <url>
  │     • Fallback 1: markitdown-cli
  │     • Fallback 2: jina::fetch_article_markdown()
  │     • Summarize via fabric pattern
  │
  ├─ Stage 5: Tag Generation
  │  └─ fabric::generate_tags(summary) → tag_pattern
  │     • Runs "create_tags" pattern, sanitizes, merges with config tags
  │
  ├─ Stage 6: Destination Routing (3-tier)
  │  └─ Tier 1: URL-type routing from LinkConfig.folder
  │  └─ Tier 2: LLM topic classification (confidence_threshold 0.6)
  │  └─ Tier 3: Fallback to config routing.fallback_folder
  │
  ├─ Stage 7: Note Rendering & Writing
  │  └─ markdown::render_note(NoteContent) → YAML frontmatter + body
  │     • Frontmatter: title, date, day, time, source, type, method, tags, author, etc.
  │     • Write to vault path
  │
  └─ Stage 8: Ledger Entry (on success OR failure)
     └─ ledger::append_entry() → Borg Log.md markdown table
        • Fields: Date, Time, Method, Status (✅/❌/⏭️), Title, Source, etc.
        • fs2 file locking for concurrent safety
```

### Config Loading Chain

```
1. Explicit --config path (CLI arg)
2. ~/.config/obsidian-borg/obsidian-borg.yml
3. ./obsidian-borg.yml (current dir)
4. Default (hardcoded fallbacks)
```

### Config Structure

```rust
pub struct Config {
    pub server: ServerConfig,               // host, port
    pub vault: VaultConfig,                 // root_path, inbox_path (support ~/)
    pub transcriber: TranscriberConfig,     // URL, timeout_secs
    pub groq: GroqConfig,                   // api_key, model (whisper-large-v3)
    pub llm: LlmConfig,                     // provider, model, api_key
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
    pub ntfy: Option<NtfyConfig>,
    pub links: Vec<LinkConfig>,             // regex patterns → folders
    pub fabric: FabricConfig,               // binary, model, patterns, max_content_chars
    pub frontmatter: FrontmatterConfig,     // default_tags, default_author, timezone
    pub routing: RoutingConfig,             // confidence_threshold, fallback_folder
    pub hotkey: HotkeyConfig,
    pub canonicalization: CanonicalConfig,   // URL normalization rules
    pub migration: MigrationConfig,
    pub log_level: Option<String>,
    pub debug: bool,
}
```

### Fabric Config

```rust
pub struct FabricConfig {
    pub binary: String,                       // default: "fabric"
    pub model: String,                        // empty = fabric's default
    pub summarize_pattern_youtube: String,    // default: "youtube_summary"
    pub summarize_pattern_article: String,    // default: "extract_article_wisdom"
    pub tag_pattern: String,                  // default: "create_tags"
    pub classify_pattern: String,             // default: "obsidian_classify"
    pub max_content_chars: usize,             // default: 30000
}
```

### Secret Resolution

```rust
fn resolve_secret(value: &str) -> Result<String> {
    // If file exists at path → read contents (trimmed)
    // Otherwise → resolve as env var name
}
```

### Note Output Format

```markdown
---
title: "Note Title"
date: YYYY-MM-DD
day: DayOfWeek
time: "HH:MM"
source: "https://canonical-url"
type: youtube | article
method: telegram | discord | http | clipboard | cli
tags:
  - tag1
  - tag2
author: "Author Name"
uploader: "Channel Name"     # YouTube only
duration_min: 45              # YouTube only
---

# Note Title

<iframe ...></iframe>         <!-- YouTube only -->

## Summary

[Summary text]

---

*Source: [url](url)*
```

### Error Handling & Resumability

- Graceful fallbacks at each stage (Fabric → jina, yt-dlp → transcriber)
- No resume mechanism (URLs are stateless, atomic processing)
- Idempotency via ledger dedup
- Failures logged to Borg Log with ❌ status

### Cargo.toml

- **Version**: 0.3.4 (Edition 2024)
- **Key deps**: tokio 1 (full), axum 0.8.8, serde/serde_yaml, clap 4.5.60, chrono 0.4.44, reqwest 0.13.2, teloxide 0.17, serenity 0.12, eyre 0.6.12, regex 1, url 2.5.8, fs2 0.4.3, shellexpand 3.1.2, colored 3.1.1, dirs 6.0.0

---

## 4. Scaffold (`~/repos/scottidler/scaffold`)

### Invocation

```bash
scaffold <project-name>
```

Flags: `--author`, `--directory`, `--config`, `--no-git`, `--no-sample-config`, `--no-verify`, `--no-deps`

### Generated Project Structure

```
<project-name>/
├── Cargo.toml              # Edition 2024, build.rs enabled
├── build.rs                # git describe versioning
├── src/
│   ├── main.rs             # Logging setup, error handling
│   ├── lib.rs              # Core logic
│   ├── cli.rs              # Clap derive structs
│   └── config.rs           # YAML config loading with fallback chain
├── <project-name>.yml      # Sample config file
├── .otto.yml               # CI/CD tasks
└── .git/                   # Git repository initialized
```

### Default Dependencies

- **clap** (derive) -- CLI parsing
- **eyre** -- Error handling
- **log** + **env_logger** -- Logging to file
- **serde** + **serde_yaml** (derive) -- YAML config
- **dirs** -- Platform-aware directories
- **colored** -- Terminal colors

### Generated Source Files

**src/cli.rs**:
```rust
#[derive(Parser)]
#[command(
    name = "{project}",
    version = env!("GIT_DESCRIBE"),
    after_help = "Logs are written to: ~/.local/share/{project}/logs/{project}.log"
)]
pub struct Cli {
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    #[arg(short, long)]
    pub verbose: bool,
}
```

**src/config.rs** -- YAML config with fallback chain:
- Primary: `~/.config/{project}/{project}.yml`
- Fallback: `./{project}.yml` (local)
- Ultimate fallback: hardcoded defaults

**build.rs** -- `git describe --tags --always` → `GIT_DESCRIBE` env var

### .otto.yml CI/CD Tasks

| Task | Purpose |
|------|---------|
| `lint` | Whitespace linting |
| `check` | cargo check + clippy + fmt |
| `test` | cargo test --all-features |
| `cov` | Coverage via cargo llvm-cov |
| `ci` | Full pipeline: lint + check + test |
| `build` | Release build |
| `clean` | Clean artifacts |
| `install` | Install to ~/.cargo/bin |

### Scott's Rust CLI Conventions (from SKILL.md)

1. **Thin Shell Pattern**: `main.rs` parses args + prints; core logic in `lib.rs`
2. **Return Data, Not Effects**: Core returns `Result<T>`, never `process::exit()`
3. **Clap Two-Stage**: Parse CLI args → Validate + apply defaults in Config
4. **Dependency Injection**: Use generics + trait bounds, never `dyn` traits
5. **Testing Strategy**: Prefer unit tests with fakes over E2E; mock I/O via injected deps; aim for 90-100% coverage
6. **Error handling**: `eyre::Result` with `.context()`
7. **Logging**: Output to `~/.local/share/{project}/logs/{project}.log`
8. **Config location**: `~/.config/{project}/{project}.yml`
9. **No `.unwrap()` in production code**
10. **Terminal output**: Detect TTY vs piped with `IsTerminal`; YAML for humans, JSON for machines
11. **Async**: `tokio` for I/O-bound, `rayon` for CPU-bound

---

## 5. Fabric CLI Integration

### Installation & Location

- Binary: `/home/saidler/.local/bin/fabric` (in PATH)
- Patterns directory: `~/.config/fabric/patterns/` (managed by fabric CLI)
- Patterns may need initial download: `fabric -U`

### Invocation Pattern

```bash
# Basic: stdin → pattern → stdout
echo "input text" | fabric -p PATTERN_NAME

# From file
fabric -p PATTERN_NAME < input_file.txt

# With model selection
fabric -p summarize -m "Anthropic|claude-3-5-haiku-20241022"

# With variables
fabric -p PATTERN_NAME -v=#role:expert -v=#points:30

# With streaming
fabric -p summarize -s

# URL extraction (built-in)
fabric -u https://example.com -p analyze_claims

# YouTube extraction (built-in)
fabric -y "https://youtube.com/watch?v=xyz" -p extract_wisdom

# Copy to clipboard
fabric -p summarize -c
```

### How obsidian-borg Calls Fabric (from `src/fabric.rs`)

```rust
pub async fn run_pattern(pattern: &str, input: &str, config: &FabricConfig) -> Result<String> {
    let binary = resolve_binary(config);  // "which fabric" if not absolute
    let mut cmd = Command::new(&binary);
    cmd.args(["-p", pattern]);
    if !config.model.is_empty() {
        cmd.args(["-m", &config.model]);
    }
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Write input to stdin, wait for output, return stdout
}
```

### Pattern Usage in obsidian-borg

| Function | Pattern | Input | Output |
|----------|---------|-------|--------|
| `summarize()` | `summarize_pattern_youtube` / `summarize_pattern_article` | Article/transcript | Summary markdown |
| `generate_tags()` | `tag_pattern` | Summary text | Space-separated tags |
| `classify_topic()` | `classify_pattern` | Title + summary | JSON: `{folder, confidence, suggested_tags}` |
| `fetch_youtube()` | N/A (uses `-y` flag) | URL | YouTubeContent struct |
| `fetch_article()` | N/A (uses `-u` flag) | URL | Markdown string |

### Binary Resolution

```rust
pub fn resolve_binary(config: &FabricConfig) -> String {
    if binary.starts_with('/') || binary.starts_with("./") {
        return binary.clone();
    }
    // Try `which fabric` to resolve from PATH
    if let Ok(output) = Command::new("which").arg(binary).output() {
        return resolved_path;
    }
    binary.clone()
}
```

### Availability Check

```rust
pub fn is_available(config: &FabricConfig) -> bool {
    Command::new(&binary)
        .arg("--version")
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
```
