use colored::Colorize;
use eyre::{Context, Result, eyre};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::ForgeConfig;
use crate::pipeline::Pipeline;
use crate::store::{self, PipelineRun, RunStatus, StageStatus};

const FORGE_DIR: &str = ".forge";
const RUN_ID_FILE: &str = ".run-id";

pub fn run_stage(config: &ForgeConfig, stage_name: Option<&str>, input: Option<&str>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let forge_dir = cwd.join(FORGE_DIR);

    if !forge_dir.exists() {
        return Err(eyre!("no .forge/ directory — run `forge unpack` first"));
    }

    // Load run from TaskStore
    let run_id = fs::read_to_string(forge_dir.join(RUN_ID_FILE))
        .context("failed to read .run-id")?
        .trim()
        .to_string();

    let store_dir = config.store_dir()?;
    let mut store = store::open_store(&store_dir)?;
    let mut run: PipelineRun = store
        .get(&run_id)?
        .ok_or_else(|| eyre!("pipeline run {} not found in store", run_id))?;

    // Load pipeline definition
    let pipeline_path = config.pipeline_path(&run.pipeline)?;
    let pipeline = Pipeline::load(&pipeline_path)?;

    // Determine which stage to run
    let stage_index = determine_stage_index(&run, &pipeline, stage_name)?;

    // If current stage is in Review, approve it and advance
    if run.stages[stage_index].status == StageStatus::Review {
        run.stages[stage_index].status = StageStatus::Completed;
        run.stages[stage_index].completed_at = Some(taskstore::now_ms());
        println!("{} Stage '{}' approved", "ok".green(), run.stages[stage_index].name);

        // Find next pending stage
        let next = find_next_pending(&run, stage_index + 1);
        if let Some(next_index) = next {
            run.current_stage = next_index;
            run.touch();
            store.update(run.clone())?;
            // Execute the next stage
            return execute_stage(config, &mut store, &mut run, &pipeline, next_index, &forge_dir, input);
        }
        // All stages complete
        run.status = RunStatus::Completed;
        run.touch();
        store.update(run)?;
        println!("{} All stages complete — run `forge pack` to finalize", "ok".green());
        return Ok(());
    }

    // Execute the stage
    execute_stage(config, &mut store, &mut run, &pipeline, stage_index, &forge_dir, input)
}

fn determine_stage_index(run: &PipelineRun, pipeline: &Pipeline, stage_name: Option<&str>) -> Result<usize> {
    if let Some(name) = stage_name {
        // Find stage by name
        pipeline
            .stages
            .iter()
            .position(|s| s.name == name)
            .ok_or_else(|| eyre!("stage '{}' not found in pipeline '{}'", name, pipeline.name))
    } else {
        // Find next actionable stage
        // If current stage is Review, return it (so it can be approved)
        if run.current_stage < run.stages.len() && run.stages[run.current_stage].status == StageStatus::Review {
            return Ok(run.current_stage);
        }
        // Otherwise find next pending
        find_next_pending(run, run.current_stage)
            .ok_or_else(|| eyre!("all stages are complete — run `forge pack` to finalize"))
    }
}

fn find_next_pending(run: &PipelineRun, from: usize) -> Option<usize> {
    (from..run.stages.len()).find(|&i| run.stages[i].status == StageStatus::Pending)
}

fn execute_stage(
    config: &ForgeConfig,
    store: &mut taskstore::Store,
    run: &mut PipelineRun,
    pipeline: &Pipeline,
    stage_index: usize,
    forge_dir: &Path,
    cli_input: Option<&str>,
) -> Result<()> {
    let stage_def = &pipeline.stages[stage_index];
    let stage_num = stage_index + 1;

    println!(
        "{} Running stage {}/{}: {}",
        ">>".cyan(),
        stage_num,
        pipeline.stages.len(),
        stage_def.name.bold()
    );

    // Mark stage as in progress
    run.stages[stage_index].status = StageStatus::InProgress;
    run.stages[stage_index].started_at = Some(taskstore::now_ms());
    run.status = RunStatus::InProgress;
    run.current_stage = stage_index;
    run.touch();
    store.update(run.clone())?;

    // Build input for fabric
    let fabric_input = compose_stage_input(config, pipeline, stage_index, forge_dir, cli_input)?;

    // Execute fabric
    let output = call_fabric(
        &config.fabric.binary,
        &stage_def.pattern,
        &config.fabric.model,
        &fabric_input,
    )?;

    // Write output to .forge/<NN>-<name>.md
    let output_file = forge_dir.join(format!("{:02}-{}.md", stage_num, stage_def.name));
    fs::write(&output_file, &output).context("failed to write stage output")?;

    println!("{} Output written to {}", "ok".green(), output_file.display());

    // Handle review gate
    if stage_def.review {
        run.stages[stage_index].status = StageStatus::Review;
        run.touch();
        store.update(run.clone())?;

        // Print the output
        println!("\n{}", "--- Stage Output ---".bold().yellow());
        println!("{}", output);
        println!("{}", "--- End Output ---".bold().yellow());
        println!(
            "\n{} Stage '{}' is waiting for review.",
            "review".yellow(),
            stage_def.name
        );
        println!(
            "   Edit .forge/{:02}-{}.md if needed, then run `forge run` to approve and continue.",
            stage_num, stage_def.name
        );
    } else {
        run.stages[stage_index].status = StageStatus::Completed;
        run.stages[stage_index].completed_at = Some(taskstore::now_ms());
        run.touch();
        store.update(run.clone())?;

        // Check if there are more stages
        if let Some(next) = find_next_pending(run, stage_index + 1) {
            run.current_stage = next;
            run.touch();
            store.update(run.clone())?;
            println!(
                "   Next: run `forge run` to execute stage '{}'",
                pipeline.stages[next].name
            );
        } else {
            run.status = RunStatus::Completed;
            run.touch();
            store.update(run.clone())?;
            println!("{} All stages complete — run `forge pack` to finalize", "ok".green());
        }
    }

    Ok(())
}

fn compose_stage_input(
    config: &ForgeConfig,
    pipeline: &Pipeline,
    stage_index: usize,
    forge_dir: &Path,
    cli_input: Option<&str>,
) -> Result<String> {
    let stage = &pipeline.stages[stage_index];
    let mut parts: Vec<String> = Vec::new();

    // 1. Task description
    parts.push(format!("--- TASK ---\n{}", stage.description));

    // 2. Previous stage output or initial input
    if stage_index == 0 {
        // First stage: use CLI input or .forge/input.md
        let input_content = if let Some(text) = cli_input {
            let path = PathBuf::from(text);
            if path.exists() {
                fs::read_to_string(&path).context(format!("failed to read input: {}", text))?
            } else {
                text.to_string()
            }
        } else {
            let input_file = forge_dir.join("input.md");
            if input_file.exists() {
                fs::read_to_string(&input_file).context("failed to read input.md")?
            } else {
                String::new()
            }
        };
        if !input_content.is_empty() {
            parts.push(format!("--- INPUT ---\n{}", input_content));
        }
    } else {
        // Subsequent stages: use previous stage output
        let prev = &pipeline.stages[stage_index - 1];
        let prev_file = forge_dir.join(format!("{:02}-{}.md", stage_index, prev.name));
        if prev_file.exists() {
            let content = fs::read_to_string(&prev_file).context("failed to read previous stage output")?;
            parts.push(format!("--- PREVIOUS OUTPUT ---\n{}", content));
        }
    }

    // 3. References
    let all_refs = pipeline.all_references_for_stage(stage_index, &config.global_references);
    let refs_dir = forge_dir.join("references");
    for ref_path in &all_refs {
        let filename = Path::new(ref_path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| ref_path.clone());
        let ref_file = refs_dir.join(&filename);
        if ref_file.exists() {
            let content = fs::read_to_string(&ref_file).context(format!("failed to read reference: {}", filename))?;
            parts.push(format!("--- REFERENCE: {} ---\n{}", filename, content));
        }
    }

    Ok(parts.join("\n\n"))
}

fn call_fabric(binary: &str, pattern: &str, model: &str, input: &str) -> Result<String> {
    let mut cmd = Command::new(binary);
    cmd.arg("-p").arg(pattern);

    if !model.is_empty() {
        cmd.arg("-m").arg(model);
    }

    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().context(format!(
        "failed to start fabric — is '{}' installed and in PATH?",
        binary
    ))?;

    if let Some(ref mut stdin) = child.stdin {
        stdin
            .write_all(input.as_bytes())
            .context("failed to write to fabric stdin")?;
    }

    let output = child.wait_with_output().context("failed to wait for fabric")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!("fabric failed (exit {}): {}", output.status, stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{OutputConfig, Pipeline, Stage};
    use tempfile::TempDir;

    fn test_pipeline() -> Pipeline {
        Pipeline {
            name: "test".to_string(),
            description: "test pipeline".to_string(),
            output: OutputConfig {
                destination: "out/".to_string(),
                filename: "{date}-{slug}.md".to_string(),
            },
            references: vec!["references/voice.md".to_string()],
            stages: vec![
                Stage {
                    name: "research".to_string(),
                    description: "Gather context".to_string(),
                    pattern: "extract_article_wisdom".to_string(),
                    references: vec![],
                    review: false,
                },
                Stage {
                    name: "outline".to_string(),
                    description: "Create outline".to_string(),
                    pattern: "create_outline".to_string(),
                    references: vec!["references/templates/techspec.md".to_string()],
                    review: true,
                },
            ],
        }
    }

    #[test]
    fn test_compose_stage_input_first_stage() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let forge_dir = dir.path().join(".forge");
        fs::create_dir_all(forge_dir.join("references")).expect("failed to create dirs");

        // Write input file
        fs::write(forge_dir.join("input.md"), "My topic").expect("failed to write input");

        // Write a reference
        fs::write(forge_dir.join("references/voice.md"), "Be concise.").expect("failed to write ref");

        let config = crate::config::ForgeConfig {
            version: "1".to_string(),
            home: dir.path().to_string_lossy().to_string(),
            store: dir.path().join("store").to_string_lossy().to_string(),
            pipelines: std::collections::HashMap::new(),
            fabric: crate::config::FabricConfig::default(),
            global_references: vec![],
        };

        let pipeline = test_pipeline();
        let result = compose_stage_input(&config, &pipeline, 0, &forge_dir, None).expect("failed to compose");
        assert!(result.contains("--- TASK ---"));
        assert!(result.contains("Gather context"));
        assert!(result.contains("--- INPUT ---"));
        assert!(result.contains("My topic"));
    }

    #[test]
    fn test_compose_stage_input_second_stage() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let forge_dir = dir.path().join(".forge");
        fs::create_dir_all(forge_dir.join("references")).expect("failed to create dirs");

        // Write previous stage output
        fs::write(forge_dir.join("01-research.md"), "Research results here").expect("failed to write");

        let config = crate::config::ForgeConfig {
            version: "1".to_string(),
            home: dir.path().to_string_lossy().to_string(),
            store: dir.path().join("store").to_string_lossy().to_string(),
            pipelines: std::collections::HashMap::new(),
            fabric: crate::config::FabricConfig::default(),
            global_references: vec![],
        };

        let pipeline = test_pipeline();
        let result = compose_stage_input(&config, &pipeline, 1, &forge_dir, None).expect("failed to compose");
        assert!(result.contains("--- TASK ---"));
        assert!(result.contains("Create outline"));
        assert!(result.contains("--- PREVIOUS OUTPUT ---"));
        assert!(result.contains("Research results here"));
    }

    #[test]
    fn test_compose_stage_input_cli_override() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let forge_dir = dir.path().join(".forge");
        fs::create_dir_all(forge_dir.join("references")).expect("failed to create dirs");

        let config = crate::config::ForgeConfig {
            version: "1".to_string(),
            home: dir.path().to_string_lossy().to_string(),
            store: dir.path().join("store").to_string_lossy().to_string(),
            pipelines: std::collections::HashMap::new(),
            fabric: crate::config::FabricConfig::default(),
            global_references: vec![],
        };

        let pipeline = test_pipeline();
        let result =
            compose_stage_input(&config, &pipeline, 0, &forge_dir, Some("CLI input text")).expect("failed to compose");
        assert!(result.contains("CLI input text"));
    }

    #[test]
    fn test_find_next_pending() {
        let run = PipelineRun::new(
            "test".to_string(),
            "/tmp".to_string(),
            None,
            None,
            vec!["s1".to_string(), "s2".to_string(), "s3".to_string()],
        );
        assert_eq!(find_next_pending(&run, 0), Some(0));
        assert_eq!(find_next_pending(&run, 1), Some(1));
        assert_eq!(find_next_pending(&run, 3), None);
    }

    #[test]
    fn test_determine_stage_by_name() {
        let run = PipelineRun::new(
            "test".to_string(),
            "/tmp".to_string(),
            None,
            None,
            vec!["research".to_string(), "outline".to_string()],
        );
        let pipeline = test_pipeline();
        let idx = determine_stage_index(&run, &pipeline, Some("outline")).expect("failed to find stage");
        assert_eq!(idx, 1);
    }

    #[test]
    fn test_determine_stage_unknown() {
        let run = PipelineRun::new(
            "test".to_string(),
            "/tmp".to_string(),
            None,
            None,
            vec!["research".to_string()],
        );
        let pipeline = test_pipeline();
        assert!(determine_stage_index(&run, &pipeline, Some("nonexistent")).is_err());
    }
}
