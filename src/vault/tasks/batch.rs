use std::{
    collections::BTreeMap,
    fs,
    ops::Range,
    path::{Path, PathBuf},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    TaskChanges, TaskDateEdit, TaskDestination, TaskRef, VaultManager, atomic_write,
    atomic_write_new, check_note_revision, insert_task, invalid, links, note_revision, parsed_at,
    read_text, rel_path,
    syntax::{Settings, eol, rewrite_task},
};
use crate::error::VaultError;

const MAX_OPERATIONS: usize = 50;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskBatchOperation {
    Move {
        reference: TaskRef,
        destination: TaskDestination,
    },
    Reschedule {
        reference: TaskRef,
        due: Option<TaskDateEdit>,
        scheduled: Option<TaskDateEdit>,
    },
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PreviewTaskBatchParams {
    pub vault: String,
    #[doc = "Up to 50 task moves or changes to due and scheduled dates."]
    pub operations: Vec<TaskBatchOperation>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ApplyTaskBatchParams {
    pub vault: String,
    pub operations: Vec<TaskBatchOperation>,
    #[doc = "Plan ID returned by preview_task_batch. Pass the same operations."]
    pub plan_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskBatchPreview {
    pub plan_id: String,
    pub files: Vec<String>,
    pub changes: Vec<TaskBatchChange>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskBatchChange {
    pub source: TaskRef,
    pub destination: String,
    pub heading: Option<String>,
    pub due: Option<TaskDateEdit>,
    pub scheduled: Option<TaskDateEdit>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskBatchOutput {
    pub files: Vec<TaskBatchFile>,
    pub completed: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskBatchState {
    Applied,
    Conflict,
    Failed,
    NotRun,
    CopiedOnly,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskBatchFile {
    pub path: String,
    pub state: TaskBatchState,
    pub reason: Option<String>,
}

struct PlannedFile {
    original: Option<String>,
    content: String,
    replacements: Vec<(Range<usize>, String)>,
    incoming: Vec<Incoming>,
}

struct Incoming {
    heading: Option<String>,
    block: String,
    cross_file: bool,
}

struct Patch {
    path: PathBuf,
    original: Option<String>,
    stage: Option<String>,
    final_content: String,
}

struct Plan {
    preview: TaskBatchPreview,
    files: Vec<Patch>,
}

impl VaultManager {
    pub fn preview_task_batch(
        &self,
        params: &PreviewTaskBatchParams,
    ) -> Result<TaskBatchPreview, VaultError> {
        self.plan_task_batch(&params.vault, &params.operations)
            .map(|plan| plan.preview)
    }

    pub fn apply_task_batch(
        &self,
        params: &ApplyTaskBatchParams,
    ) -> Result<TaskBatchOutput, VaultError> {
        let _guard = self.write_guard();

        let root = super::super::path::projected_path(self.resolve_vault(&params.vault)?)?;
        let plan = match self.plan_task_batch(&params.vault, &params.operations) {
            Ok(plan) => plan,
            Err(VaultError::RevisionConflict(path)) => {
                let path = rel_path(&root, Path::new(&path));
                return Ok(TaskBatchOutput {
                    files: vec![TaskBatchFile {
                        path,
                        state: TaskBatchState::Conflict,
                        reason: Some("A source note changed. List tasks and preview again.".into()),
                    }],
                    completed: false,
                });
            }
            Err(error) => return Err(error),
        };
        if plan.preview.plan_id != params.plan_id {
            return Ok(TaskBatchOutput {
                files: plan
                    .preview
                    .files
                    .into_iter()
                    .map(|path| TaskBatchFile {
                        path,
                        state: TaskBatchState::Conflict,
                        reason: Some("The preview is stale. Run preview_task_batch again.".into()),
                    })
                    .collect(),
                completed: false,
            });
        }

        let mut files: Vec<_> = plan
            .files
            .iter()
            .map(|patch| TaskBatchFile {
                path: rel_path(&root, &patch.path),
                state: TaskBatchState::NotRun,
                reason: None,
            })
            .collect();

        for (patch, result) in plan.files.iter().zip(files.iter_mut()) {
            if let Err(error) = verify_disk(&patch.path, patch.original.as_deref()) {
                result.state = TaskBatchState::Conflict;
                result.reason = Some(error.to_string());
                return Ok(TaskBatchOutput {
                    files,
                    completed: false,
                });
            }
        }

        for (patch, result) in plan.files.iter().zip(files.iter_mut()) {
            let Some(stage) = &patch.stage else { continue };
            if let Err(error) = publish(patch, patch.original.as_deref(), stage) {
                result.state = TaskBatchState::Failed;
                result.reason = Some(error.to_string());
                return Ok(TaskBatchOutput {
                    files,
                    completed: false,
                });
            }
            result.state = TaskBatchState::CopiedOnly;
            result.reason =
                Some("Incoming tasks were copied. Their original copies may remain.".into());
        }
        for (patch, result) in plan.files.iter().zip(files.iter_mut()) {
            let expected = patch.stage.as_deref().or(patch.original.as_deref());
            if expected != Some(patch.final_content.as_str())
                && let Err(error) = publish(patch, expected, &patch.final_content)
            {
                if patch.stage.is_none() {
                    result.state = TaskBatchState::Failed;
                }
                result.reason = Some(error.to_string());
                return Ok(TaskBatchOutput {
                    files,
                    completed: false,
                });
            }
            result.state = TaskBatchState::Applied;
            result.reason = None;
        }
        Ok(TaskBatchOutput {
            files,
            completed: true,
        })
    }

    fn plan_task_batch(
        &self,
        vault: &str,
        operations: &[TaskBatchOperation],
    ) -> Result<Plan, VaultError> {
        if operations.is_empty() || operations.len() > MAX_OPERATIONS {
            return Err(invalid(format!(
                "Select 1 to {MAX_OPERATIONS} task changes per batch."
            )));
        }
        let root = super::super::path::projected_path(self.resolve_vault(vault)?)?;
        let settings = Settings::read(&root)?;
        let mut files = BTreeMap::new();
        let mut changes = Vec::new();
        let mut warnings = Vec::new();
        for operation in operations {
            let target = match operation {
                TaskBatchOperation::Move { reference, .. }
                | TaskBatchOperation::Reschedule { reference, .. } => reference,
            };
            let source = self.note_path(vault, &target.path, None)?;
            self.ensure_write_allowed(vault, &source)?;
            let source = canonical_file(&source)?;
            let file = load_file(&mut files, &source, None)?;
            check_note_revision(&source, file.content.as_bytes(), &target.revision)?;
            let task = parsed_at(&file.content, target.line)?;
            if !settings.includes(task.body) {
                return Err(invalid(
                    "A source task is outside the Obsidian Tasks global filter.",
                ));
            }
            match operation {
                TaskBatchOperation::Move { destination, .. } => {
                    let (path, heading, seed) = self.task_destination(vault, destination)?;
                    self.ensure_write_allowed(vault, &path)?;
                    let path = super::super::path::projected_path(&path)?;
                    let range = task.span.start..task.child_end;
                    let block = dedent(&file.content[range.clone()], task.indentation);
                    if !links::parse_links(&block).is_empty() {
                        warnings.push(format!(
                            "Review links after moving {} at line {}.",
                            target.path, target.line
                        ));
                    }
                    file.replacements.push((range, String::new()));
                    let same_file = source == path;
                    let destination = load_file(&mut files, &path, seed)?;
                    insert_task(&destination.content, heading, &block)?;
                    destination.incoming.push(Incoming {
                        heading: heading.map(str::to_owned),
                        block,
                        cross_file: !same_file,
                    });
                    changes.push(TaskBatchChange {
                        source: target.clone(),
                        destination: rel_path(&root, &path),
                        heading: heading.map(str::to_owned),
                        due: None,
                        scheduled: None,
                    });
                }
                TaskBatchOperation::Reschedule { due, scheduled, .. } => {
                    if due.is_none() && scheduled.is_none() {
                        return Err(invalid(
                            "A reschedule operation needs a due or scheduled change.",
                        ));
                    }
                    let edit = TaskChanges {
                        due: due.clone(),
                        scheduled: scheduled.clone(),
                        ..TaskChanges::default()
                    };
                    let (updated, _, _) = rewrite_task(
                        &file.content,
                        &task,
                        &edit,
                        &settings,
                        super::periodic::today(),
                    )?;
                    let end = updated.len() - (file.content.len() - task.span.end);
                    let replacement = updated[task.span.start..end].to_owned();
                    file.replacements.push((task.span, replacement));
                    changes.push(TaskBatchChange {
                        source: target.clone(),
                        destination: rel_path(&root, &source),
                        heading: None,
                        due: due.clone(),
                        scheduled: scheduled.clone(),
                    });
                }
            }
        }
        let files: Vec<_> = files
            .into_iter()
            .map(|(path, file)| assemble(path, file))
            .collect::<Result<_, _>>()?;
        let mut fingerprint = serde_json::to_vec(&(vault, operations))
            .map_err(|e| VaultError::Search(e.to_string()))?;
        let mut paths = Vec::new();
        for file in &files {
            let path = rel_path(&root, &file.path);
            fingerprint.extend(
                serde_json::to_vec(&(
                    &path,
                    file.original.as_ref().map(|s| note_revision(s.as_bytes())),
                    file.stage.as_ref().map(|s| note_revision(s.as_bytes())),
                    note_revision(file.final_content.as_bytes()),
                ))
                .map_err(|e| VaultError::Search(e.to_string()))?,
            );
            paths.push(path);
        }
        Ok(Plan {
            preview: TaskBatchPreview {
                plan_id: note_revision(&fingerprint),
                files: paths,
                changes,
                warnings,
            },
            files,
        })
    }
}

fn canonical_file(file: &Path) -> Result<PathBuf, VaultError> {
    fs::canonicalize(file).map_err(|error| VaultError::io(file.display().to_string(), error))
}

fn load_file<'a>(
    files: &'a mut BTreeMap<PathBuf, PlannedFile>,
    path: &Path,
    seed: Option<String>,
) -> Result<&'a mut PlannedFile, VaultError> {
    match files.entry(path.to_owned()) {
        std::collections::btree_map::Entry::Occupied(entry) => Ok(entry.into_mut()),
        std::collections::btree_map::Entry::Vacant(entry) => {
            let (original, content) = match read_text(path) {
                Ok(old) => (Some(old.clone()), old),
                Err(VaultError::Io(_, error))
                    if error.kind() == std::io::ErrorKind::NotFound && seed.is_some() =>
                {
                    (None, seed.unwrap_or_default())
                }
                Err(error) => return Err(error),
            };
            Ok(entry.insert(PlannedFile {
                original,
                content,
                replacements: Vec::new(),
                incoming: Vec::new(),
            }))
        }
    }
}

fn assemble(path: PathBuf, mut plan: PlannedFile) -> Result<Patch, VaultError> {
    plan.replacements.sort_by_key(|(range, _)| range.start);
    if plan
        .replacements
        .windows(2)
        .any(|edits| edits[0].0.end > edits[1].0.start)
    {
        return Err(invalid(format!(
            "Overlapping task edits in '{}'.",
            path.display()
        )));
    }
    let mut stage =
        (!plan.incoming.iter().all(|item| !item.cross_file)).then(|| plan.content.clone());
    let mut final_content = plan.content;
    for (range, replacement) in plan.replacements.into_iter().rev() {
        final_content.replace_range(range, &replacement);
    }
    for incoming in plan.incoming {
        if incoming.cross_file
            && let Some(stage) = &mut stage
        {
            *stage = insert_task(stage, incoming.heading.as_deref(), &incoming.block)?.0;
        }
        final_content =
            insert_task(&final_content, incoming.heading.as_deref(), &incoming.block)?.0;
    }
    Ok(Patch {
        path,
        original: plan.original,
        stage,
        final_content,
    })
}

fn dedent(content: &str, indentation: &str) -> String {
    let newline = eol(content);
    content
        .lines()
        .map(|line| line.strip_prefix(indentation).unwrap_or(line))
        .collect::<Vec<_>>()
        .join(newline)
        .trim_end_matches(['\r', '\n'])
        .to_owned()
}

fn verify_disk(path: &Path, expected: Option<&str>) -> Result<(), VaultError> {
    let actual = read_text(path);
    match (actual, expected) {
        (Ok(actual), Some(expected)) if actual == expected => Ok(()),
        (Err(VaultError::Io(_, error)), None) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(())
        }
        (Err(error), _) => Err(error),
        _ => Err(invalid(format!(
            "'{}' changed after preview. Preview the batch again.",
            path.display()
        ))),
    }
}

fn publish(patch: &Patch, expected: Option<&str>, next: &str) -> Result<(), VaultError> {
    verify_disk(&patch.path, expected)?;
    if expected.is_some() {
        return atomic_write(&patch.path, next.as_bytes());
    }
    if let Some(parent) = patch.path.parent() {
        fs::create_dir_all(parent).map_err(|e| VaultError::io(parent.display().to_string(), e))?;
    }
    atomic_write_new(&patch.path, next.as_bytes())
}
