use eyre::{Result, eyre};
use include_dir::{Dir, File, include_dir};
use log::debug;
use std::fs;

static EXAMPLES: Dir = include_dir!("$CARGO_MANIFEST_DIR/examples");

/// Recursively collect all files from an embedded directory.
fn collect_files<'a>(dir: &'a Dir<'a>, files: &mut Vec<&'a File<'a>>) {
    for file in dir.files() {
        files.push(file);
    }
    for subdir in dir.dirs() {
        collect_files(subdir, files);
    }
}

#[cfg(test)]
fn write_examples(target: &std::path::Path, force: bool) -> Result<(usize, usize)> {
    let mut files = Vec::new();
    collect_files(&EXAMPLES, &mut files);

    let mut written = 0;
    let mut skipped = 0;

    for file in files {
        let dest = target.join(file.path());
        if dest.exists() && !force {
            skipped += 1;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest, file.contents())?;
        written += 1;
    }

    Ok((written, skipped))
}

pub fn init(force: bool) -> Result<()> {
    debug!("init: force={}", force);
    let config_dir = dirs::config_dir()
        .ok_or_else(|| eyre!("cannot determine config directory"))?
        .join("forge");

    let mut files = Vec::new();
    collect_files(&EXAMPLES, &mut files);

    let mut written = 0;
    let mut skipped = 0;

    for file in files {
        let dest = config_dir.join(file.path());
        if dest.exists() && !force {
            println!("Skipped {} (already exists)", dest.display());
            skipped += 1;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest, file.contents())?;
        println!("Created {}", dest.display());
        written += 1;
    }

    if written == 0 && skipped == 0 {
        println!("Warning: no embedded example files found");
    } else if written == 0 {
        println!("\nAll {} files already exist. Use --force to overwrite.", skipped);
    } else {
        println!("\nInitialized {} files in {}", written, config_dir.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_examples_embedded() {
        assert!(EXAMPLES.get_file("forge.yml").is_some());
        assert!(EXAMPLES.get_file("pipelines/research.yml").is_some());
        assert!(EXAMPLES.get_file("pipelines/techspec.yml").is_some());
        assert!(EXAMPLES.get_file("references/voice.md").is_some());
        assert!(EXAMPLES.get_file("references/templates/techspec.md").is_some());
        assert!(EXAMPLES.get_file("references/rubrics/techspec-rubric.md").is_some());
    }

    #[test]
    fn test_init_writes_files() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let (written, skipped) = write_examples(dir.path(), false).expect("init failed");

        assert!(written > 0);
        assert_eq!(skipped, 0);
        assert!(dir.path().join("forge.yml").exists());
        assert!(dir.path().join("pipelines/research.yml").exists());
        assert!(dir.path().join("pipelines/techspec.yml").exists());
        assert!(dir.path().join("references/voice.md").exists());
        assert!(dir.path().join("references/templates/techspec.md").exists());
        assert!(dir.path().join("references/rubrics/techspec-rubric.md").exists());
    }

    #[test]
    fn test_init_skips_existing() {
        let dir = TempDir::new().expect("failed to create temp dir");
        write_examples(dir.path(), false).expect("first init failed");

        // Write custom content to forge.yml
        let forge_yml = dir.path().join("forge.yml");
        fs::write(&forge_yml, "custom content").expect("failed to write");

        // Run again without force
        let (_, skipped) = write_examples(dir.path(), false).expect("second init failed");

        assert!(skipped > 0);
        // Custom content should be preserved
        let content = fs::read_to_string(&forge_yml).expect("failed to read");
        assert_eq!(content, "custom content");
    }

    #[test]
    fn test_init_force_overwrites() {
        let dir = TempDir::new().expect("failed to create temp dir");
        write_examples(dir.path(), false).expect("first init failed");

        // Write custom content to forge.yml
        let forge_yml = dir.path().join("forge.yml");
        fs::write(&forge_yml, "custom content").expect("failed to write");

        // Run again with force
        let (written, _) = write_examples(dir.path(), true).expect("force init failed");

        assert!(written > 0);
        // Custom content should be overwritten
        let content = fs::read_to_string(&forge_yml).expect("failed to read");
        assert_ne!(content, "custom content");
        assert!(content.contains("forge:"));
    }
}
