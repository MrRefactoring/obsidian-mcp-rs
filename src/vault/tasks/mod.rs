mod batch;
mod syntax;

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use chrono::NaiveDate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::VaultError;

use super::{
    Period, VaultManager, check_note_revision, links, md_files, note_revision, patch, periodic,
    rel_path, safe_join,
    write::{atomic_write, atomic_write_new},
};
use syntax::{ParsedTask, Settings, parse_tasks};

pub use batch::{ApplyTaskBatchParams, PreviewTaskBatchParams, TaskBatchOutput, TaskBatchPreview};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskScope {
    Note { path: String },
    Folder { path: String },
    Today,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    Done,
    InProgress,
    OnHold,
    Cancelled,
    NonTask,
    Unknown,
}

impl TaskStatus {
    fn finished(self) -> bool {
        matches!(self, Self::Done | Self::Cancelled | Self::NonTask)
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Highest,
    High,
    Medium,
    Normal,
    Low,
    Lowest,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskDateField {
    Due,
    Start,
    Scheduled,
    Done,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "match", rename_all = "snake_case")]
pub enum TaskDateMatch {
    Missing,
    Overdue,
    Today,
    Range {
        from: Option<String>,
        through: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TaskDateFilter {
    pub field: TaskDateField,
    #[serde(flatten)]
    pub date_match: TaskDateMatch,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskSort {
    #[default]
    Source,
    Due,
    Priority,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTasksParams {
    pub vault: String,
    #[doc = "Choose a note, a recursive folder, or today's daily note. Omit for the whole vault."]
    pub scope: Option<TaskScope>,
    pub status: Option<TaskStatus>,
    pub date: Option<TaskDateFilter>,
    #[doc = "Date for relative filters in YYYY-MM-DD format. Defaults to today's local date."]
    pub reference_date: Option<String>,
    pub tag: Option<String>,
    pub priority: Option<TaskPriority>,
    pub blocked: Option<bool>,
    #[serde(default)]
    pub sort: TaskSort,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
pub struct TaskRef {
    pub path: String,
    #[doc = "One-based line number from list_tasks."]
    pub line: usize,
    #[doc = "Note revision returned by list_tasks."]
    pub revision: String,
}

#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
pub struct TaskDates {
    pub due: Option<String>,
    pub start: Option<String>,
    pub scheduled: Option<String>,
    pub done: Option<String>,
}

impl TaskDates {
    fn get(&self, field: TaskDateField) -> Option<&str> {
        match field {
            TaskDateField::Due => self.due.as_deref(),
            TaskDateField::Start => self.start.as_deref(),
            TaskDateField::Scheduled => self.scheduled.as_deref(),
            TaskDateField::Done => self.done.as_deref(),
        }
    }
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TaskItem {
    pub reference: TaskRef,
    pub heading: Option<String>,
    pub text: String,
    pub text_truncated: bool,
    pub details_truncated: bool,
    pub symbol: char,
    pub status: TaskStatus,
    pub status_name: Option<String>,
    pub dates: TaskDates,
    pub priority: TaskPriority,
    pub tags: Vec<String>,
    pub recurrence: Option<String>,
    pub id: Option<String>,
    pub dependencies: Vec<String>,
    pub blockers: Vec<TaskRef>,
    pub dependency_issues: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListTasksOutput {
    pub tasks: Vec<TaskItem>,
    pub total: usize,
    pub next_offset: Option<usize>,
    pub reference_date: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskDestination {
    Note {
        path: String,
        heading: Option<String>,
    },
    Daily {
        date: Option<String>,
        heading: Option<String>,
    },
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateTaskParams {
    pub vault: String,
    pub destination: TaskDestination,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SetTaskStatus {
    Open,
    Completed,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TaskDateEdit {
    Set { date: String },
    Clear,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct TaskChanges {
    pub text: Option<String>,
    pub status: Option<SetTaskStatus>,
    pub due: Option<TaskDateEdit>,
    pub scheduled: Option<TaskDateEdit>,
    #[doc = "Use normal to remove the priority marker."]
    pub priority: Option<TaskPriority>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateTaskParams {
    pub vault: String,
    pub reference: TaskRef,
    pub changes: TaskChanges,
    #[doc = "Completion date for deterministic recurrence in YYYY-MM-DD format. Defaults to today."]
    pub reference_date: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskWriteOutput {
    pub task: TaskItem,
    pub next_occurrence: Option<TaskItem>,
    pub changed: bool,
}

impl VaultManager {
    pub fn list_tasks(&self, params: &ListTasksParams) -> Result<ListTasksOutput, VaultError> {
        let root = self.resolve_vault(&params.vault)?;
        let today = parse_reference_date(params.reference_date.as_deref())?;
        let selector = match &params.scope {
            Some(TaskScope::Note { path }) => {
                let file = self.note_path(&params.vault, path, None)?;
                if !file.is_file() {
                    return Err(VaultError::NoteNotFound(path.clone(), params.vault.clone()));
                }
                Some(file)
            }
            Some(TaskScope::Folder { path }) => {
                let folder = safe_join(root, None, path)?;
                if !folder.is_dir() {
                    return Err(invalid(format!("Task folder '{path}' does not exist.")));
                }
                Some(folder)
            }
            Some(TaskScope::Today) => {
                let daily = periodic::resolve(root, Period::Daily, today)?;
                Some(self.note_path(&params.vault, &daily.filename, periodic::folder_of(&daily))?)
            }
            None => None,
        };

        let selector = selector
            .map(|path| {
                super::path::normalize_relative(path.strip_prefix(root).unwrap_or(&path))
                    .map(|relative| root.join(relative))
            })
            .transpose()?;
        let settings = Settings::read(root)?;
        let date_filter = DateFilter::build(params.date.as_ref(), today)?;
        let scoped_note = matches!(
            params.scope,
            Some(TaskScope::Note { .. } | TaskScope::Today)
        );
        let in_scope = |path: &Path| {
            selector.as_ref().is_none_or(|scope| {
                if scoped_note {
                    path == scope
                } else {
                    path.starts_with(scope)
                }
            })
        };
        let mut items = if scoped_note {
            collect_task_items(
                root,
                selector
                    .iter()
                    .filter(|path| path.is_file())
                    .cloned()
                    .collect(),
                &settings,
            )?
        } else {
            collect_task_items(
                root,
                md_files(root)
                    .into_iter()
                    .filter(|path| in_scope(path))
                    .collect(),
                &settings,
            )?
        };
        if selector.is_some()
            && items
                .iter()
                .any(|task| task.id.is_some() || !task.dependencies.is_empty())
        {
            items.extend(collect_task_items(
                root,
                md_files(root)
                    .into_iter()
                    .filter(|path| !in_scope(path))
                    .collect(),
                &settings,
            )?);
        }
        let index = dependency_index(&items);
        let mut selected = Vec::new();
        let wanted_tag = params.tag.as_deref().map(super::tags::normalize_tag);
        for item in &items {
            let path = root.join(&item.reference.path);
            if !in_scope(&path)
                || params.status.is_some_and(|status| item.status != status)
                || params
                    .priority
                    .is_some_and(|priority| item.priority != priority)
                || wanted_tag.as_ref().is_some_and(|tag| {
                    !item
                        .tags
                        .iter()
                        .any(|candidate| super::tags::normalize_tag(candidate) == *tag)
                })
                || !date_filter.matches(item)
            {
                continue;
            }
            let mut item = item.clone();
            add_dependencies(&mut item, &items, &index);
            if params
                .blocked
                .is_some_and(|wanted| item.blockers.is_empty() == wanted)
            {
                continue;
            }
            selected.push(item);
        }
        match params.sort {
            TaskSort::Source => {}
            TaskSort::Due => selected.sort_by(|a, b| {
                a.dates
                    .due
                    .as_ref()
                    .map_or((true, ""), |v| (false, v))
                    .cmp(&b.dates.due.as_ref().map_or((true, ""), |v| (false, v)))
            }),
            TaskSort::Priority => selected.sort_by_key(|item| item.priority),
        }
        let total = selected.len();
        let offset = params.offset.unwrap_or(0).min(total);
        let limit = params.limit.unwrap_or(25).clamp(1, 100);
        let mut tasks = Vec::new();
        let mut bytes = 0;
        for mut task in selected.into_iter().skip(offset).take(limit) {
            let len = serde_json::to_vec(&task)
                .map_err(|e| VaultError::Search(e.to_string()))?
                .len();
            if bytes + len > 40_000 && !tasks.is_empty() {
                break;
            }
            if len > 40_000 {
                task.blockers.truncate(25);
                task.dependency_issues.truncate(25);
                task.tags.truncate(50);
                task.dependencies.truncate(50);
                task.details_truncated = true;
                let task_len = serde_json::to_vec(&task)
                    .map_err(|e| VaultError::Search(e.to_string()))?
                    .len();
                if task_len > 40_000 {
                    task.blockers.clear();
                    task.dependency_issues.clear();
                }
            }
            bytes += len;
            tasks.push(task);
        }
        let returned_through = offset + tasks.len();
        Ok(ListTasksOutput {
            tasks,
            total,
            next_offset: (returned_through < total).then_some(returned_through),
            reference_date: today.to_string(),
        })
    }

    pub fn create_task(&self, params: &CreateTaskParams) -> Result<TaskWriteOutput, VaultError> {
        let _guard = self.write_guard();
        validate_text(&params.text)?;
        let root = self.resolve_vault(&params.vault)?;
        let settings = Settings::read(root)?;
        let (path, heading, seed) = self.task_destination(&params.vault, &params.destination)?;
        self.ensure_write_allowed(&params.vault, &path)?;
        let (old, was_missing) = match read_text(&path) {
            Ok(content) => (content, false),
            Err(VaultError::Io(_, error)) if error.kind() == std::io::ErrorKind::NotFound => (
                seed.ok_or_else(|| {
                    VaultError::NoteNotFound(rel_path(root, &path), params.vault.clone())
                })?,
                true,
            ),
            Err(error) => return Err(error),
        };
        let text = settings.create_text(params.text.trim());
        let new_line = format!("- [{}] {text}", settings.open_symbol()?);
        let (updated, line) = insert_task(&old, heading, &new_line)?;
        let task = parsed_at(&updated, line)?.item(
            &rel_path(root, &path),
            &note_revision(updated.as_bytes()),
            &settings,
        );
        if was_missing {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| VaultError::io(parent.display().to_string(), e))?;
            }
            atomic_write_new(&path, updated.as_bytes())?;
        } else {
            atomic_write(&path, updated.as_bytes())?;
        }
        Ok(TaskWriteOutput {
            task,
            next_occurrence: None,
            changed: true,
        })
    }

    pub fn update_task(&self, params: &UpdateTaskParams) -> Result<TaskWriteOutput, VaultError> {
        let _guard = self.write_guard();
        let root = self.resolve_vault(&params.vault)?;
        let settings = Settings::read(root)?;
        let path = self.note_path(&params.vault, &params.reference.path, None)?;
        self.ensure_write_allowed(&params.vault, &path)?;
        let content = read_text(&path)?;
        check_note_revision(&path, content.as_bytes(), &params.reference.revision)?;
        let task = parsed_at(&content, params.reference.line)?;
        if !settings.includes(task.body) {
            return Err(invalid(
                "This line is outside the Obsidian Tasks global filter.",
            ));
        }
        let today = parse_reference_date(params.reference_date.as_deref())?;
        let (updated, line, next_line) =
            syntax::rewrite_task(&content, &task, &params.changes, &settings, today)?;
        let path_str = rel_path(root, &path);
        let revision = note_revision(updated.as_bytes());
        let task = parsed_at(&updated, line)?.item(&path_str, &revision, &settings);
        let next_occurrence = next_line
            .map(|line| {
                parsed_at(&updated, line).map(|next| next.item(&path_str, &revision, &settings))
            })
            .transpose()?;
        let changed = updated != content;
        if changed {
            atomic_write(&path, updated.as_bytes())?;
        }
        Ok(TaskWriteOutput {
            task,
            next_occurrence,
            changed,
        })
    }

    fn task_destination<'a>(
        &self,
        vault: &str,
        destination: &'a TaskDestination,
    ) -> Result<(PathBuf, Option<&'a str>, Option<String>), VaultError> {
        match destination {
            TaskDestination::Note { path, heading } => {
                Ok((self.note_path(vault, path, None)?, heading.as_deref(), None))
            }
            TaskDestination::Daily { date, heading } => {
                let root = self.resolve_vault(vault)?;
                let resolved =
                    periodic::resolve(root, Period::Daily, parse_reference_date(date.as_deref())?)?;
                let path =
                    self.note_path(vault, &resolved.filename, periodic::folder_of(&resolved))?;
                Ok((path, heading.as_deref(), Some(resolved.seed)))
            }
        }
    }
}

fn collect_task_items(
    root: &Path,
    mut files: Vec<PathBuf>,
    settings: &Settings,
) -> Result<Vec<TaskItem>, VaultError> {
    files.sort();
    let mut items = Vec::new();
    for file in files {
        let content = read_text(&file)?;
        let path = rel_path(root, &file);
        let revision = note_revision(content.as_bytes());
        items.extend(
            parse_tasks(&content)
                .into_iter()
                .filter(|task| settings.includes(task.body))
                .map(|task| task.item(&path, &revision, settings)),
        );
    }
    Ok(items)
}

fn read_text(path: &Path) -> Result<String, VaultError> {
    fs::read_to_string(path).map_err(|e| VaultError::io(path.display().to_string(), e))
}

fn invalid(message: impl Into<String>) -> VaultError {
    VaultError::MissingArgument(message.into())
}

fn parse_reference_date(date: Option<&str>) -> Result<NaiveDate, VaultError> {
    date.map_or_else(|| Ok(periodic::today()), periodic::parse_date)
}

fn validate_text(text: &str) -> Result<(), VaultError> {
    if text.trim().is_empty() || text.contains(['\r', '\n', '\0']) {
        return Err(invalid("Task text must be a non-empty single line."));
    }
    Ok(())
}

fn parsed_at(content: &str, line: usize) -> Result<ParsedTask<'_>, VaultError> {
    parse_tasks(content)
        .into_iter()
        .find(|task| task.line == line)
        .ok_or_else(|| invalid(format!("No task at line {line}. List tasks again.")))
}

fn insert_task(
    content: &str,
    heading: Option<&str>,
    block: &str,
) -> Result<(String, usize), VaultError> {
    let position = match heading {
        None => content.len(),
        Some(heading) => {
            let headings = patch::headings(content);
            let wanted = heading.trim();
            let level = wanted.bytes().take_while(|b| *b == b'#').count();
            let wanted = wanted[level..].trim();
            let mut matches = headings.iter().enumerate().filter(|(_, h)| {
                h.text.eq_ignore_ascii_case(wanted) && (level == 0 || h.level == level)
            });
            let (index, matched) = matches
                .next()
                .ok_or_else(|| invalid(format!("Heading '{heading}' was not found.")))?;
            if matches.next().is_some() {
                return Err(invalid(format!("Heading '{heading}' is ambiguous.")));
            }
            headings[index + 1..]
                .iter()
                .find(|h| h.level <= matched.level)
                .map_or(content.len(), |h| h.start)
        }
    };
    let (prefix, suffix) = content.split_at(position);
    let line_end = syntax::eol(content);
    let break_before = if prefix.is_empty() || prefix.ends_with('\n') {
        ""
    } else {
        line_end
    };
    let line =
        prefix.bytes().filter(|b| *b == b'\n').count() + 1 + usize::from(!break_before.is_empty());
    let inserted = format!("{prefix}{break_before}{block}{line_end}{suffix}");
    parsed_at(&inserted, line).map_err(|_| {
        invalid("The task destination does not render as a task. Check for an unclosed code fence.")
    })?;
    Ok((inserted, line))
}

struct DateFilter {
    field: TaskDateField,
    from: Option<NaiveDate>,
    through: Option<NaiveDate>,
    missing: bool,
    skip_finished: bool,
}

impl DateFilter {
    fn build(filter: Option<&TaskDateFilter>, today: NaiveDate) -> Result<Self, VaultError> {
        let Some(filter) = filter else {
            return Ok(Self {
                field: TaskDateField::Due,
                from: None,
                through: None,
                missing: false,
                skip_finished: false,
            });
        };
        let (from, through, missing, skip_finished) = match &filter.date_match {
            TaskDateMatch::Missing => (None, None, true, false),
            TaskDateMatch::Overdue => (None, today.pred_opt(), false, true),
            TaskDateMatch::Today => (Some(today), Some(today), false, false),
            TaskDateMatch::Range { from, through } => (
                from.as_deref().map(periodic::parse_date).transpose()?,
                through.as_deref().map(periodic::parse_date).transpose()?,
                false,
                false,
            ),
        };
        if let (Some(from), Some(through)) = (from, through)
            && from > through
        {
            return Err(invalid("The task date range begins after it ends."));
        }
        Ok(Self {
            field: filter.field,
            from,
            through,
            missing,
            skip_finished,
        })
    }

    fn matches(&self, item: &TaskItem) -> bool {
        if self.skip_finished && item.status.finished() {
            return false;
        }
        if self.missing {
            return item.dates.get(self.field).is_none();
        }
        if self.from.is_none() && self.through.is_none() {
            return true;
        }
        item.dates
            .get(self.field)
            .and_then(|date| periodic::parse_date(date).ok())
            .is_some_and(|date| {
                self.from.is_none_or(|from| date >= from)
                    && self.through.is_none_or(|through| date <= through)
            })
    }
}

fn dependency_index(items: &[TaskItem]) -> HashMap<&str, Vec<usize>> {
    let mut index: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        if let Some(id) = item.id.as_deref() {
            index.entry(id).or_default().push(i);
        }
    }
    index
}

fn add_dependencies(item: &mut TaskItem, all: &[TaskItem], index: &HashMap<&str, Vec<usize>>) {
    if let Some(id) = item.id.as_deref()
        && index.get(id).is_some_and(|set| set.len() > 1)
    {
        item.dependency_issues
            .push(format!("Duplicate task ID '{id}'."));
    }
    for id in &item.dependencies {
        match index.get(id.as_str()).map(Vec::as_slice) {
            Some([position]) => {
                let dependency = &all[*position];
                if !item.status.finished() && !dependency.status.finished() {
                    item.blockers.push(dependency.reference.clone());
                }
            }
            Some(_) => item
                .dependency_issues
                .push(format!("Dependency ID '{id}' is ambiguous.")),
            None => item
                .dependency_issues
                .push(format!("Dependency ID '{id}' was not found.")),
        }
    }
    if let Some(id) = item.id.as_deref() {
        let mut pending: Vec<_> = item.dependencies.iter().map(String::as_str).collect();
        let mut seen = HashSet::new();
        while let Some(next) = pending.pop() {
            if next == id {
                item.dependency_issues
                    .push(format!("Dependency cycle reaches task ID '{id}'."));
                break;
            }
            if seen.insert(next)
                && let Some([position]) = index.get(next).map(Vec::as_slice)
            {
                pending.extend(all[*position].dependencies.iter().map(String::as_str));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> T {
        serde_json::from_value(value).unwrap()
    }

    fn fixture() -> (tempfile::TempDir, VaultManager, String) {
        let dir = tempfile::tempdir().unwrap();
        let manager =
            VaultManager::with_lock_path(vec![dir.path().to_path_buf()], dir.path().join(".lock"));
        let vault = manager.list_vaults()[0].0.clone();
        (dir, manager, vault)
    }

    #[test]
    fn task_revision_guards_identical_descriptions_and_preserves_surrounding_text() {
        let (dir, manager, vault) = fixture();
        let path = dir.path().join("meeting.md");
        fs::write(&path, "---\r\nexample: |\r\n  - [ ] yaml\r\n---\r\n# Meeting\r\n```md\r\n- [ ] code\r\n```\r\n- [ ] act\r\n- [ ] act 🗓️ 2026-11-18 ^block\r\n  - detail\r\n").unwrap();
        let output = manager
            .list_tasks(&parse(serde_json::json!({"vault": vault})))
            .unwrap();
        assert_eq!(output.tasks.len(), 2);
        assert_eq!(output.tasks[1].heading.as_deref(), Some("Meeting"));
        assert_eq!(output.tasks[1].dates.due.as_deref(), Some("2026-11-18"));
        let update = UpdateTaskParams {
            vault,
            reference: output.tasks[1].reference.clone(),
            changes: TaskChanges {
                status: Some(SetTaskStatus::Completed),
                ..TaskChanges::default()
            },
            reference_date: None,
        };
        let updated = manager.update_task(&update).unwrap();
        assert_eq!(updated.task.status, TaskStatus::Done);
        assert!(
            fs::read_to_string(path)
                .unwrap()
                .ends_with("- [ ] act\r\n- [x] act 🗓️ 2026-11-18 ^block\r\n  - detail\r\n")
        );
        assert!(manager.update_task(&update).is_err());
        let repeat = UpdateTaskParams {
            reference: updated.task.reference,
            ..update
        };
        assert!(!manager.update_task(&repeat).unwrap().changed);
    }

    #[test]
    fn task_edits_use_daily_template_and_reject_ambiguous_heading() {
        let (dir, manager, vault) = fixture();
        let config = dir.path().join(".obsidian");
        fs::create_dir(&config).unwrap();
        fs::write(
            config.join("daily-notes.json"),
            r#"{"folder":"Daily","template":"Template"}"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("Template.md"),
            "# Daily\n\n## Actions\n\n## Notes\n",
        )
        .unwrap();
        let list = parse(
            serde_json::json!({"vault":vault, "scope":{"kind":"today"}, "reference_date":"2026-11-18"}),
        );
        assert_eq!(manager.list_tasks(&list).unwrap().total, 0);
        assert!(!dir.path().join("Daily").exists());
        let create = parse(
            serde_json::json!({"vault":vault, "destination":{"kind":"daily", "date":"2026-11-18", "heading":"Actions"}, "text":"Send notes"}),
        );
        let created = manager.create_task(&create).unwrap();
        assert_eq!(created.task.heading.as_deref(), Some("Actions"));
        assert_eq!(manager.list_tasks(&list).unwrap().total, 1);
        fs::write(dir.path().join("duplicate.md"), "## Actions\n## Actions\n").unwrap();
        let bad = parse(
            serde_json::json!({"vault":vault, "destination":{"kind":"note", "path":"duplicate.md", "heading":"Actions"}, "text":"Not added"}),
        );
        assert!(manager.create_task(&bad).is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("duplicate.md")).unwrap(),
            "## Actions\n## Actions\n"
        );
    }

    #[test]
    fn task_recurrence_uses_settings_preserves_custom_status_and_is_single_write() {
        let (dir, manager, vault) = fixture();
        let plugin = dir.path().join(".obsidian/plugins/obsidian-tasks-plugin");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(plugin.join("data.json"), r##"{"globalFilter":"#task","recurrenceOnNextLine":true,"statusSettings":{"customStatuses":[{"symbol":"/","name":"Active","type":"IN_PROGRESS","nextStatusSymbol":"x"}]}}"##).unwrap();
        let source = dir.path().join("tasks.md");
        fs::write(&source, "- [/] Report #task ⏳ 2026-01-29 📅 2026-01-31 🔁 every month 🆔 A ^report\n  - keep child\n- [ ] excluded\n").unwrap();
        let output = manager.list_tasks(&parse(serde_json::json!({"vault":vault,"date":{"field":"due","match":"overdue"},"reference_date":"2026-02-01"}))).unwrap();
        assert_eq!(output.total, 1);
        assert_eq!(output.tasks[0].status_name.as_deref(), Some("Active"));
        let params = UpdateTaskParams {
            vault,
            reference: output.tasks[0].reference.clone(),
            changes: TaskChanges {
                status: Some(SetTaskStatus::Completed),
                ..TaskChanges::default()
            },
            reference_date: Some("2026-02-01".to_owned()),
        };
        let result = manager.update_task(&params).unwrap();
        let next = result.next_occurrence.unwrap();
        assert_eq!(next.dates.due.as_deref(), Some("2026-02-28"));
        assert_eq!(next.dates.scheduled.as_deref(), Some("2026-02-26"));
        assert_eq!(result.task.dates.done.as_deref(), Some("2026-02-01"));
        assert!(next.id.is_none());
        let text = fs::read_to_string(source).unwrap();
        assert!(text.find("keep child").unwrap() < text.find("2026-02-28").unwrap());
        assert_eq!(text.matches("^report").count(), 1);
        assert!(manager.update_task(&params).is_err());
    }

    #[test]
    fn task_batch_preview_precedes_cross_note_move_and_rejects_a_stale_destination() {
        let (dir, manager, vault) = fixture();
        let from = dir.path().join("inbox.md");
        let to = dir.path().join("project.md");
        fs::write(
            &from,
            "# Inbox\n- [ ] Move me\n  - keep child\n- [ ] Stay\n",
        )
        .unwrap();
        fs::write(&to, "# Project\n## Actions\n").unwrap();
        let list = manager
            .list_tasks(&parse(
                serde_json::json!({"vault":vault,"scope":{"kind":"note","path":"inbox.md"}}),
            ))
            .unwrap();
        let preview_params: PreviewTaskBatchParams = parse(serde_json::json!({
            "vault":vault,"operations":[{"kind":"move","reference":list.tasks[0].reference,
            "destination":{"kind":"note","path":"project.md","heading":"Actions"}}]
        }));
        let first = manager.preview_task_batch(&preview_params).unwrap();
        fs::write(&to, "# Project\nUpdated\n## Actions\n").unwrap();
        let mut apply = ApplyTaskBatchParams {
            vault,
            operations: preview_params.operations.clone(),
            plan_id: first.plan_id,
        };
        assert!(!manager.apply_task_batch(&apply).unwrap().completed);
        assert!(fs::read_to_string(&from).unwrap().contains("Move me"));
        apply.plan_id = manager.preview_task_batch(&preview_params).unwrap().plan_id;
        assert!(manager.apply_task_batch(&apply).unwrap().completed);
        assert_eq!(fs::read_to_string(&from).unwrap(), "# Inbox\n- [ ] Stay\n");
        assert!(
            fs::read_to_string(&to)
                .unwrap()
                .ends_with("- [ ] Move me\n  - keep child\n")
        );
    }
    #[test]
    fn task_scoped_dependencies_and_date_edits_keep_existing_metadata() {
        let (dir, manager, vault) = fixture();
        let from = dir.path().join("project.md");
        fs::write(
            &from,
            "- [ ] Old title #PROJECT 📅 2026-11-18 ⏳ 2026-11-20 ⏫ ⛔ dependency ^ref\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("separate.md"),
            "- [ ] Another task 🆔 dependency\n",
        )
        .unwrap();
        let list = manager.list_tasks(&parse(serde_json::json!({
            "vault":vault,"scope":{"kind":"note","path":"project.md"}, "tag":"#project", "blocked":true
        }))).unwrap();
        assert_eq!(list.total, 1);
        assert_eq!(list.tasks[0].blockers[0].path, "separate.md");
        let changes = TaskChanges {
            text: Some("New title".into()),
            due: Some(TaskDateEdit::Clear),
            scheduled: Some(TaskDateEdit::Set {
                date: "2026-11-22".into(),
            }),
            ..TaskChanges::default()
        };
        let result = manager
            .update_task(&UpdateTaskParams {
                vault,
                reference: list.tasks[0].reference.clone(),
                changes,
                reference_date: None,
            })
            .unwrap();
        assert!(result.task.dates.due.is_none());
        assert_eq!(result.task.dates.scheduled.as_deref(), Some("2026-11-22"));
        assert_eq!(result.task.priority, TaskPriority::High);
        assert_eq!(
            fs::read_to_string(from).unwrap(),
            "- [ ] New title #PROJECT ⏳ 2026-11-22 ⏫ ⛔ dependency ^ref\n"
        );
    }
}
