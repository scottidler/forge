use eyre::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use taskstore::{IndexValue, Record};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: String,
    pub pipeline: String,
    pub working_dir: String,
    pub status: RunStatus,
    pub current_stage: usize,
    pub created_at: i64,
    pub updated_at: i64,
    pub input: Option<String>,
    pub slug: Option<String>,
    pub stages: Vec<StageRecord>,
    pub final_destination: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RunStatus {
    Unpacked,
    InProgress,
    Packed,
    Completed,
    Abandoned,
}

impl fmt::Display for RunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunStatus::Unpacked => write!(f, "Unpacked"),
            RunStatus::InProgress => write!(f, "InProgress"),
            RunStatus::Packed => write!(f, "Packed"),
            RunStatus::Completed => write!(f, "Completed"),
            RunStatus::Abandoned => write!(f, "Abandoned"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageRecord {
    pub name: String,
    pub status: StageStatus,
    pub artifact_path: Option<String>,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StageStatus {
    Pending,
    InProgress,
    Review,
    Completed,
    Skipped,
}

impl fmt::Display for StageStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StageStatus::Pending => write!(f, "Pending"),
            StageStatus::InProgress => write!(f, "InProgress"),
            StageStatus::Review => write!(f, "Review"),
            StageStatus::Completed => write!(f, "Completed"),
            StageStatus::Skipped => write!(f, "Skipped"),
        }
    }
}

impl Record for PipelineRun {
    fn id(&self) -> &str {
        &self.id
    }

    fn updated_at(&self) -> i64 {
        self.updated_at
    }

    fn collection_name() -> &'static str {
        "pipeline_runs"
    }

    fn indexed_fields(&self) -> HashMap<String, IndexValue> {
        let mut fields = HashMap::new();
        fields.insert("pipeline".to_string(), IndexValue::String(self.pipeline.clone()));
        fields.insert("working_dir".to_string(), IndexValue::String(self.working_dir.clone()));
        fields.insert("status".to_string(), IndexValue::String(self.status.to_string()));
        fields.insert("current_stage".to_string(), IndexValue::Int(self.current_stage as i64));
        fields
    }
}

impl PipelineRun {
    pub fn new(
        pipeline: String,
        working_dir: String,
        input: Option<String>,
        slug: Option<String>,
        stage_names: Vec<String>,
    ) -> Self {
        let now = taskstore::now_ms();
        let id = uuid::Uuid::now_v7().to_string();
        let stages = stage_names
            .into_iter()
            .map(|name| StageRecord {
                name,
                status: StageStatus::Pending,
                artifact_path: None,
                started_at: None,
                completed_at: None,
            })
            .collect();
        Self {
            id,
            pipeline,
            working_dir,
            status: RunStatus::Unpacked,
            current_stage: 0,
            created_at: now,
            updated_at: now,
            input,
            slug,
            stages,
            final_destination: None,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = taskstore::now_ms();
    }
}

/// Open the forge TaskStore at the configured store directory
pub fn open_store(store_dir: &std::path::Path) -> Result<taskstore::Store> {
    let store = taskstore::Store::open(store_dir)?;
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_run_new() {
        let run = PipelineRun::new(
            "techspec".to_string(),
            "/tmp/test".to_string(),
            Some("input.md".to_string()),
            Some("my-slug".to_string()),
            vec!["research".to_string(), "outline".to_string(), "draft".to_string()],
        );
        assert_eq!(run.pipeline, "techspec");
        assert_eq!(run.working_dir, "/tmp/test");
        assert_eq!(run.status, RunStatus::Unpacked);
        assert_eq!(run.current_stage, 0);
        assert_eq!(run.stages.len(), 3);
        assert_eq!(run.stages[0].name, "research");
        assert_eq!(run.stages[0].status, StageStatus::Pending);
        assert!(run.slug.is_some());
    }

    #[test]
    fn test_indexed_fields() {
        let run = PipelineRun::new(
            "techspec".to_string(),
            "/tmp/test".to_string(),
            None,
            None,
            vec!["research".to_string()],
        );
        let fields = run.indexed_fields();
        assert_eq!(fields.len(), 4);
        assert!(fields.contains_key("pipeline"));
        assert!(fields.contains_key("working_dir"));
        assert!(fields.contains_key("status"));
        assert!(fields.contains_key("current_stage"));
    }

    #[test]
    fn test_touch() {
        let mut run = PipelineRun::new(
            "test".to_string(),
            "/tmp".to_string(),
            None,
            None,
            vec!["s1".to_string()],
        );
        let before = run.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(2));
        run.touch();
        assert!(run.updated_at >= before);
    }

    #[test]
    fn test_run_status_display() {
        assert_eq!(RunStatus::Unpacked.to_string(), "Unpacked");
        assert_eq!(RunStatus::InProgress.to_string(), "InProgress");
        assert_eq!(RunStatus::Completed.to_string(), "Completed");
    }

    #[test]
    fn test_stage_status_display() {
        assert_eq!(StageStatus::Pending.to_string(), "Pending");
        assert_eq!(StageStatus::Review.to_string(), "Review");
        assert_eq!(StageStatus::Completed.to_string(), "Completed");
    }
}
