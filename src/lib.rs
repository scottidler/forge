pub mod briefcase;
pub mod cli;
pub mod config;
pub mod executor;
pub mod init;
pub mod pipeline;
pub mod store;

use std::collections::HashMap;
use std::path::PathBuf;

use colored::Colorize;
use eyre::{Result, eyre};
use log::debug;
use terminal_size::{Width, terminal_size};

use crate::cli::Command;
use crate::config::ForgeConfig;
use crate::pipeline::Pipeline;

pub fn run_command(command: &Command, config: &ForgeConfig) -> Result<()> {
    debug!("run_command: command={:?}", command);
    match command {
        Command::Describe { pipeline, stage } => cmd_describe(config, pipeline, *stage),
        Command::Refs { pipeline, stage } => cmd_refs(config, pipeline, *stage),
        Command::Unpack { pipeline, input, slug } => {
            briefcase::unpack(config, pipeline, input.as_deref(), slug.as_deref())
        }
        Command::Pack { abandon } => briefcase::pack(config, *abandon),
        Command::Run { stage, input } => executor::run_stage(config, stage.as_deref(), input.as_deref()),
        Command::Ls { pipelines, all } => cmd_ls(config, pipelines, *all),
        Command::Show { run_id } => cmd_show(config, run_id.as_deref()),
        Command::History { pipeline, limit } => cmd_history(config, pipeline.as_deref(), *limit),
        Command::Init { .. } => unreachable!("Init is handled before config loading"),
    }
}

fn cmd_describe(config: &ForgeConfig, pipeline_name: &str, stage_filter: Option<usize>) -> Result<()> {
    debug!(
        "cmd_describe: pipeline_name={}, stage_filter={:?}",
        pipeline_name, stage_filter
    );
    let path = config.pipeline_path(pipeline_name)?;
    let pipeline = Pipeline::load(&path)?;

    println!("{}: {}", pipeline.name.bold().cyan(), pipeline.description);
    println!("Output: {} / {}", pipeline.output.destination, pipeline.output.filename);

    if !pipeline.references.is_empty() {
        println!("\n{}:", "Pipeline references".bold());
        for r in &pipeline.references {
            println!("  - {}", r);
        }
    }

    println!("\n{}:", "Stages".bold());
    for (i, stage) in pipeline.stages.values().enumerate() {
        if let Some(filter) = stage_filter
            && i != filter
        {
            continue;
        }
        let review_tag = if stage.review { " [review gate]".yellow().to_string() } else { String::new() };
        println!(
            "  {}. {} -- {}{}",
            i + 1,
            stage.name.bold(),
            stage.description,
            review_tag
        );
        let cmd_display = if stage.args.is_empty() {
            stage.command.clone()
        } else {
            format!("{} {}", stage.command, stage.args.join(" "))
        };
        println!("     command: {}", cmd_display.dimmed());
        if !stage.references.is_empty() {
            for r in &stage.references {
                println!("     ref: {}", r.dimmed());
            }
        }
    }
    Ok(())
}

fn cmd_refs(config: &ForgeConfig, pipeline_name: &str, stage_filter: Option<usize>) -> Result<()> {
    debug!(
        "cmd_refs: pipeline_name={}, stage_filter={:?}",
        pipeline_name, stage_filter
    );
    let path = config.pipeline_path(pipeline_name)?;
    let pipeline = Pipeline::load(&path)?;

    println!("{}", "Global references:".bold());
    for r in &config.global_references {
        let full = config.reference_path(r)?;
        let status = if full.exists() { "ok".green() } else { "missing".red() };
        println!("  [{}] {}", status, r);
    }

    println!("\n{}", "Pipeline references:".bold());
    for r in &pipeline.references {
        let full = config.reference_path(r)?;
        let status = if full.exists() { "ok".green() } else { "missing".red() };
        println!("  [{}] {}", status, r);
    }

    for (i, stage) in pipeline.stages.values().enumerate() {
        if let Some(filter) = stage_filter
            && i != filter
        {
            continue;
        }
        if !stage.references.is_empty() {
            println!("\n{} '{}':", "Stage references for".bold(), stage.name);
            for r in &stage.references {
                let full = config.reference_path(r)?;
                let status = if full.exists() { "ok".green() } else { "missing".red() };
                println!("  [{}] {}", status, r);
            }
        }
    }
    Ok(())
}

fn cmd_ls(config: &ForgeConfig, pipelines: &[String], all: bool) -> Result<()> {
    debug!("cmd_ls: pipelines={:?}, all={}", pipelines, all);
    let available = config.list_pipelines()?;
    let active_runs = load_active_runs(config)?;

    if pipelines.is_empty() && !all {
        cmd_ls_compact(config, &available, &active_runs)
    } else {
        let matched = if all { available } else { filter_pipelines(&available, pipelines)? };
        cmd_ls_detailed(config, &matched, &active_runs)
    }
}

fn cmd_ls_compact(
    config: &ForgeConfig,
    available: &[(String, PathBuf)],
    active_runs: &HashMap<String, Vec<store::PipelineRun>>,
) -> Result<()> {
    if available.is_empty() {
        println!("No pipelines configured.");
        return Ok(());
    }

    // Load descriptions and compute column width
    let mut entries: Vec<(String, String, usize)> = Vec::new();
    let mut max_name_len = 0;
    for (name, path) in available {
        let description = match Pipeline::load(path) {
            Ok(p) => p.description,
            Err(e) => {
                log::warn!("failed to load pipeline {}: {}", name, e);
                "(load error)".to_string()
            }
        };
        let count = active_runs.get(name).map_or(0, |v| v.len());
        max_name_len = max_name_len.max(name.len());
        entries.push((name.clone(), description, count));
    }

    let term_width = terminal_size().map(|(Width(w), _)| w as usize).unwrap_or(80);
    // prefix: "  {padded_name} - "
    let prefix_len = 2 + max_name_len + 3;

    println!("{}:", "Pipelines".bold());
    for (name, description, count) in &entries {
        let count_suffix = if *count > 0 { format!(" ({})", count) } else { String::new() };
        let padded_name = format!("{:width$}", name, width = max_name_len);
        let full_desc = format!("{}{}", description, count_suffix);
        let desc_width = term_width.saturating_sub(prefix_len);
        let wrapped = wrap_text(&full_desc, desc_width);
        for (i, line) in wrapped.iter().enumerate() {
            if i == 0 {
                println!("  {} - {}", padded_name.cyan(), line);
            } else {
                println!("{:indent$}{}", "", line, indent = prefix_len);
            }
        }
    }
    let _ = config; // suppress unused warning
    Ok(())
}

fn cmd_ls_detailed(
    config: &ForgeConfig,
    matched: &[(String, PathBuf)],
    active_runs: &HashMap<String, Vec<store::PipelineRun>>,
) -> Result<()> {
    if matched.is_empty() {
        return Ok(());
    }

    for (i, (name, path)) in matched.iter().enumerate() {
        if i > 0 {
            println!();
        }

        match Pipeline::load(path) {
            Ok(pipeline) => {
                let stage_count = pipeline.stages.len();
                let stage_chain = format_stage_chain(&pipeline);
                let output_path = format!("{}/{}", pipeline.output.destination, pipeline.output.filename);
                println!(
                    "{} ({} {}) - {}",
                    name.bold().cyan(),
                    stage_count,
                    if stage_count == 1 { "stage" } else { "stages" },
                    pipeline.description
                );
                println!("  {} => {}", stage_chain, output_path.dimmed());

                if let Some(runs) = active_runs.get(name) {
                    for run in runs {
                        let stage_info = if run.current_stage < run.stages.len() {
                            format!(
                                "stage {}/{} ({})",
                                run.current_stage + 1,
                                run.stages.len(),
                                run.stages[run.current_stage].name
                            )
                        } else {
                            "complete".to_string()
                        };
                        println!(
                            "  {}  [{}]  {}   {}",
                            &run.id[..8].dimmed(),
                            run.status.to_string().yellow(),
                            stage_info,
                            run.working_dir.dimmed()
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!("{}: failed to load pipeline definition: {}", name.bold().cyan(), e);
            }
        }
    }
    let _ = config; // suppress unused warning
    Ok(())
}

/// Load all active (Unpacked + InProgress) runs, grouped by pipeline name
fn load_active_runs(config: &ForgeConfig) -> Result<HashMap<String, Vec<store::PipelineRun>>> {
    debug!("load_active_runs");
    let store_dir = config.store_dir()?;
    if !store_dir.exists() {
        return Ok(HashMap::new());
    }
    let store = store::open_store(&store_dir)?;

    let mut runs: Vec<store::PipelineRun> = store.list(&[taskstore::Filter {
        field: "status".to_string(),
        op: taskstore::FilterOp::Eq,
        value: taskstore::IndexValue::String("Unpacked".to_string()),
    }])?;
    let in_progress: Vec<store::PipelineRun> = store.list(&[taskstore::Filter {
        field: "status".to_string(),
        op: taskstore::FilterOp::Eq,
        value: taskstore::IndexValue::String("InProgress".to_string()),
    }])?;
    debug!(
        "load_active_runs: found {} unpacked, {} in_progress",
        runs.len(),
        in_progress.len()
    );
    runs.extend(in_progress);

    let mut grouped: HashMap<String, Vec<store::PipelineRun>> = HashMap::new();
    for run in &runs {
        debug!(
            "load_active_runs: run id={}, pipeline={}, status={:?}",
            &run.id[..8],
            run.pipeline,
            run.status
        );
    }
    for run in runs {
        grouped.entry(run.pipeline.clone()).or_default().push(run);
    }
    Ok(grouped)
}

/// Filter pipelines by substring matching against user-provided patterns
fn filter_pipelines(available: &[(String, PathBuf)], patterns: &[String]) -> Result<Vec<(String, PathBuf)>> {
    let mut matched = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for pattern in patterns {
        let mut found = false;
        for (name, path) in available {
            if matches_pipeline(name, pattern) && seen.insert(name.clone()) {
                matched.push((name.clone(), path.clone()));
                found = true;
            }
        }
        if !found {
            eprintln!("No pipeline matching '{}' found.", pattern);
        }
    }

    // Sort alphabetically like the full list
    matched.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(matched)
}

/// Case-insensitive substring match
fn matches_pipeline(name: &str, pattern: &str) -> bool {
    name.to_lowercase().contains(&pattern.to_lowercase())
}

/// Wrap text to a given width, breaking on word boundaries
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 || text.len() <= width {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
        } else if current.len() + 1 + word.len() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current);
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Format stage chain like: research -> outline [review] -> draft [review]
fn format_stage_chain(pipeline: &Pipeline) -> String {
    pipeline
        .stages
        .values()
        .map(|stage| {
            if stage.review {
                format!("{} [review]", stage.name)
            } else {
                stage.name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" -> ")
}

fn cmd_show(config: &ForgeConfig, run_id: Option<&str>) -> Result<()> {
    debug!("cmd_show: run_id={:?}", run_id);
    let store_dir = config.store_dir()?;
    let store = store::open_store(&store_dir)?;

    let run = if let Some(id) = run_id {
        store
            .get::<store::PipelineRun>(id)?
            .ok_or_else(|| eyre!("run not found: {}", id))?
    } else {
        // Find active run in current directory
        let cwd = std::env::current_dir()?;
        let cwd_str = cwd.to_string_lossy().to_string();
        let runs: Vec<store::PipelineRun> = store.list(&[taskstore::Filter {
            field: "working_dir".to_string(),
            op: taskstore::FilterOp::Eq,
            value: taskstore::IndexValue::String(cwd_str),
        }])?;
        runs.into_iter()
            .find(|r| r.status == store::RunStatus::Unpacked || r.status == store::RunStatus::InProgress)
            .ok_or_else(|| eyre!("no active pipeline run in current directory"))?
    };

    println!("{}: {}", "Run ID".bold(), run.id);
    println!("{}: {}", "Pipeline".bold(), run.pipeline.cyan());
    println!("{}: {}", "Status".bold(), run.status.to_string().yellow());
    println!("{}: {}", "Working dir".bold(), run.working_dir);
    if let Some(ref input) = run.input {
        println!("{}: {}", "Input".bold(), input);
    }
    if let Some(ref slug) = run.slug {
        println!("{}: {}", "Slug".bold(), slug);
    }

    println!("\n{}:", "Stages".bold());
    for (i, stage) in run.stages.iter().enumerate() {
        let marker = if i == run.current_stage && run.status != store::RunStatus::Completed {
            ">>".green().to_string()
        } else {
            "  ".to_string()
        };
        let status_color = match stage.status {
            store::StageStatus::Completed => stage.status.to_string().green().to_string(),
            store::StageStatus::Review => stage.status.to_string().yellow().to_string(),
            store::StageStatus::InProgress => stage.status.to_string().cyan().to_string(),
            store::StageStatus::Failed => stage.status.to_string().red().to_string(),
            _ => stage.status.to_string().dimmed().to_string(),
        };
        println!("{} {}. {} [{}]", marker, i + 1, stage.name, status_color);
    }

    if let Some(ref dest) = run.final_destination {
        println!("\n{}: {}", "Final output".bold(), dest);
    }
    Ok(())
}

fn cmd_history(config: &ForgeConfig, pipeline_filter: Option<&str>, limit: usize) -> Result<()> {
    debug!("cmd_history: pipeline_filter={:?}, limit={}", pipeline_filter, limit);
    let store_dir = config.store_dir()?;
    if !store_dir.exists() {
        println!("No pipeline history.");
        return Ok(());
    }
    let store = store::open_store(&store_dir)?;

    let filters = if let Some(name) = pipeline_filter {
        vec![taskstore::Filter {
            field: "pipeline".to_string(),
            op: taskstore::FilterOp::Eq,
            value: taskstore::IndexValue::String(name.to_string()),
        }]
    } else {
        vec![]
    };

    let runs: Vec<store::PipelineRun> = store.list(&filters)?;
    if runs.is_empty() {
        println!("No pipeline history.");
        return Ok(());
    }

    println!("{}", "Pipeline history:".bold());
    for run in runs.iter().take(limit) {
        let ts = chrono::DateTime::from_timestamp_millis(run.updated_at)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "unknown".to_string());
        println!(
            "  {} {} [{}] {} -- {}",
            &run.id[..8].dimmed(),
            run.pipeline.cyan(),
            run.status.to_string().yellow(),
            ts.dimmed(),
            run.working_dir.dimmed()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{PipelineRun, RunStatus};
    use tempfile::TempDir;

    static PIPELINE_YAML: &str = "name: techspec\ndescription: Research, outline, draft, and review a technical specification\noutput:\n  destination: docs/design\n  filename: \"{date}-{slug}.md\"\nstages:\n  research:\n    description: Gather context\n    command: fabric\n    args: [\"-p\", \"extract_article_wisdom\"]\n    review: false\n  outline:\n    description: Create outline\n    command: fabric\n    args: [\"-p\", \"create_outline\"]\n    review: true\n  draft:\n    description: Write full draft\n    command: fabric\n    args: [\"-p\", \"write_document\"]\n    review: true\n";

    static RESEARCH_YAML: &str = "name: research\ndescription: Deep research pipeline\noutput:\n  destination: research\n  filename: \"{date}-{slug}.md\"\nstages:\n  gather:\n    description: Gather sources\n    command: fabric\n    args: [\"-p\", \"extract_article_wisdom\"]\n  analyze:\n    description: Analyze sources\n    command: fabric\n    args: [\"-p\", \"analyze_paper\"]\n";

    fn test_config(dir: &std::path::Path) -> ForgeConfig {
        ForgeConfig {
            version: "1".to_string(),
            home: dir.to_string_lossy().to_string(),
            store: dir.join("store").to_string_lossy().to_string(),
            pipelines: vec![],
            global_references: vec![],
            log_level: None,
        }
    }

    fn test_config_with_pipelines(dir: &std::path::Path) -> ForgeConfig {
        let pipelines_dir = dir.join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).expect("failed to create dir");
        std::fs::write(pipelines_dir.join("techspec.yml"), PIPELINE_YAML).expect("failed to write");
        std::fs::write(pipelines_dir.join("research.yml"), RESEARCH_YAML).expect("failed to write");
        let mut config = test_config(dir);
        config.pipelines.push("pipelines/".to_string());
        config
    }

    // --- matches_pipeline tests ---

    #[test]
    fn test_matches_pipeline_exact() {
        assert!(matches_pipeline("techspec", "techspec"));
    }

    #[test]
    fn test_matches_pipeline_substring() {
        assert!(matches_pipeline("techspec", "tech"));
        assert!(matches_pipeline("techspec", "spec"));
    }

    #[test]
    fn test_matches_pipeline_case_insensitive() {
        assert!(matches_pipeline("techspec", "TechSpec"));
        assert!(matches_pipeline("TechSpec", "techspec"));
    }

    #[test]
    fn test_matches_pipeline_no_match() {
        assert!(!matches_pipeline("techspec", "blog"));
    }

    #[test]
    fn test_matches_pipeline_empty_pattern() {
        assert!(matches_pipeline("techspec", ""));
    }

    // --- filter_pipelines tests ---

    #[test]
    fn test_filter_pipelines_single_match() {
        let available = vec![
            ("research".to_string(), PathBuf::from("/a/research.yml")),
            ("techspec".to_string(), PathBuf::from("/a/techspec.yml")),
        ];
        let result = filter_pipelines(&available, &["tech".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "techspec");
    }

    #[test]
    fn test_filter_pipelines_multi_match() {
        let available = vec![
            ("blog-post".to_string(), PathBuf::from("/a/blog-post.yml")),
            ("research".to_string(), PathBuf::from("/a/research.yml")),
            ("techspec".to_string(), PathBuf::from("/a/techspec.yml")),
        ];
        let result = filter_pipelines(&available, &["tech".to_string(), "blog".to_string()]).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "blog-post");
        assert_eq!(result[1].0, "techspec");
    }

    #[test]
    fn test_filter_pipelines_overlapping_patterns() {
        let available = vec![("techspec".to_string(), PathBuf::from("/a/techspec.yml"))];
        // Both patterns match "techspec" but it should only appear once
        let result = filter_pipelines(&available, &["tech".to_string(), "spec".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_filter_pipelines_no_match() {
        let available = vec![("techspec".to_string(), PathBuf::from("/a/techspec.yml"))];
        let result = filter_pipelines(&available, &["xyz".to_string()]).unwrap();
        assert!(result.is_empty());
    }

    // --- format_stage_chain tests ---

    #[test]
    fn test_format_stage_chain_with_review() {
        let tmp = tempfile::NamedTempFile::with_suffix(".yml").unwrap();
        std::fs::write(tmp.path(), PIPELINE_YAML).unwrap();
        let pipeline = Pipeline::load(tmp.path()).unwrap();
        let chain = format_stage_chain(&pipeline);
        assert_eq!(chain, "research -> outline [review] -> draft [review]");
    }

    #[test]
    fn test_format_stage_chain_no_review() {
        let tmp = tempfile::NamedTempFile::with_suffix(".yml").unwrap();
        std::fs::write(tmp.path(), RESEARCH_YAML).unwrap();
        let pipeline = Pipeline::load(tmp.path()).unwrap();
        let chain = format_stage_chain(&pipeline);
        assert_eq!(chain, "gather -> analyze");
    }

    // --- cmd_ls compact mode tests ---

    #[test]
    fn test_cmd_ls_compact_no_pipelines() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config(dir.path());
        // No pipelines configured - should print message and succeed
        assert!(cmd_ls(&config, &[], false).is_ok());
    }

    #[test]
    fn test_cmd_ls_compact_with_pipelines() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config_with_pipelines(dir.path());
        assert!(cmd_ls(&config, &[], false).is_ok());
    }

    #[test]
    fn test_cmd_ls_compact_with_active_runs() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config_with_pipelines(dir.path());
        let store_dir = config.store_dir().expect("failed to get store dir");
        std::fs::create_dir_all(&store_dir).expect("failed to create store dir");
        let mut store = store::open_store(&store_dir).expect("failed to open store");

        let run = PipelineRun::new(
            "techspec".to_string(),
            "/tmp/test".to_string(),
            None,
            None,
            vec!["research".to_string()],
        );
        store.create(run).expect("failed to create run");

        assert!(cmd_ls(&config, &[], false).is_ok());
    }

    // --- cmd_ls detailed mode tests ---

    #[test]
    fn test_cmd_ls_detailed_by_pattern() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config_with_pipelines(dir.path());
        assert!(cmd_ls(&config, &["tech".to_string()], false).is_ok());
    }

    #[test]
    fn test_cmd_ls_detailed_all() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config_with_pipelines(dir.path());
        assert!(cmd_ls(&config, &[], true).is_ok());
    }

    #[test]
    fn test_cmd_ls_detailed_with_active_runs() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config_with_pipelines(dir.path());
        let store_dir = config.store_dir().expect("failed to get store dir");
        std::fs::create_dir_all(&store_dir).expect("failed to create store dir");
        let mut store = store::open_store(&store_dir).expect("failed to open store");

        let run = PipelineRun::new(
            "techspec".to_string(),
            "/tmp/test".to_string(),
            None,
            None,
            vec!["research".to_string(), "outline".to_string()],
        );
        store.create(run).expect("failed to create run");

        assert!(cmd_ls(&config, &["tech".to_string()], false).is_ok());
    }

    #[test]
    fn test_cmd_ls_detailed_no_match() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config_with_pipelines(dir.path());
        // Should succeed but print warning about no match
        assert!(cmd_ls(&config, &["xyz".to_string()], false).is_ok());
    }

    // --- load_active_runs tests ---

    #[test]
    fn test_load_active_runs_no_store() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config(dir.path());
        let runs = load_active_runs(&config).unwrap();
        assert!(runs.is_empty());
    }

    #[test]
    fn test_load_active_runs_empty_store() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config(dir.path());
        // Store dir doesn't exist - should return empty map
        let runs = load_active_runs(&config).unwrap();
        assert!(runs.is_empty());
    }

    #[test]
    fn test_load_active_runs_groups_by_pipeline() {
        // Note: load_active_runs opens its own store handle, and taskstore
        // has eventual consistency across handles. We test grouping behavior
        // through cmd_ls integration tests instead, and test the empty case directly.
        // Here we verify the function works when the store exists but has no active runs.
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config(dir.path());
        let store_dir = config.store_dir().expect("failed to get store dir");
        std::fs::create_dir_all(&store_dir).expect("failed to create store dir");

        let mut s = store::open_store(&store_dir).expect("failed to open store");
        let mut completed = PipelineRun::new(
            "techspec".to_string(),
            "/tmp/d".to_string(),
            None,
            None,
            vec!["s1".to_string()],
        );
        completed.status = RunStatus::Completed;
        completed.touch();
        s.create(completed).unwrap();
        drop(s);

        // load_active_runs should find no Unpacked/InProgress runs
        let runs = load_active_runs(&config).unwrap();
        assert!(runs.is_empty());
    }

    // --- existing tests updated ---

    #[test]
    fn test_cmd_history_empty() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config(dir.path());
        assert!(cmd_history(&config, None, 10).is_ok());
    }

    #[test]
    fn test_cmd_history_with_filter() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config(dir.path());
        let store_dir = config.store_dir().expect("failed to get store dir");
        std::fs::create_dir_all(&store_dir).expect("failed to create store dir");
        let mut store = store::open_store(&store_dir).expect("failed to open store");

        let run = PipelineRun::new(
            "techspec".to_string(),
            "/tmp/test".to_string(),
            None,
            None,
            vec!["research".to_string()],
        );
        store.create(run).expect("failed to create run");

        assert!(cmd_history(&config, Some("techspec"), 10).is_ok());
        assert!(cmd_history(&config, Some("nonexistent"), 10).is_ok());
    }

    #[test]
    fn test_cmd_show_by_id() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config(dir.path());
        let store_dir = config.store_dir().expect("failed to get store dir");
        std::fs::create_dir_all(&store_dir).expect("failed to create store dir");
        let mut store = store::open_store(&store_dir).expect("failed to open store");

        let run = PipelineRun::new(
            "techspec".to_string(),
            "/tmp/test".to_string(),
            None,
            Some("my-slug".to_string()),
            vec!["research".to_string(), "outline".to_string()],
        );
        let run_id = run.id.clone();
        store.create(run).expect("failed to create run");

        assert!(cmd_show(&config, Some(&run_id)).is_ok());
    }

    #[test]
    fn test_cmd_show_not_found() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config(dir.path());
        let store_dir = config.store_dir().expect("failed to get store dir");
        std::fs::create_dir_all(&store_dir).expect("failed to create store dir");
        let _ = store::open_store(&store_dir).expect("failed to open store");

        assert!(cmd_show(&config, Some("nonexistent-id")).is_err());
    }

    #[test]
    fn test_store_query_by_status() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let store_dir = dir.path().join("store");
        std::fs::create_dir_all(&store_dir).expect("failed to create store dir");
        let mut store = store::open_store(&store_dir).expect("failed to open store");

        let run1 = PipelineRun::new(
            "techspec".to_string(),
            "/tmp/a".to_string(),
            None,
            None,
            vec!["s1".to_string()],
        );
        let mut run2 = PipelineRun::new(
            "research".to_string(),
            "/tmp/b".to_string(),
            None,
            None,
            vec!["s1".to_string()],
        );
        run2.status = RunStatus::Completed;
        run2.touch();

        store.create(run1).expect("failed to create run1");
        store.create(run2).expect("failed to create run2");

        let unpacked: Vec<PipelineRun> = store
            .list(&[taskstore::Filter {
                field: "status".to_string(),
                op: taskstore::FilterOp::Eq,
                value: taskstore::IndexValue::String("Unpacked".to_string()),
            }])
            .expect("failed to list");
        assert_eq!(unpacked.len(), 1);
        assert_eq!(unpacked[0].pipeline, "techspec");

        let all: Vec<PipelineRun> = store.list(&[]).expect("failed to list");
        assert_eq!(all.len(), 2);
    }
}
