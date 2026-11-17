use std::{
    collections::{BTreeMap, HashSet},
    fs,
    ops::Range,
    path::Path,
    sync::LazyLock,
};

use chrono::{Days, Months, NaiveDate};
use regex::Regex;
use serde::Deserialize;

use crate::error::VaultError;

use super::{
    SetTaskStatus, TaskChanges, TaskDateEdit, TaskDates, TaskItem, TaskPriority, TaskRef,
    TaskStatus, invalid, links, patch, periodic, safe_join, validate_text,
};

static CHECKBOX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<indent>[ \t]*)(?:[-+*]|[0-9]{1,9}[.)])[ \t]+\[(?P<symbol>[^\]\r\n])\](?:[ \t]+|$)",
    )
    .unwrap()
});
static META: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("[🔺⏫🔼🔽⏬🛫➕⏳⌛📅📆🗓✅❌🔁🏁⛔🆔]\u{fe0f}?").unwrap());
static BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[ \t]+)\^[\p{L}\p{N}_-]+[ \t]*$").unwrap());
static RULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^every (?:(?<count>[1-9][0-9]*) )?(?<unit>days?|weeks?|months?)(?<after> when done)?$",
    )
    .unwrap()
});

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusConfig {
    symbol: String,
    name: String,
    #[serde(rename = "type", deserialize_with = "status_type")]
    status: TaskStatus,
    next_status_symbol: Option<String>,
}

fn status_type<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<TaskStatus, D::Error> {
    let name = String::deserialize(deserializer)?;
    Ok(match name.as_str() {
        "TODO" => TaskStatus::Todo,
        "DONE" => TaskStatus::Done,
        "IN_PROGRESS" => TaskStatus::InProgress,
        "ON_HOLD" => TaskStatus::OnHold,
        "CANCELLED" => TaskStatus::Cancelled,
        "NON_TASK" => TaskStatus::NonTask,
        _ => TaskStatus::Unknown,
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct StatusSettings {
    core_statuses: Vec<StatusConfig>,
    custom_statuses: Vec<StatusConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Settings {
    global_filter: String,
    status_settings: Option<StatusSettings>,
    task_format: Option<String>,
    set_done_date: Option<bool>,
    set_created_date: bool,
    recurrence_on_next_line: bool,
    remove_scheduled_date_on_recurrence: bool,
    #[serde(skip)]
    present: bool,
}

fn single_symbol(value: &str) -> Option<char> {
    let mut chars = value.chars();
    let first = chars.next()?;
    (chars.next().is_none()).then_some(first)
}

impl Settings {
    pub(super) fn read(root: &Path) -> Result<Self, VaultError> {
        let path = safe_join(
            root,
            None,
            ".obsidian/plugins/obsidian-tasks-plugin/data.json",
        )?;
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(VaultError::io(path.display().to_string(), error)),
        };
        let mut settings: Self = serde_json::from_slice(&bytes)
            .map_err(|error| invalid(format!("Cannot read Obsidian Tasks settings: {error}")))?;
        settings.present = true;
        Ok(settings)
    }

    pub(super) fn includes(&self, body: &str) -> bool {
        let filter = self.global_filter.trim();
        if filter.is_empty() {
            true
        } else if let Some(tag) = filter.strip_prefix('#') {
            super::super::tags::contains_inline_tag(body, tag, false)
        } else {
            body.contains(filter)
        }
    }

    pub(super) fn create_text(&self, text: &str) -> String {
        if self.includes(text) {
            return text.to_owned();
        }
        let (text, suffix) = BLOCK
            .find(text)
            .map_or((text, ""), |found| text.split_at(found.start()));
        format!("{text} {}{suffix}", self.global_filter.trim())
    }

    pub(super) fn open_symbol(&self) -> Result<char, VaultError> {
        let symbol = self.symbol_for(TaskStatus::Todo, ' ');
        if self.status(symbol) != TaskStatus::Todo {
            return Err(invalid(
                "Obsidian Tasks settings have no open checkbox status.",
            ));
        }
        Ok(symbol)
    }

    fn config(&self, symbol: char) -> Option<&StatusConfig> {
        let wanted = if symbol == 'X' { 'x' } else { symbol };
        self.status_settings
            .as_ref()?
            .core_statuses
            .iter()
            .chain(&self.status_settings.as_ref()?.custom_statuses)
            .find(|config| single_symbol(&config.symbol) == Some(wanted))
    }

    fn status(&self, symbol: char) -> TaskStatus {
        self.config(symbol).map_or_else(
            || match symbol {
                ' ' => TaskStatus::Todo,
                'x' | 'X' => TaskStatus::Done,
                '/' if self.present && self.status_settings.is_none() => TaskStatus::InProgress,
                '-' if self.present && self.status_settings.is_none() => TaskStatus::Cancelled,
                _ => TaskStatus::Unknown,
            },
            |c| c.status,
        )
    }

    fn symbol_for(&self, status: TaskStatus, fallback: char) -> char {
        if self.status(fallback) == status {
            return fallback;
        }
        self.status_settings
            .as_ref()
            .and_then(|settings| {
                settings
                    .core_statuses
                    .iter()
                    .chain(&settings.custom_statuses)
                    .find(|config| config.status == status)
            })
            .and_then(|config| single_symbol(&config.symbol))
            .unwrap_or(fallback)
    }

    fn next_recurrence_symbol(&self, completed: char) -> Result<char, VaultError> {
        let mut cursor = self
            .config(completed)
            .and_then(|config| config.next_status_symbol.as_deref())
            .and_then(single_symbol)
            .unwrap_or(' ');
        let mut seen = HashSet::new();
        let mut progress = None;
        while seen.insert(cursor) {
            match self.status(cursor) {
                TaskStatus::Todo => return Ok(cursor),
                TaskStatus::InProgress => {
                    progress.get_or_insert(cursor);
                }
                _ => {}
            }
            cursor = self
                .config(cursor)
                .and_then(|config| config.next_status_symbol.as_deref())
                .and_then(single_symbol)
                .unwrap_or(' ');
        }
        if let Some(progress) = progress {
            return Ok(progress);
        }
        self.open_symbol()
    }
}

pub(super) struct ParsedTask<'a> {
    pub line: usize,
    pub span: Range<usize>,
    pub child_end: usize,
    pub indentation: &'a str,
    pub body: &'a str,
    symbol: char,
    symbol_at: Range<usize>,
    body_at: Range<usize>,
    heading: Option<String>,
}

pub(super) fn parse_tasks(content: &str) -> Vec<ParsedTask<'_>> {
    let frontmatter_end = patch::body_start(content);
    let code = links::code_spans(content);
    let headings = patch::headings(content);
    let mut tasks: Vec<ParsedTask<'_>> = Vec::new();
    let mut parents: Vec<usize> = Vec::new();
    let mut byte = 0;
    let mut heading = None;
    let mut next_heading = 0;
    let mut code_index = 0;
    for (i, raw) in content.split_inclusive('\n').enumerate() {
        let start = byte;
        byte += raw.len();
        if start < frontmatter_end {
            continue;
        }
        while next_heading < headings.len() && headings[next_heading].line <= i + 1 {
            heading = Some(headings[next_heading].text.clone());
            next_heading += 1;
        }
        let line = raw.trim_end_matches(['\r', '\n']);
        if !line.trim().is_empty() {
            let width = indentation_width(line);
            while let Some(&index) = parents.last() {
                if indentation_width(tasks[index].indentation) < width {
                    break;
                }
                tasks[index].child_end = start;
                parents.pop();
            }
        }
        while code.get(code_index).is_some_and(|s| s.end <= start) {
            code_index += 1;
        }
        if code.get(code_index).is_some_and(|s| s.contains(&start)) {
            continue;
        }
        let Some(parts) = CHECKBOX.captures(line) else {
            continue;
        };
        let found = parts.get(0).unwrap();
        let symbol = parts.name("symbol").unwrap();
        tasks.push(ParsedTask {
            line: i + 1,
            span: start..byte,
            child_end: byte,
            indentation: parts.name("indent").unwrap().as_str(),
            body: &line[found.end()..],
            symbol: symbol.as_str().chars().next().unwrap(),
            symbol_at: start + symbol.start()..start + symbol.end(),
            body_at: start + found.end()..start + line.len(),
            heading: heading.clone(),
        });
        parents.push(tasks.len() - 1);
    }
    for index in parents {
        tasks[index].child_end = content.len();
    }
    tasks
}

fn indentation_width(line: &str) -> usize {
    line.bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .fold(0, |width, b| {
            if b == b'\t' {
                (width / 4 + 1) * 4
            } else {
                width + 1
            }
        })
}

impl ParsedTask<'_> {
    pub(super) fn item(&self, path: &str, revision: &str, settings: &Settings) -> TaskItem {
        let meta = Metadata::parse(self.body);
        let date = |field| {
            meta.value(field)
                .filter(|val| periodic::parse_date(val).is_ok())
                .map(str::to_owned)
        };
        let priority = meta
            .fields
            .get(&Field::Priority)
            .map_or(TaskPriority::Normal, |field| field.priority);
        let tags: Vec<_> = super::super::tags::inline_tags(self.body)
            .into_iter()
            .take(50)
            .map(|tag| clip(&tag, 100).0)
            .collect();
        let (text, text_truncated) = clip(meta.description, 2400);
        TaskItem {
            reference: TaskRef {
                path: path.to_owned(),
                line: self.line,
                revision: revision.to_owned(),
            },
            heading: self.heading.as_ref().map(|heading| clip(heading, 500).0),
            text,
            text_truncated,
            details_truncated: false,
            symbol: self.symbol,
            status: settings.status(self.symbol),
            status_name: settings
                .config(self.symbol)
                .map(|config| clip(&config.name, 100).0),
            dates: TaskDates {
                due: date(Field::Due),
                start: date(Field::Start),
                scheduled: date(Field::Scheduled),
                done: date(Field::Done),
            },
            priority,
            tags,
            recurrence: meta.value(Field::Recurrence).map(|rule| clip(rule, 300).0),
            id: meta
                .value(Field::Id)
                .filter(|value| valid_id(value))
                .map(str::to_owned),
            dependencies: meta
                .value(Field::Dependencies)
                .into_iter()
                .flat_map(|value| value.split(','))
                .map(str::trim)
                .filter(|id| valid_id(id))
                .map(str::to_owned)
                .collect(),
            blockers: Vec::new(),
            dependency_issues: Vec::new(),
        }
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn clip(text: &str, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text.to_owned(), false);
    }
    let end = text
        .char_indices()
        .rev()
        .find(|(i, _)| *i <= max)
        .map_or(0, |(i, _)| i);
    (text[..end].to_owned(), true)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Field {
    Start,
    Created,
    Scheduled,
    Due,
    Done,
    Cancelled,
    Recurrence,
    OnCompletion,
    Dependencies,
    Id,
    Priority,
}

impl Field {
    fn from_marker(symbol: char) -> Self {
        match symbol {
            '🛫' => Self::Start,
            '➕' => Self::Created,
            '⏳' | '⌛' => Self::Scheduled,
            '📅' | '📆' | '🗓' => Self::Due,
            '✅' => Self::Done,
            '❌' => Self::Cancelled,
            '🔁' => Self::Recurrence,
            '🏁' => Self::OnCompletion,
            '⛔' => Self::Dependencies,
            '🆔' => Self::Id,
            _ => Self::Priority,
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Self::Start => "🛫",
            Self::Created => "➕",
            Self::Scheduled => "⏳",
            Self::Due => "📅",
            Self::Done => "✅",
            Self::Cancelled => "❌",
            Self::Recurrence => "🔁",
            Self::OnCompletion => "🏁",
            Self::Dependencies => "⛔",
            Self::Id => "🆔",
            Self::Priority => "",
        }
    }

    fn takes_date(self) -> bool {
        matches!(
            self,
            Self::Start
                | Self::Created
                | Self::Scheduled
                | Self::Due
                | Self::Done
                | Self::Cancelled
        )
    }
}

fn priority(marker: char) -> TaskPriority {
    match marker {
        '🔺' => TaskPriority::Highest,
        '⏫' => TaskPriority::High,
        '🔼' => TaskPriority::Medium,
        '🔽' => TaskPriority::Low,
        '⏬' => TaskPriority::Lowest,
        _ => TaskPriority::Normal,
    }
}

fn priority_marker(priority: TaskPriority) -> &'static str {
    match priority {
        TaskPriority::Highest => "🔺",
        TaskPriority::High => "⏫",
        TaskPriority::Medium => "🔼",
        TaskPriority::Normal => "",
        TaskPriority::Low => "🔽",
        TaskPriority::Lowest => "⏬",
    }
}

struct FieldSpan<'a> {
    span: Range<usize>,
    value: &'a str,
    priority: TaskPriority,
}

struct Metadata<'a> {
    body: &'a str,
    description: &'a str,
    block: Option<Range<usize>>,
    fields: BTreeMap<Field, FieldSpan<'a>>,
}

impl<'a> Metadata<'a> {
    fn parse(body: &'a str) -> Self {
        let block = BLOCK.find(body).map(|m| m.range());
        let end = block.as_ref().map_or(body.len(), |m| m.start);
        let body_without_block = &body[..end];
        let code = links::code_spans(body_without_block);
        let markers: Vec<_> = META
            .find_iter(body_without_block)
            .filter(|m| {
                (m.start() == 0 || body.as_bytes()[m.start() - 1].is_ascii_whitespace())
                    && !links::in_code(&code, m.start())
            })
            .collect();
        let mut fields = BTreeMap::new();
        let mut desc_end = end;
        for (index, found) in markers.iter().enumerate() {
            let marker = found.as_str().chars().next().unwrap();
            let field = Field::from_marker(marker);
            let next = markers.get(index + 1).map_or(end, |m| m.start());
            let rest = &body[found.end()..next];
            let value = if field == Field::Priority {
                ""
            } else if field.takes_date() || matches!(field, Field::Id | Field::OnCompletion) {
                rest.split_whitespace().next().unwrap_or_default()
            } else {
                rest.split(" #").next().unwrap_or(rest).trim()
            };
            desc_end = desc_end.min(found.start());
            if field.takes_date() && periodic::parse_date(value).is_err() {
                continue;
            }
            let field_end = if value.is_empty() {
                found.end()
            } else {
                found.end() + rest.len() - rest.trim_start().len() + value.len()
            };
            fields.insert(
                field,
                FieldSpan {
                    span: found.start()..field_end,
                    value,
                    priority: priority(marker),
                },
            );
        }
        Self {
            body,
            description: body[..desc_end].trim_end(),
            block,
            fields,
        }
    }

    fn value(&self, field: Field) -> Option<&str> {
        self.fields.get(&field).map(|m| m.value)
    }

    fn edit(
        &self,
        text: Option<&str>,
        changes: &[(Field, Option<String>)],
        remove_block: bool,
    ) -> String {
        let mut output = self.body.to_string();
        let mut edits: Vec<(Range<usize>, String)> = Vec::new();
        let mut added = Vec::new();
        if let Some(text) = text {
            let original_tags = super::super::tags::inline_tags(self.description);
            let wanted_tags = super::super::tags::inline_tags(text);
            let extras = original_tags
                .into_iter()
                .filter(|tag| !wanted_tags.contains(tag))
                .map(|tag| format!(" #{tag}"))
                .collect::<String>();
            let separator = if self.description.is_empty()
                && !self.body.is_empty()
                && !self.body.starts_with([' ', '\t'])
            {
                " "
            } else {
                ""
            };
            edits.push((
                0..self.description.len(),
                format!("{text}{extras}{separator}"),
            ));
        }
        for (field, value) in changes {
            let rendered = value.as_ref().map(|v| {
                if *field == Field::Priority {
                    v.clone()
                } else {
                    format!("{} {v}", field.symbol())
                }
            });
            if let Some(existing) = self.fields.get(field) {
                let start = if rendered.is_none()
                    && self
                        .body
                        .as_bytes()
                        .get(existing.span.start.wrapping_sub(1))
                        == Some(&b' ')
                {
                    existing.span.start - 1
                } else {
                    existing.span.start
                };
                edits.push((start..existing.span.end, rendered.unwrap_or_default()));
            } else if let Some(rendered) = rendered {
                added.push(rendered);
            }
        }
        let suffix = self
            .block
            .as_ref()
            .map_or(self.body.len(), |span| span.start);
        if remove_block && let Some(block) = &self.block {
            edits.push((block.clone(), String::new()));
        }
        if !added.is_empty() {
            let pos = self.body[..suffix].trim_end().len();
            edits.push((pos..pos, format!(" {}", added.join(" "))));
        }
        edits.sort_by(|(a, _), (b, _)| b.start.cmp(&a.start).then(b.end.cmp(&a.end)));
        for (range, replacement) in edits {
            output.replace_range(range, &replacement);
        }
        output
    }
}

pub(super) fn eol(content: &str) -> &str {
    content
        .find('\n')
        .filter(|i| *i > 0 && content.as_bytes()[i - 1] == b'\r')
        .map_or("\n", |_| "\r\n")
}

fn date_change(change: &TaskDateEdit) -> Result<Option<String>, VaultError> {
    match change {
        TaskDateEdit::Set { date } => Ok(Some(periodic::parse_date(date)?.to_string())),
        TaskDateEdit::Clear => Ok(None),
    }
}

fn line_text(content: &str, task: &ParsedTask<'_>, symbol: char, body: &str) -> String {
    let between = &content[task.symbol_at.end..task.body_at.start];
    let space = if between == "]" && !body.is_empty() {
        " "
    } else {
        ""
    };
    format!(
        "{}{symbol}{between}{space}{body}",
        &content[task.span.start..task.symbol_at.start]
    )
}

pub(super) fn rewrite_task(
    content: &str,
    task: &ParsedTask<'_>,
    changes: &TaskChanges,
    settings: &Settings,
    today: NaiveDate,
) -> Result<(String, usize, Option<usize>), VaultError> {
    if let Some(text) = &changes.text {
        validate_text(text)?;
    }
    let mut edits = Vec::new();
    if let Some(change) = &changes.due {
        edits.push((Field::Due, date_change(change)?));
    }
    if let Some(change) = &changes.scheduled {
        edits.push((Field::Scheduled, date_change(change)?));
    }
    if let Some(priority) = changes.priority {
        let marker = priority_marker(priority);
        edits.push((
            Field::Priority,
            (!marker.is_empty()).then(|| marker.to_owned()),
        ));
    }
    let status = settings.status(task.symbol);
    let target = changes.status.map_or(status, |requested| match requested {
        SetTaskStatus::Open => TaskStatus::Todo,
        SetTaskStatus::Completed => TaskStatus::Done,
    });
    let symbol = if status == target {
        task.symbol
    } else {
        let fallback = if target == TaskStatus::Done { 'x' } else { ' ' };
        let symbol = settings.symbol_for(target, fallback);
        if settings.status(symbol) != target {
            return Err(invalid(
                "Obsidian Tasks settings have no checkbox for the requested status.",
            ));
        }
        symbol
    };
    if settings
        .task_format
        .as_deref()
        .is_some_and(|format| format != "tasksPluginEmoji")
        && (!edits.is_empty() || (status != target && task.body.contains("repeat::")))
    {
        return Err(invalid(
            "This task uses a non-emoji Tasks format. Its metadata cannot be edited here.",
        ));
    }
    if changes.text.is_some()
        && settings
            .task_format
            .as_deref()
            .is_some_and(|format| format != "tasksPluginEmoji")
        && task.body.contains("::")
    {
        return Err(invalid(
            "This task has non-emoji metadata. Editing its description could remove that metadata.",
        ));
    }
    let body =
        Metadata::parse(task.body).edit(changes.text.as_deref().map(str::trim), &edits, false);
    let effective = Metadata::parse(&body);
    if status != target && effective.value(Field::OnCompletion).is_some() {
        return Err(invalid(
            "This task has an on-completion action. Use Obsidian Tasks to change its status.",
        ));
    }
    let mut next = None;
    let body = if status != target && target == TaskStatus::Done {
        if let Some(rule) = effective.value(Field::Recurrence) {
            next = Some(next_occurrence(
                content, task, &effective, settings, today, symbol, rule,
            )?);
        }
        if settings
            .set_done_date
            .unwrap_or(settings.present || next.is_some())
        {
            effective.edit(
                None,
                &[
                    (Field::Done, Some(today.to_string())),
                    (Field::Cancelled, None),
                ],
                false,
            )
        } else {
            body
        }
    } else if status == TaskStatus::Done && target == TaskStatus::Todo {
        effective.edit(None, &[(Field::Done, None)], false)
    } else {
        body
    };
    let newline = &content[task.body_at.end..task.span.end];
    let text = line_text(content, task, symbol, &body);
    let head = &content[..task.span.start];
    let tail = &content[task.span.end..];
    let Some(next) = next else {
        return Ok((format!("{head}{text}{newline}{tail}"), task.line, None));
    };
    let separator = eol(content);
    if settings.recurrence_on_next_line {
        let children = &content[task.span.end..task.child_end];
        let rest = &content[task.child_end..];
        let ended = if children.is_empty() {
            !newline.is_empty()
        } else {
            children.ends_with('\n')
        };
        let jump = usize::from(!newline.is_empty())
            + children.bytes().filter(|b| *b == b'\n').count()
            + usize::from(!ended);
        let between = if ended { "" } else { separator };
        Ok((
            format!("{head}{text}{newline}{children}{between}{next}{separator}{rest}"),
            task.line,
            Some(task.line + jump),
        ))
    } else {
        Ok((
            format!("{head}{next}{separator}{text}{newline}{tail}"),
            task.line + 1,
            Some(task.line),
        ))
    }
}

fn next_occurrence(
    content: &str,
    task: &ParsedTask<'_>,
    current: &Metadata<'_>,
    settings: &Settings,
    today: NaiveDate,
    completed: char,
    rule: &str,
) -> Result<String, VaultError> {
    let parsed = RULE.captures(rule).ok_or_else(|| invalid(format!("Unsupported task recurrence '{rule}'. Use 'every day', 'every week', 'every month', or an interval.")))?;
    let interval = parsed
        .name("count")
        .map_or(Ok(1), |count| count.as_str().parse::<u32>())
        .map_err(|_| invalid("The task recurrence interval is too large."))?;
    let with_completion = parsed.name("after").is_some();
    let order = if settings.remove_scheduled_date_on_recurrence {
        [Field::Due, Field::Start, Field::Scheduled]
    } else {
        [Field::Due, Field::Scheduled, Field::Start]
    };
    let fields: Vec<_> = order
        .into_iter()
        .filter_map(|field| {
            current
                .value(field)
                .and_then(|str| periodic::parse_date(str).ok())
                .map(|date| (field, date))
        })
        .collect();
    let base = fields.first().map_or(today, |(_, date)| *date);
    let anchor = if with_completion { today } else { base };
    let unit = parsed.name("unit").unwrap().as_str().to_ascii_lowercase();
    let next = if unit.starts_with("day") {
        anchor.checked_add_days(Days::new(u64::from(interval)))
    } else if unit.starts_with("week") {
        anchor.checked_add_days(Days::new(u64::from(interval) * 7))
    } else {
        anchor.checked_add_months(Months::new(interval))
    }
    .ok_or_else(|| invalid("The next task occurrence is outside the supported date range."))?;
    let mut edits = Vec::new();
    let drop_scheduled = settings.remove_scheduled_date_on_recurrence
        && fields
            .iter()
            .any(|(field, _)| matches!(field, Field::Start | Field::Due));
    for (field, date) in fields {
        if field == Field::Scheduled && drop_scheduled {
            edits.push((field, None));
        } else {
            let shifted = next.checked_add_signed(date - base).ok_or_else(|| {
                invalid("A date in the next task occurrence is outside the supported range.")
            })?;
            edits.push((field, Some(shifted.to_string())));
        }
    }
    edits.extend([
        (Field::Done, None),
        (Field::Cancelled, None),
        (Field::Id, None),
        (Field::Dependencies, None),
        (
            Field::Created,
            settings.set_created_date.then(|| today.to_string()),
        ),
    ]);
    let body = current.edit(None, &edits, true);
    Ok(line_text(
        content,
        task,
        settings.next_recurrence_symbol(completed)?,
        &body,
    ))
}
