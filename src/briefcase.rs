use colored::Colorize;
use eyre::{Context, Result, eyre};
use log::debug;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::ForgeConfig;
use crate::pipeline::Pipeline;
use crate::store::{self, PipelineRun, RunStatus};

const FORGE_DIR: &str = ".forge";
const GITIGNORE_MARKER: &str = "# forge-managed";
const RUN_ID_FILE: &str = ".run-id";

/// Deploy pipeline scaffolding into the current directory
pub fn unpack(config: &ForgeConfig, pipeline_name: &str, input: Option<&str>, slug: Option<&str>) -> Result<()> {
    debug!(
        "unpack: pipeline_name={}, input={:?}, slug={:?}",
        pipeline_name, input, slug
    );
    let cwd = std::env::current_dir()?;
    let forge_dir = cwd.join(FORGE_DIR);

    // Collision detection
    if forge_dir.exists() {
        return Err(eyre!(
            ".forge/ already exists in {}; pack or remove it first",
            cwd.display()
        ));
    }

    // Load and validate pipeline
    let pipeline_path = config.pipeline_path(pipeline_name)?;
    let pipeline = Pipeline::load(&pipeline_path)?;

    // Create .forge/ directory
    fs::create_dir_all(&forge_dir).context("failed to create .forge/")?;

    // Symlink pipeline definition
    let pipeline_link = forge_dir.join("pipeline.yml");
    std::os::unix::fs::symlink(&pipeline_path, &pipeline_link).context("failed to symlink pipeline.yml")?;

    // Create references/ directory and symlink all references
    let refs_dir = forge_dir.join("references");
    fs::create_dir_all(&refs_dir).context("failed to create .forge/references/")?;

    // Collect all unique references (global + pipeline-level + all stage-level)
    let mut all_refs: Vec<String> = config.global_references.clone();
    all_refs.extend(pipeline.references.clone());
    for stage in pipeline.stages.values() {
        all_refs.extend(stage.references.clone());
    }
    all_refs.sort();
    all_refs.dedup();

    for ref_path in &all_refs {
        let source = config.reference_path(ref_path)?;
        if !source.exists() {
            log::warn!("reference not found: {}", source.display());
            continue;
        }
        // Use the filename as the symlink name (flatten the directory structure)
        let filename = Path::new(ref_path)
            .file_name()
            .ok_or_else(|| eyre!("invalid reference path: {}", ref_path))?;
        let link = refs_dir.join(filename);
        if !link.exists() {
            std::os::unix::fs::symlink(&source, &link).context(format!("failed to symlink reference: {}", ref_path))?;
        }
    }

    // Write initial input if provided
    if let Some(input_text) = input {
        let input_path = forge_dir.join("input.md");
        // Check if it's a file path
        let input_file = PathBuf::from(input_text);
        if input_file.exists() {
            let content =
                fs::read_to_string(&input_file).context(format!("failed to read input file: {}", input_text))?;
            fs::write(&input_path, content).context("failed to write input.md")?;
        } else {
            fs::write(&input_path, input_text).context("failed to write input.md")?;
        }
    }

    // Create TaskStore record
    let store_dir = config.store_dir()?;
    fs::create_dir_all(&store_dir).context("failed to create store directory")?;
    let mut store = store::open_store(&store_dir)?;

    let stage_names: Vec<String> = pipeline.stages.keys().cloned().collect();
    let run = PipelineRun::new(
        pipeline_name.to_string(),
        cwd.to_string_lossy().to_string(),
        input.map(|s| s.to_string()),
        slug.map(|s| s.to_string()),
        stage_names,
    );

    // Write run ID file
    fs::write(forge_dir.join(RUN_ID_FILE), &run.id).context("failed to write .run-id")?;

    store.create(run.clone())?;

    // Add .forge to .gitignore
    add_to_gitignore(&cwd)?;

    println!("{} Pipeline '{}' unpacked", "ok".green(), pipeline_name.cyan());
    println!("   Run ID: {}", run.id.dimmed());
    println!("   Stages: {}", pipeline.stages.len());
    println!("   Next: run `forge run` to execute the first stage");

    Ok(())
}

/// Retract .forge/ from current directory
pub fn pack(config: &ForgeConfig, abandon: bool) -> Result<()> {
    debug!("pack: abandon={}", abandon);
    let cwd = std::env::current_dir()?;
    let forge_dir = cwd.join(FORGE_DIR);

    if !forge_dir.exists() {
        return Err(eyre!("no .forge/ directory in {}", cwd.display()));
    }

    // Read run ID
    let run_id_path = forge_dir.join(RUN_ID_FILE);
    let run_id = fs::read_to_string(&run_id_path).context("failed to read .run-id -- is this a forge directory?")?;
    let run_id = run_id.trim().to_string();

    // Load run from TaskStore
    let store_dir = config.store_dir()?;
    let mut store = store::open_store(&store_dir)?;
    let mut run: PipelineRun = store
        .get(&run_id)?
        .ok_or_else(|| eyre!("pipeline run {} not found in store", run_id))?;

    // Archive intermediates to artifact store
    let run_dir = store_dir.join("runs").join(&run_id);
    fs::create_dir_all(&run_dir).context("failed to create run archive directory")?;

    // Copy stage output files
    for (i, stage) in run.stages.iter_mut().enumerate() {
        let stage_file = forge_dir.join(format!("{:02}-{}.md", i + 1, stage.name));
        if stage_file.exists() {
            let dest = run_dir.join(format!("{:02}-{}.md", i + 1, stage.name));
            fs::copy(&stage_file, &dest).context(format!("failed to archive stage output: {}", stage.name))?;
            stage.artifact_path = Some(dest.to_string_lossy().to_string());
        }
    }

    // Write final output to destination (unless abandoning)
    if !abandon {
        // Find the last completed stage output
        let last_output = find_last_stage_output(&forge_dir, &run);
        if let Some(output_path) = last_output {
            let pipeline_path = config.pipeline_path(&run.pipeline)?;
            let pipeline = Pipeline::load(&pipeline_path)?;

            let dest_dir = cwd.join(&pipeline.output.destination);
            fs::create_dir_all(&dest_dir).context("failed to create output destination")?;

            let filename = resolve_output_filename(&pipeline.output.filename, run.slug.as_deref());
            let dest = dest_dir.join(&filename);

            fs::copy(&output_path, &dest).context("failed to write final output")?;
            run.final_destination = Some(dest.to_string_lossy().to_string());
            run.status = RunStatus::Completed;

            println!("{} Final output: {}", "ok".green(), dest.display());
        } else if !abandon {
            run.status = RunStatus::Abandoned;
            println!("{} No completed stage outputs found, abandoning", "warn".yellow());
        }
    } else {
        run.status = RunStatus::Abandoned;
    }

    // Update TaskStore
    run.touch();
    store.update(run)?;

    // Remove .forge/ directory
    fs::remove_dir_all(&forge_dir).context("failed to remove .forge/")?;

    // Remove .forge from .gitignore
    remove_from_gitignore(&cwd)?;

    println!("{} Pipeline packed", "ok".green());
    Ok(())
}

fn find_last_stage_output(forge_dir: &Path, run: &PipelineRun) -> Option<PathBuf> {
    for (i, stage) in run.stages.iter().enumerate().rev() {
        if stage.status == store::StageStatus::Completed || stage.status == store::StageStatus::Review {
            let path = forge_dir.join(format!("{:02}-{}.md", i + 1, stage.name));
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

fn resolve_output_filename(template: &str, slug: Option<&str>) -> String {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let slug_val = slug.unwrap_or("output");
    template.replace("{date}", &today).replace("{slug}", slug_val)
}

fn add_to_gitignore(dir: &Path) -> Result<()> {
    let gitignore = dir.join(".gitignore");
    let entry = format!("{} {}", FORGE_DIR, GITIGNORE_MARKER);

    if gitignore.exists() {
        let content = fs::read_to_string(&gitignore)?;
        // Check if .forge is already there
        for line in content.lines() {
            if line.trim().starts_with(FORGE_DIR) {
                // Already present (user or forge-managed), don't duplicate
                return Ok(());
            }
        }
        // Append
        let mut new_content = content;
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(&entry);
        new_content.push('\n');
        fs::write(&gitignore, new_content)?;
    } else {
        fs::write(&gitignore, format!("{}\n", entry))?;
    }
    Ok(())
}

fn remove_from_gitignore(dir: &Path) -> Result<()> {
    let gitignore = dir.join(".gitignore");
    if !gitignore.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&gitignore)?;
    let lines: Vec<&str> = content.lines().collect();
    let filtered: Vec<&str> = lines
        .into_iter()
        .filter(|line| {
            // Only remove lines with our marker
            !(line.contains(FORGE_DIR) && line.contains(GITIGNORE_MARKER))
        })
        .collect();

    let new_content = filtered.join("\n");
    // If gitignore is empty after removal, delete it
    if new_content.trim().is_empty() {
        fs::remove_file(&gitignore)?;
    } else {
        let mut final_content = new_content;
        if !final_content.ends_with('\n') {
            final_content.push('\n');
        }
        fs::write(&gitignore, final_content)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_resolve_output_filename() {
        let result = resolve_output_filename("{date}-{slug}.md", Some("my-spec"));
        assert!(result.ends_with("-my-spec.md"));
        assert!(result.starts_with("20")); // year prefix
    }

    #[test]
    fn test_resolve_output_filename_no_slug() {
        let result = resolve_output_filename("{date}-{slug}.md", None);
        assert!(result.ends_with("-output.md"));
    }

    #[test]
    fn test_add_to_gitignore_new() {
        let dir = TempDir::new().expect("failed to create temp dir");
        add_to_gitignore(dir.path()).expect("failed to add");
        let content = fs::read_to_string(dir.path().join(".gitignore")).expect("failed to read");
        assert!(content.contains(".forge"));
        assert!(content.contains(GITIGNORE_MARKER));
    }

    #[test]
    fn test_add_to_gitignore_existing() {
        let dir = TempDir::new().expect("failed to create temp dir");
        fs::write(dir.path().join(".gitignore"), "target/\n").expect("failed to write");
        add_to_gitignore(dir.path()).expect("failed to add");
        let content = fs::read_to_string(dir.path().join(".gitignore")).expect("failed to read");
        assert!(content.contains("target/"));
        assert!(content.contains(".forge"));
    }

    #[test]
    fn test_add_to_gitignore_already_present() {
        let dir = TempDir::new().expect("failed to create temp dir");
        fs::write(dir.path().join(".gitignore"), ".forge\n").expect("failed to write");
        add_to_gitignore(dir.path()).expect("failed to add");
        let content = fs::read_to_string(dir.path().join(".gitignore")).expect("failed to read");
        // Should not duplicate
        assert_eq!(content.matches(".forge").count(), 1);
    }

    #[test]
    fn test_remove_from_gitignore() {
        let dir = TempDir::new().expect("failed to create temp dir");
        fs::write(
            dir.path().join(".gitignore"),
            format!("target/\n.forge {}\n", GITIGNORE_MARKER),
        )
        .expect("failed to write");
        remove_from_gitignore(dir.path()).expect("failed to remove");
        let content = fs::read_to_string(dir.path().join(".gitignore")).expect("failed to read");
        assert!(content.contains("target/"));
        assert!(!content.contains(".forge"));
    }

    #[test]
    fn test_remove_from_gitignore_empty_result() {
        let dir = TempDir::new().expect("failed to create temp dir");
        fs::write(dir.path().join(".gitignore"), format!(".forge {}\n", GITIGNORE_MARKER)).expect("failed to write");
        remove_from_gitignore(dir.path()).expect("failed to remove");
        assert!(!dir.path().join(".gitignore").exists());
    }

    #[test]
    fn test_remove_from_gitignore_preserves_user_entry() {
        let dir = TempDir::new().expect("failed to create temp dir");
        fs::write(dir.path().join(".gitignore"), ".forge\ntarget/\n").expect("failed to write");
        remove_from_gitignore(dir.path()).expect("failed to remove");
        let content = fs::read_to_string(dir.path().join(".gitignore")).expect("failed to read");
        // .forge without marker should be preserved
        assert!(content.contains(".forge"));
        assert!(content.contains("target/"));
    }
}
