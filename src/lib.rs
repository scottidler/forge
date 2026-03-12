pub mod briefcase;
pub mod cli;
pub mod config;
pub mod executor;
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
    }
}

fn cmd_pipelines(config: &ForgeConfig) -> Result<()> {
    if config.pipelines.is_empty() {
        println!("No pipelines configured.");
        return Ok(());
    }
    println!("{}", "Available pipelines:".bold());
    for (name, path) in &config.pipelines {
        let home = config.home_dir()?;
        let full_path = home.join(path);
        let status = if full_path.exists() {
            "ok".green().to_string()
        } else {
            "missing".red().to_string()
        };
        println!("  {} [{}] -> {}", name.cyan(), status, path);
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
    for (i, stage) in pipeline.stages.iter().enumerate() {
        if let Some(filter) = stage_filter
            && i != filter
        {
            continue;
        }
        let review_tag = if stage.review { " [review gate]".yellow().to_string() } else { String::new() };
        println!(
            "  {}. {} — {}{}",
            i + 1,
            stage.name.bold(),
            stage.description,
            review_tag
        );
        println!("     pattern: {}", stage.pattern.dimmed());
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

    for (i, stage) in pipeline.stages.iter().enumerate() {
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
            "  {} {} [{}] {} — {}",
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
            "  {} {} [{}] {} — {}",
            &run.id[..8].dimmed(),
            run.pipeline.cyan(),
            run.status.to_string().yellow(),
            ts.dimmed(),
            run.working_dir.dimmed()
        );
    }
    Ok(())
}
