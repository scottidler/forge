pub mod briefcase;
pub mod cli;
pub mod config;
pub mod executor;
pub mod init;
pub mod pipeline;
pub mod store;

use colored::Colorize;
use eyre::{Result, eyre};

use crate::cli::Command;
use crate::config::ForgeConfig;
use crate::pipeline::Pipeline;

pub fn run_command(command: &Command, config: &ForgeConfig) -> Result<()> {
    match command {
        Command::Pipelines => cmd_pipelines(config),
        Command::Describe { pipeline, stage } => cmd_describe(config, pipeline, *stage),
        Command::Refs { pipeline, stage } => cmd_refs(config, pipeline, *stage),
        Command::Unpack { pipeline, input, slug } => {
            briefcase::unpack(config, pipeline, input.as_deref(), slug.as_deref())
        }
        Command::Pack { abandon } => briefcase::pack(config, *abandon),
        Command::Run { stage, input } => executor::run_stage(config, stage.as_deref(), input.as_deref()),
        Command::Ls { all } => cmd_ls(config, *all),
        Command::Show { run_id } => cmd_show(config, run_id.as_deref()),
        Command::History { pipeline, limit } => cmd_history(config, pipeline.as_deref(), *limit),
        Command::Init { .. } => unreachable!("Init is handled before config loading"),
    }
}

fn cmd_pipelines(config: &ForgeConfig) -> Result<()> {
    if config.pipelines.is_empty() {
        println!("No pipelines configured.");
        return Ok(());
    }
    let pipelines = config.list_pipelines()?;
    if pipelines.is_empty() {
        println!("No pipelines found in configured directories.");
        return Ok(());
    }
    println!("{}", "Available pipelines:".bold());
    for (name, path) in &pipelines {
        println!("  {} -> {}", name.cyan(), path.display());
    }
    Ok(())
}

fn cmd_describe(config: &ForgeConfig, pipeline_name: &str, stage_filter: Option<usize>) -> Result<()> {
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
        println!("     fabric-pattern: {}", stage.fabric_pattern.dimmed());
        if !stage.references.is_empty() {
            for r in &stage.references {
                println!("     ref: {}", r.dimmed());
            }
        }
    }
    Ok(())
}

fn cmd_refs(config: &ForgeConfig, pipeline_name: &str, stage_filter: Option<usize>) -> Result<()> {
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

fn cmd_ls(config: &ForgeConfig, all: bool) -> Result<()> {
    let store_dir = config.store_dir()?;
    if !store_dir.exists() {
        println!("No pipeline runs found.");
        return Ok(());
    }
    let store = store::open_store(&store_dir)?;
    let filters = if all {
        vec![]
    } else {
        vec![taskstore::Filter {
            field: "status".to_string(),
            op: taskstore::FilterOp::Eq,
            value: taskstore::IndexValue::String("Unpacked".to_string()),
        }]
    };
    let mut runs: Vec<store::PipelineRun> = store.list(&filters)?;
    if !all {
        // Also include InProgress
        let in_progress: Vec<store::PipelineRun> = store.list(&[taskstore::Filter {
            field: "status".to_string(),
            op: taskstore::FilterOp::Eq,
            value: taskstore::IndexValue::String("InProgress".to_string()),
        }])?;
        runs.extend(in_progress);
    }
    if runs.is_empty() {
        println!("No active pipeline runs.");
        return Ok(());
    }
    println!("{}", "Pipeline runs:".bold());
    for run in &runs {
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
            "  {} {} [{}] {} -- {}",
            &run.id[..8].dimmed(),
            run.pipeline.cyan(),
            run.status.to_string().yellow(),
            stage_info,
            run.working_dir.dimmed()
        );
    }
    Ok(())
}

fn cmd_show(config: &ForgeConfig, run_id: Option<&str>) -> Result<()> {
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

    fn test_config(dir: &std::path::Path) -> ForgeConfig {
        ForgeConfig {
            version: "1".to_string(),
            home: dir.to_string_lossy().to_string(),
            store: dir.join("store").to_string_lossy().to_string(),
            pipelines: vec![],
            fabric: crate::config::FabricConfig::default(),
            global_references: vec![],
        }
    }

    #[test]
    fn test_cmd_ls_empty_store() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config(dir.path());
        // Should not error when store dir doesn't exist
        assert!(cmd_ls(&config, false).is_ok());
    }

    #[test]
    fn test_cmd_ls_with_runs() {
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

        assert!(cmd_ls(&config, false).is_ok());
        assert!(cmd_ls(&config, true).is_ok());
    }

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
    fn test_cmd_pipelines_empty() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let config = test_config(dir.path());
        assert!(cmd_pipelines(&config).is_ok());
    }

    #[test]
    fn test_cmd_pipelines_with_entries() {
        let dir = TempDir::new().expect("failed to create temp dir");
        // Create a pipelines directory with a YAML file
        let pipelines_dir = dir.path().join("pipelines");
        std::fs::create_dir_all(&pipelines_dir).expect("failed to create dir");
        std::fs::write(
            pipelines_dir.join("techspec.yml"),
            "name: techspec\ndescription: test\noutput:\n  destination: .\n  filename: out.md\nstages:\n  s1:\n    description: d\n    fabric-pattern: p\n",
        )
        .expect("failed to write");
        let mut config = test_config(dir.path());
        config.pipelines.push("pipelines/".to_string());
        assert!(cmd_pipelines(&config).is_ok());
    }

    #[test]
    fn test_store_query_by_status() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let store_dir = dir.path().join("store");
        std::fs::create_dir_all(&store_dir).expect("failed to create store dir");
        let mut store = store::open_store(&store_dir).expect("failed to open store");

        // Create two runs with different statuses
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

        // Query only Unpacked
        let unpacked: Vec<PipelineRun> = store
            .list(&[taskstore::Filter {
                field: "status".to_string(),
                op: taskstore::FilterOp::Eq,
                value: taskstore::IndexValue::String("Unpacked".to_string()),
            }])
            .expect("failed to list");
        assert_eq!(unpacked.len(), 1);
        assert_eq!(unpacked[0].pipeline, "techspec");

        // Query all
        let all: Vec<PipelineRun> = store.list(&[]).expect("failed to list");
        assert_eq!(all.len(), 2);
    }
}
