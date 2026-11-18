use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    io::ErrorKind,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    error::VaultError,
    tools::batch_move::{BatchMoveParams, BatchNoteMove},
};

use super::{
    VaultManager,
    canvas::{
        CanvasLinkUpdate, CanvasMove, encode_uri, ensure_parent, markdown_label, validate_relative,
    },
    check_note_revision,
    links::{self, Link, LinkKind},
    note_revision,
    path::{projected_path, safe_join},
    rel_path,
    walk::md_files,
    write::{
        atomic_write, atomic_write_durable, atomic_write_new, copy_verified, rename_no_replace,
        stream_revision,
    },
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Location {
    vault: String,
    path: String,
}

impl Location {
    fn new(vault: &str, path: String) -> Self {
        Self {
            vault: vault.into(),
            path,
        }
    }

    fn display(&self) -> String {
        format!("{}/{}", self.vault, self.path)
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BatchMoveResult {
    pub preview_id: String,
    pub applied: bool,
    pub blockers: Vec<String>,
    pub total: usize,
    pub next_offset: Option<usize>,
    pub changes: Vec<BatchMoveDetail>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct BatchMoveDetail {
    kind: &'static str,
    source: String,
    destination: Option<String>,
    description: Option<String>,
    status: &'static str,
}

struct MoveEntry {
    request: BatchNoteMove,
    from: Location,
    to: Location,
    source: PathBuf,
    destination: PathBuf,
    revision: String,
}

struct CopyEntry {
    source: Location,
    destination: Location,
    revision: String,
}

struct NoteUpdate {
    read: Location,
    write: Location,
    revision: String,
    content: String,
}

#[derive(Default)]
struct Plan {
    moves: Vec<MoveEntry>,
    copies: Vec<CopyEntry>,
    notes: Vec<NoteUpdate>,
    boards: Vec<CanvasLinkUpdate>,
    blockers: BTreeSet<String>,
}

impl Plan {
    fn details(&self) -> Vec<BatchMoveDetail> {
        let mut result = Vec::new();
        for entry in &self.moves {
            result.push(detail("move", &entry.from, Some(&entry.to), None));
        }
        for entry in &self.copies {
            result.push(detail(
                "copy_attachment",
                &entry.source,
                Some(&entry.destination),
                None,
            ));
        }
        for entry in &self.notes {
            result.push(detail(
                "update_note_links",
                &entry.read,
                (entry.read != entry.write).then_some(&entry.write),
                None,
            ));
        }
        for entry in &self.boards {
            result.push(detail(
                "update_canvas",
                &Location::new(&entry.vault, entry.path.clone()),
                None,
                (entry.converted > 0)
                    .then(|| format!("{} file cards become text links.", entry.converted)),
            ));
        }
        result
    }

    fn id(&self, params: &BatchMoveParams) -> Result<String, VaultError> {
        let moves: Vec<_> = self
            .moves
            .iter()
            .map(|e| (&e.request, &e.source, &e.destination, &e.revision))
            .collect();
        let copies: Vec<_> = self
            .copies
            .iter()
            .map(|e| {
                (
                    &e.source.vault,
                    &e.source.path,
                    &e.destination.vault,
                    &e.destination.path,
                    &e.revision,
                )
            })
            .collect();
        let notes: Vec<_> = self
            .notes
            .iter()
            .map(|e| {
                (
                    e.read.display(),
                    e.write.display(),
                    &e.revision,
                    note_revision(e.content.as_bytes()),
                )
            })
            .collect();
        let boards: Vec<_> = self
            .boards
            .iter()
            .map(|e| {
                (
                    &e.vault,
                    &e.path,
                    &e.revision,
                    note_revision(e.content.as_bytes()),
                )
            })
            .collect();
        let payload = serde_json::to_vec(&(
            moves,
            copies,
            notes,
            boards,
            &self.blockers,
            params.allow_attachment_copy,
            params.allow_embed_conversion,
            params.allow_canvas_conversion,
        ))
        .map_err(|e| invalid(e.to_string()))?;
        Ok(plan_key(&payload))
    }
}

fn detail(
    kind: &'static str,
    source: &Location,
    destination: Option<&Location>,
    description: Option<String>,
) -> BatchMoveDetail {
    BatchMoveDetail {
        kind,
        source: source.display(),
        destination: destination.map(Location::display),
        description,
        status: "planned",
    }
}

impl VaultManager {
    pub fn batch_move_notes(
        &self,
        params: &BatchMoveParams,
    ) -> Result<BatchMoveResult, VaultError> {
        if params.moves.is_empty() || params.moves.len() > 100 {
            return Err(invalid("Provide between 1 and 100 note moves."));
        }
        let _guard = params.preview_id.as_ref().map(|_| self.write_guard());
        let plan = self.build_batch_plan(params);
        let digest = plan.id(params)?;
        let mut preview_id = new_preview_id(&digest);
        let mut details = plan.details();
        let mut blockers: Vec<_> = plan.blockers.iter().cloned().collect();
        let mut attempted = false;
        if let Some(expected) = &params.preview_id {
            if !matches_preview(expected, &digest) {
                blockers.push(
                    "The plan changed or expired. Preview the moves again before applying.".into(),
                );
            }
            if blockers.is_empty() {
                let receipt = plan_key(expected.as_bytes());
                match self.reserve_preview(&receipt) {
                    Ok(()) => {
                        preview_id.clone_from(expected);
                        self.apply_batch_plan(&plan, &receipt, &mut details);
                        attempted = true;
                    }
                    Err(error) => blockers.push(error.to_string()),
                }
            }
        }

        let total = details.len();
        let end = params
            .offset
            .saturating_add(params.limit.unwrap_or(50).clamp(1, 200));
        let next_offset = (end < total).then_some(end);
        let applied = attempted && details.iter().all(|detail| detail.status == "completed");
        if !attempted {
            details = details
                .into_iter()
                .skip(params.offset)
                .take(end - params.offset)
                .collect();
        }

        Ok(BatchMoveResult {
            preview_id,
            applied,
            blockers,
            total,
            next_offset: if attempted { None } else { next_offset },
            changes: details,
        })
    }

    fn build_batch_plan(&self, params: &BatchMoveParams) -> Plan {
        let mut plan = Plan::default();
        let mut sources = HashSet::new();
        let mut destinations = HashSet::new();
        for request in &params.moves {
            match self.batch_entry(request) {
                Ok(entry) => {
                    if !sources.insert(path_key(&entry.source)) {
                        plan.blockers
                            .insert(format!("Source repeats: {}.", entry.from.display()));
                    }
                    if !destinations.insert(path_key(&entry.destination)) {
                        plan.blockers
                            .insert(format!("Destination repeats: {}.", entry.to.display()));
                    }
                    plan.moves.push(entry);
                }
                Err(error) => {
                    plan.blockers.insert(error.to_string());
                }
            }
        }
        for entry in &plan.moves {
            if entry.source == entry.destination {
                plan.blockers.insert(format!(
                    "Source and destination are the same: {}.",
                    entry.from.display()
                ));
            } else if entry.destination.exists() && !sources.contains(&path_key(&entry.destination))
            {
                plan.blockers.insert(format!(
                    "Destination already exists: {}.",
                    entry.to.display()
                ));
            }
        }
        if !plan.blockers.is_empty() {
            return plan;
        }
        let canvas_moves: Vec<_> = plan
            .moves
            .iter()
            .map(|e| CanvasMove {
                source_vault: e.from.vault.clone(),
                source_path: e.from.path.clone(),
                destination_vault: e.to.vault.clone(),
                destination_path: e.to.path.clone(),
            })
            .collect();
        match self.plan_canvas_note_moves(&canvas_moves, params.allow_canvas_conversion) {
            Ok(updates) => plan.boards = updates,
            Err(error) => {
                plan.blockers.insert(error.to_string());
            }
        }
        self.plan_note_updates(&mut plan, params);
        plan
    }

    fn batch_entry(&self, request: &BatchNoteMove) -> Result<MoveEntry, VaultError> {
        let from = Location::new(
            &request.source_vault,
            super::ensure_md_extension(&request.source_path),
        );
        let to = Location::new(
            &request.destination_vault,
            super::ensure_md_extension(&request.destination_path),
        );
        let source = self.batch_path(&from)?;
        let destination = self.batch_path(&to)?;
        self.ensure_write_allowed(&from.vault, &source)?;
        self.ensure_write_allowed(&to.vault, &destination)?;
        let revision = stream_revision(&source)?;
        Ok(MoveEntry {
            request: request.clone(),
            from,
            to,
            source,
            destination,
            revision,
        })
    }

    fn batch_path(&self, location: &Location) -> Result<PathBuf, VaultError> {
        validate_relative(&location.path)?;
        let root = self.resolve_vault(&location.vault)?;
        let path = safe_join(root, None, &location.path)?;
        let mut cursor = root.to_path_buf();
        for segment in location.path.split('/') {
            cursor.push(segment);
            match fs::symlink_metadata(&cursor) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(invalid(format!(
                        "Symbolic links are not supported in a batch move path: {}.",
                        location.display()
                    )));
                }
                Ok(metadata) if cursor != path && !metadata.is_dir() => {
                    return Err(invalid(format!(
                        "A parent of {} is not a folder.",
                        location.display()
                    )));
                }
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => break,
                Err(error) => return Err(VaultError::io(cursor.display().to_string(), error)),
            }
        }
        Ok(path)
    }

    fn plan_note_updates(&self, plan: &mut Plan, params: &BatchMoveParams) {
        let mapping: BTreeMap<_, _> = plan
            .moves
            .iter()
            .map(|e| (e.from.clone(), e.to.clone()))
            .collect();
        let source_vaults: BTreeSet<&str> =
            mapping.keys().map(|from| from.vault.as_str()).collect();
        let mut copies = BTreeMap::<Location, CopyEntry>::new();
        for vault in source_vaults {
            let root = match self.resolve_vault(vault) {
                Ok(root) => root,
                Err(error) => {
                    plan.blockers.insert(error.to_string());
                    continue;
                }
            };
            let mut files = md_files(root);
            files.extend(
                plan.moves
                    .iter()
                    .filter(|e| e.from.vault == vault)
                    .map(|e| e.source.clone()),
            );
            files.sort();
            files.dedup();
            let known: BTreeSet<_> = files.iter().map(|p| rel_path(root, p)).collect();
            for path in files {
                let source = Location::new(vault, rel_path(root, &path));
                let destination = mapping.get(&source).unwrap_or(&source);
                let content = match read_utf8(&path) {
                    Ok(content) => content,
                    Err(error) => {
                        plan.blockers.insert(error.to_string());
                        continue;
                    }
                };
                let mut edits = Vec::new();
                let mut refs = links::parse_links(&content);
                refs.extend(attachment_links(&content));
                for reference in refs {
                    match self.plan_link(
                        &reference,
                        &source,
                        destination,
                        &known,
                        &mapping,
                        params,
                        &mut copies,
                    ) {
                        Ok(Some(replacement)) if content[reference.span.clone()] != replacement => {
                            edits.push((reference.span, replacement))
                        }
                        Ok(_) => {}
                        Err(error) => {
                            plan.blockers.insert(format!(
                                "{} line {}: {error}",
                                source.display(),
                                reference.line
                            ));
                        }
                    }
                }
                if edits.is_empty() {
                    continue;
                }
                if let Err(error) = self.ensure_write_allowed(vault, &path) {
                    plan.blockers.insert(error.to_string());
                    continue;
                }
                edits.sort_by_key(|(span, _)| span.start);
                edits.dedup_by(|a, b| a.0 == b.0);
                let revision = note_revision(content.as_bytes());
                let mut updated = content;
                let mut end = updated.len();
                for (span, replacement) in edits.into_iter().rev() {
                    if span.end <= end {
                        end = span.start;
                        updated.replace_range(span, &replacement);
                    }
                }
                plan.notes.push(NoteUpdate {
                    read: source.clone(),
                    write: destination.clone(),
                    revision,
                    content: updated,
                });
            }
        }
        for entry in copies.into_values() {
            if plan.moves.iter().any(|e| e.to == entry.destination) {
                plan.blockers.insert(format!(
                    "Attachment conflicts with a destination note: {}.",
                    entry.destination.display()
                ));
            }
            plan.copies.push(entry);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_link(
        &self,
        link: &Link,
        from: &Location,
        future_from: &Location,
        known: &BTreeSet<String>,
        mapping: &BTreeMap<Location, Location>,
        params: &BatchMoveParams,
        copies: &mut BTreeMap<Location, CopyEntry>,
    ) -> Result<Option<String>, VaultError> {
        let parent = Path::new(&from.path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let target = match resolve_note_link(link, parent, known) {
            Ok(target) => target,
            Err(ambiguous) => {
                if from.vault != future_from.vault
                    || ambiguous
                        .iter()
                        .any(|path| mapping.contains_key(&Location::new(&from.vault, path.clone())))
                {
                    return Err(invalid(format!(
                        "Link to '{}' is ambiguous: {}.",
                        link.target,
                        ambiguous.join(", ")
                    )));
                }
                return Ok(None);
            }
        };
        if let Some(note) = target {
            let old_target = Location::new(&from.vault, note);
            let target = mapping.get(&old_target).unwrap_or(&old_target);
            if target.vault != future_from.vault {
                if link.embed && !params.allow_embed_conversion {
                    return Err(invalid(format!(
                        "The embedded note '{}' needs consent to become a link.",
                        old_target.display()
                    )));
                }
                self.batch_path(target)?;
                let anchor = link
                    .heading
                    .as_ref()
                    .map_or_else(String::new, |value| format!("#{value}"));
                return Ok(Some(format!(
                    "[{}]({})",
                    link_label(link),
                    self.obsidian_note_uri(&target.vault, &target.path, &anchor)?
                )));
            }
            let origin_folder_changed = parent
                != Path::new(&future_from.path)
                    .parent()
                    .unwrap_or_else(|| Path::new(""));
            let wiki_may_resolve_differently = link.kind == LinkKind::Wiki
                && !link.target.contains('/')
                && mapping.values().any(|destination| {
                    destination.vault == target.vault
                        && Path::new(&destination.path).file_name()
                            == Path::new(&target.path).file_name()
                });
            if &old_target != target || origin_folder_changed || wiki_may_resolve_differently {
                return Ok(Some(format_internal(link, future_from, &target.path)));
            }
            return Ok(None);
        }
        if from.vault == future_from.vault {
            if link.kind == LinkKind::Markdown && from.path != future_from.path {
                let resolved = resolve_asset(self.resolve_vault(&from.vault)?, parent, link)?;
                return Ok(resolved
                    .as_deref()
                    .map(|path| format_internal(link, future_from, path)));
            }
            return Ok(None);
        }

        let resolved = resolve_asset(self.resolve_vault(&from.vault)?, parent, link)?;
        let Some(path) = resolved else {
            if link.embed {
                return Err(invalid(format!("Missing embedded file '{}'.", link.target)));
            }
            return Ok(None);
        };
        if !params.allow_attachment_copy {
            return Err(invalid(format!(
                "Referenced file '{path}' needs consent to copy to '{}'.",
                future_from.vault
            )));
        }
        let source = Location::new(&from.vault, path.clone());
        let destination = Location::new(&future_from.vault, path);
        let orig = self.batch_path(&source)?;
        let dest = self.batch_path(&destination)?;
        self.ensure_write_allowed(&destination.vault, &dest)?;
        let revision = stream_revision(&orig)?;
        if dest.exists() {
            if revision != stream_revision(&dest)? {
                return Err(invalid(format!(
                    "Attachment destination already contains a different file: {}.",
                    destination.display()
                )));
            }
        } else {
            let entry = CopyEntry {
                source,
                destination: destination.clone(),
                revision,
            };
            if let Some(previous) = copies.get(&destination) {
                if previous.revision != entry.revision {
                    return Err(invalid(format!(
                        "Multiple attachments target {}.",
                        destination.display()
                    )));
                }
            } else {
                copies.insert(destination.clone(), entry);
            }
        }
        Ok(Some(format_internal(link, future_from, &destination.path)))
    }
}

impl VaultManager {
    fn apply_batch_plan(&self, plan: &Plan, id: &str, details: &mut [BatchMoveDetail]) {
        for change in &mut *details {
            change.status = "not_started";
        }
        let mut step = 0;
        let sources: HashSet<_> = plan
            .moves
            .iter()
            .map(|entry| path_key(&entry.source))
            .collect();
        if plan
            .moves
            .iter()
            .any(|entry| sources.contains(&path_key(&entry.destination)))
        {
            if let Err(error) = self.apply_exchange(plan, id, &mut details[..plan.moves.len()]) {
                if let Some(detail) = details[..plan.moves.len()]
                    .iter_mut()
                    .find(|detail| matches!(detail.status, "failed" | "not_started"))
                {
                    detail.status = "failed";
                    detail.description = Some(error.to_string());
                } else {
                    details[0].description = Some(error.to_string());
                    details[0].status = "check_paths";
                }
                return;
            }
            step = plan.moves.len();
        } else {
            for entry in &plan.moves {
                let result = self.batch_path(&entry.from).and_then(|source| {
                    let destination = self.batch_path(&entry.to)?;
                    self.ensure_write_allowed(&entry.from.vault, &source)?;
                    self.ensure_write_allowed(&entry.to.vault, &destination)?;
                    move_verified(&source, &destination, &entry.revision)
                });
                if !advance(&mut details[step], result) {
                    return;
                }
                step += 1;
            }
        }
        for entry in &plan.copies {
            let result = self.batch_path(&entry.source).and_then(|source| {
                let destination = self.batch_path(&entry.destination)?;
                self.ensure_write_allowed(&entry.destination.vault, &destination)?;
                ensure_parent(&destination)?;
                copy_verified(&source, &destination, &entry.revision)
            });
            if !advance(&mut details[step], result) {
                return;
            }
            step += 1;
        }
        for entry in &plan.notes {
            let result = self.batch_path(&entry.write).and_then(|path| {
                self.ensure_write_allowed(&entry.write.vault, &path)?;
                let bytes = read_bytes(&path)?;
                check_note_revision(&path, &bytes, &entry.revision)?;
                atomic_write(&path, entry.content.as_bytes())
            });
            if !advance(&mut details[step], result) {
                return;
            }
            step += 1;
        }
        for entry in &plan.boards {
            if !advance(&mut details[step], self.apply_canvas_link_update(entry)) {
                return;
            }
            step += 1;
        }
    }

    fn reserve_preview(&self, id: &str) -> Result<(), VaultError> {
        let parent = self
            .lock_path
            .as_deref()
            .and_then(Path::parent)
            .ok_or_else(|| invalid("The write-lock directory is required to apply a preview."))?;
        let dir = parent.join("batch-move-previews");
        fs::create_dir_all(&dir)
            .map_err(|error| VaultError::io(dir.display().to_string(), error))?;
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) == Some("used")
                    && entry.file_type().is_ok_and(|kind| kind.is_file())
                    && entry
                        .metadata()
                        .and_then(|meta| meta.modified())
                        .is_ok_and(|modified| modified.elapsed().is_ok_and(|age| age > PREVIEW_TTL))
                {
                    let _ = fs::remove_file(path);
                }
            }
        }
        let path = dir.join(format!("{id}.used"));
        atomic_write_new(
            &path, b"used
",
        )
        .map_err(|error| match error {
            VaultError::Io(_, ref source) if source.kind() == ErrorKind::AlreadyExists => invalid(
                "This preview was already applied or attempted. Preview the remaining moves again.",
            ),
            other => other,
        })
    }

    fn journal_path(&self, id: &str) -> Result<PathBuf, VaultError> {
        let parent = self
            .lock_path
            .as_deref()
            .and_then(Path::parent)
            .ok_or_else(|| {
                invalid("The write-lock directory is required for a recoverable exchange.")
            })?;
        let dir = parent.join("batch-move-journals");
        fs::create_dir_all(&dir).map_err(|e| VaultError::io(dir.display().to_string(), e))?;
        Ok(dir.join(format!("{id}.json")))
    }

    fn apply_exchange(
        &self,
        plan: &Plan,
        id: &str,
        details: &mut [BatchMoveDetail],
    ) -> Result<(), VaultError> {
        let journal_path = self.journal_path(id)?;
        let mut journal = Journal {
            id: id.into(),
            staged: false,
            entries: plan
                .moves
                .iter()
                .map(|entry| {
                    Ok(JournalEntry {
                        request: BatchNoteMove {
                            source_path: entry.from.path.clone(),
                            source_vault: entry.from.vault.clone(),
                            destination_path: entry.to.path.clone(),
                            destination_vault: entry.to.vault.clone(),
                        },
                        source_root: canonical_root(self.resolve_vault(&entry.from.vault)?)?,
                        destination_root: canonical_root(self.resolve_vault(&entry.to.vault)?)?,
                        revision: entry.revision.clone(),
                    })
                })
                .collect::<Result<_, VaultError>>()?,
        };
        let paths = self.journal_entries(&journal)?;
        if let Some(paths) = paths.iter().find(|paths| exists(&paths.2)) {
            return Err(invalid(format!(
                "Recovery stage already exists: {}.",
                paths.2.display()
            )));
        }
        write_journal(&journal_path, &journal, true)?;
        let describe_error = |error: VaultError| {
            invalid(format!(
                "{error} Exchange stopped. Recovery journal: {}.",
                journal_path.display()
            ))
        };
        for ((entry, (source, _, stage)), detail) in
            journal.entries.iter().zip(&paths).zip(&mut *details)
        {
            move_verified(source, stage, &entry.revision).map_err(describe_error)?;
            detail.status = "temporarily_staged";
            detail.description = Some(format!("Temporary file: {}.", stage.display()));
        }
        journal.staged = true;
        write_journal(&journal_path, &journal, false).map_err(describe_error)?;
        for ((entry, (_, destination, stage)), detail) in
            journal.entries.iter().zip(&paths).zip(&mut *details)
        {
            if let Err(error) = move_verified(stage, destination, &entry.revision) {
                detail.status = "failed";
                return Err(describe_error(error));
            }
            detail.status = "completed";
            detail.description = None;
        }
        fs::remove_file(&journal_path)
            .map_err(|e| VaultError::io(journal_path.display().to_string(), e))?;
        Ok(())
    }

    fn journal_entries(
        &self,
        journal: &Journal,
    ) -> Result<Vec<(PathBuf, PathBuf, PathBuf)>, VaultError> {
        if journal.id.len() != 64 || !journal.id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid("Invalid batch recovery journal ID."));
        }
        let configured: BTreeMap<_, _> = self
            .list_vaults()
            .into_iter()
            .filter_map(|(name, root)| canonical_root(&root).ok().map(|root| (root, name)))
            .collect();
        let mut result = Vec::new();
        for (index, entry) in journal.entries.iter().enumerate() {
            let Some(source_vault) = configured.get(&entry.source_root) else {
                return Err(invalid("Source vault is unavailable for recovery."));
            };
            let Some(destination_vault) = configured.get(&entry.destination_root) else {
                return Err(invalid("Destination vault is unavailable for recovery."));
            };
            let source = self.batch_path(&Location::new(
                source_vault,
                entry.request.source_path.clone(),
            ))?;
            let destination = self.batch_path(&Location::new(
                destination_vault,
                entry.request.destination_path.clone(),
            ))?;
            let stage =
                source.with_file_name(format!(".obsidian-mcp-swap-{}-{index}.tmp", journal.id));
            self.ensure_write_allowed(source_vault, &source)?;
            self.ensure_write_allowed(destination_vault, &destination)?;
            self.ensure_write_allowed(source_vault, &stage)?;
            result.push((source, destination, stage));
        }
        Ok(result)
    }

    pub fn recover_batch_moves(&self) -> Vec<String> {
        let _guard = self.write_guard();
        let Some(parent) = self.lock_path.as_deref().and_then(Path::parent) else {
            return Vec::new();
        };
        let dir = parent.join("batch-move-journals");
        let files = match fs::read_dir(&dir) {
            Ok(files) => files,
            Err(error) if error.kind() == ErrorKind::NotFound => return Vec::new(),
            Err(error) => return vec![format!("Cannot list batch recovery journals: {error}")],
        };
        let mut messages = Vec::new();
        for file in files {
            let file = match file {
                Ok(file) => file,
                Err(error) => {
                    messages.push(error.to_string());
                    continue;
                }
            };
            let path = file.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json")
                || !file.file_type().is_ok_and(|kind| kind.is_file())
            {
                continue;
            }
            let result = read_bytes(&path).and_then(|bytes| {
                let mut journal: Journal =
                    serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
                if path.file_stem().and_then(|s| s.to_str()) != Some(&journal.id) {
                    return Err(invalid("Journal filename does not match the exchange ID."));
                }
                let paths = self.journal_entries(&journal)?;
                recover_exchange(&path, &mut journal, &paths)?;
                fs::remove_file(&path).map_err(|e| VaultError::io(path.display().to_string(), e))
            });
            match result {
                Ok(()) => messages.push(format!(
                    "Restored an interrupted note exchange: {}.",
                    path.display()
                )),
                Err(error) => messages.push(format!(
                    "Could not recover {}: {error} The recovery files were retained.",
                    path.display()
                )),
            }
        }
        messages
    }
}

#[derive(Deserialize, Serialize)]
struct Journal {
    id: String,
    staged: bool,
    entries: Vec<JournalEntry>,
}

#[derive(Deserialize, Serialize)]
struct JournalEntry {
    request: BatchNoteMove,
    source_root: PathBuf,
    destination_root: PathBuf,
    revision: String,
}

fn write_journal(path: &Path, journal: &Journal, new: bool) -> Result<(), VaultError> {
    let bytes = serde_json::to_vec(journal).map_err(|e| invalid(e.to_string()))?;
    atomic_write_durable(path, &bytes, new)
}

fn recover_exchange(
    journal_path: &Path,
    journal: &mut Journal,
    paths: &[(PathBuf, PathBuf, PathBuf)],
) -> Result<(), VaultError> {
    let mut transferred = BTreeSet::new();
    if journal.staged {
        for (entry, (_, destination, stage)) in journal.entries.iter().zip(paths) {
            if exists(stage) {
                verify(stage, &entry.revision)?;
                if exists(destination) {
                    return Err(invalid(format!(
                        "The staged file '{}' and destination '{}' both exist.",
                        stage.display(),
                        destination.display()
                    )));
                }
            } else {
                verify(destination, &entry.revision)?;
                transferred.insert(path_key(destination));
            }
        }
    }
    for (entry, (source, _, stage)) in journal.entries.iter().zip(paths) {
        if journal.staged {
            if exists(source) && !transferred.contains(&path_key(source)) {
                return Err(invalid(format!(
                    "Original path {} has an unexpected file. Stage: {}.",
                    source.display(),
                    stage.display()
                )));
            }
        } else if exists(stage) {
            verify(stage, &entry.revision)?;
            if exists(source) {
                return Err(invalid(format!(
                    "Original path {} is occupied. Stage: {}.",
                    source.display(),
                    stage.display()
                )));
            }
        } else {
            verify(source, &entry.revision)?;
        }
    }
    if journal.staged {
        for (entry, (_, destination, stage)) in journal.entries.iter().zip(paths) {
            if !exists(stage) {
                move_verified(destination, stage, &entry.revision)?;
            }
        }
        journal.staged = false;
        write_journal(journal_path, journal, false)?;
    }
    for (entry, (source, _, stage)) in journal.entries.iter().zip(paths) {
        if exists(stage) {
            move_verified(stage, source, &entry.revision)?;
        }
    }
    Ok(())
}

fn advance(detail: &mut BatchMoveDetail, result: Result<(), VaultError>) -> bool {
    match result {
        Ok(()) => {
            detail.status = "completed";
            true
        }
        Err(error) => {
            detail.status = "failed";
            detail.description = Some(error.to_string());
            false
        }
    }
}

fn exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

fn verify(path: &Path, revision: &str) -> Result<(), VaultError> {
    if stream_revision(path)? != revision {
        return Err(VaultError::RevisionConflict(path.display().to_string()));
    }
    Ok(())
}

fn move_verified(source: &Path, destination: &Path, revision: &str) -> Result<(), VaultError> {
    verify(source, revision)?;
    ensure_parent(destination)?;
    match rename_no_replace(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::CrossesDevices => {
            copy_verified(source, destination, revision)?;
            verify(destination, revision)?;
            verify(source, revision)?;
            fs::remove_file(source).map_err(|e| {
                invalid(format!(
                    "Copies exist at '{}' and '{}'. Could not remove the source: {e}.",
                    source.display(),
                    destination.display()
                ))
            })?;
            Ok(())
        }
        Err(error) => Err(VaultError::io(destination.display().to_string(), error)),
    }
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, VaultError> {
    fs::read(path).map_err(|e| VaultError::io(path.display().to_string(), e))
}

fn read_utf8(path: &Path) -> Result<String, VaultError> {
    fs::read_to_string(path).map_err(|e| VaultError::io(path.display().to_string(), e))
}

const PREVIEW_TTL: Duration = Duration::from_secs(24 * 60 * 60);

fn epoch_time() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

fn new_preview_id(digest: &str) -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let time = epoch_time();
    let stamp = time.as_secs();
    let unique = format!(
        "{}:{}:{}",
        time.as_nanos(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    format!("{stamp}.{}.{digest}", plan_key(unique.as_bytes()))
}

fn matches_preview(preview: &str, digest: &str) -> bool {
    let mut parts = preview.split('.');
    let Some(stamp) = parts.next().and_then(|stamp| stamp.parse::<u64>().ok()) else {
        return false;
    };
    let now = epoch_time().as_secs();
    stamp <= now
        && now - stamp < PREVIEW_TTL.as_secs()
        && parts.next().is_some_and(|nonce| {
            nonce.len() == 64 && nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        && parts.next() == Some(digest)
        && parts.next().is_none()
}

fn plan_key(bytes: &[u8]) -> String {
    note_revision(bytes)
        .trim_start_matches("sha256:")
        .to_owned()
}

fn canonical_root(path: &Path) -> Result<PathBuf, VaultError> {
    path.canonicalize()
        .map_err(|e| VaultError::io(path.display().to_string(), e))
}

fn path_key(path: &Path) -> String {
    let absolute = projected_path(path)
        .or_else(|_| {
            std::path::absolute(path)
                .map_err(|error| VaultError::io(path.display().to_string(), error))
        })
        .unwrap_or_else(|_| path.to_owned());
    let text = absolute.to_string_lossy().into_owned();
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        text.to_lowercase()
    } else {
        text
    }
}

fn invalid(message: impl Into<String>) -> VaultError {
    VaultError::MissingArgument(message.into())
}

fn normalize_join(parent: &Path, target: &str) -> Option<String> {
    let target = target.replace('\\', "/");
    let mut parts: Vec<_> = if target.starts_with('/') {
        Vec::new()
    } else {
        parent
            .components()
            .filter_map(|component| match component {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .map(str::to_owned)
            .collect()
    };
    for component in target.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            name => parts.push(name.into()),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn resolve_note_link(
    link: &Link,
    parent: &Path,
    known: &BTreeSet<String>,
) -> Result<Option<String>, Vec<String>> {
    let target = super::ensure_md_extension(&link.target);
    let candidates = if link.kind == LinkKind::Markdown || !target.contains('/') {
        [
            normalize_join(parent, &target),
            normalize_join(Path::new(""), &target),
        ]
    } else {
        [
            normalize_join(Path::new(""), &target),
            normalize_join(parent, &target),
        ]
    };
    for candidate in candidates.into_iter().flatten() {
        if let Some(path) = known.get(&candidate) {
            return Ok(Some(path.clone()));
        }
        if cfg!(any(target_os = "windows", target_os = "macos")) {
            let same: Vec<_> = known
                .iter()
                .filter(|path| path.to_lowercase() == candidate.to_lowercase())
                .cloned()
                .collect();
            match &same[..] {
                [path] => return Ok(Some(path.clone())),
                [] => {}
                _ => return Err(same),
            }
        }
    }
    if link.kind == LinkKind::Wiki && !target.contains('/') {
        let matches: Vec<_> = known
            .iter()
            .filter(|path| {
                path.rsplit('/')
                    .next()
                    .is_some_and(|name| name.to_lowercase() == target.to_lowercase())
            })
            .cloned()
            .collect();
        match &matches[..] {
            [] => Ok(None),
            [name] => Ok(Some(name.clone())),
            _ => Err(matches),
        }
    } else {
        Ok(None)
    }
}

fn resolve_asset(root: &Path, parent: &Path, link: &Link) -> Result<Option<String>, VaultError> {
    if Path::new(&link.target).extension().is_none() || link.target.ends_with(".md") {
        return Ok(None);
    }
    let tries = if link.kind == LinkKind::Wiki && link.target.contains('/') {
        [
            normalize_join(Path::new(""), &link.target),
            normalize_join(parent, &link.target),
        ]
    } else {
        [
            normalize_join(parent, &link.target),
            normalize_join(Path::new(""), &link.target),
        ]
    };
    for target in tries.into_iter().flatten() {
        if validate_relative(&target).is_ok() && safe_join(root, None, &target)?.is_file() {
            return Ok(Some(target));
        }
    }
    if link.kind != LinkKind::Wiki || link.target.contains('/') {
        return Ok(None);
    }
    let mut result = Vec::new();
    for entry in ignore::WalkBuilder::new(root).follow_links(false).build() {
        let entry = entry.map_err(|error| invalid(error.to_string()))?;
        if entry.file_type().is_some_and(|kind| kind.is_file())
            && entry
                .path()
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == link.target)
        {
            result.push(rel_path(root, entry.path()));
        }
    }
    match &result[..] {
        [] => Ok(None),
        [path] => Ok(Some(path.clone())),
        _ => Err(invalid(format!(
            "Attachment '{}' has multiple matches.",
            link.target
        ))),
    }
}

fn format_internal(link: &Link, from: &Location, target: &str) -> String {
    let bang = if link.embed { "!" } else { "" };
    let anchor = link
        .heading
        .as_ref()
        .map_or_else(String::new, |value| format!("#{value}"));
    match link.kind {
        LinkKind::Wiki => {
            let path = target.strip_suffix(".md").unwrap_or(target);
            let alias = link
                .alias
                .as_ref()
                .map_or_else(String::new, |value| format!("|{value}"));
            format!("{bang}[[{path}{anchor}{alias}]]")
        }
        LinkKind::Markdown => {
            let relative = relative_link(&from.path, target);
            let path = relative
                .split('/')
                .map(encode_uri)
                .collect::<Vec<_>>()
                .join("/");
            let anchor = link
                .heading
                .as_ref()
                .map_or_else(String::new, |value| format!("#{}", encode_uri(value)));
            format!("{bang}[{}]({path}{anchor})", link_label(link))
        }
    }
}

pub(super) fn relative_link(from: &str, target: &str) -> String {
    let mut from: Vec<_> = from.split('/').collect();
    from.pop();
    let target: Vec<_> = target.split('/').collect();
    let shared = from.iter().zip(&target).take_while(|(a, b)| a == b).count();
    std::iter::repeat_n("..", from.len() - shared)
        .chain(target.iter().skip(shared).copied())
        .collect::<Vec<_>>()
        .join("/")
}

fn link_label(link: &Link) -> String {
    match (link.kind, &link.alias) {
        (LinkKind::Markdown, Some(alias)) => alias.clone(),
        (LinkKind::Wiki, Some(alias)) => markdown_label(alias),
        _ => markdown_label(link.target.rsplit('/').next().unwrap_or(&link.target)),
    }
}

pub(super) fn attachment_links(content: &str) -> Vec<Link> {
    static PATTERN: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r"(!?)\[([^\]\n]*)\]\((?:<([^<>\n]+)>|([^\s)]+))\)")
            .expect("static attachment link pattern")
    });
    let spans = links::code_spans(content);
    PATTERN
        .captures_iter(content)
        .filter_map(|captures| {
            let span = captures.get(0)?.range();
            if links::in_code(&spans, span.start) {
                return None;
            }
            let raw = captures.get(3).or_else(|| captures.get(4))?.as_str();
            if raw.contains(':') || raw.starts_with('#') {
                return None;
            }
            let (target, heading) = raw
                .split_once('#')
                .map_or((raw, None), |(head, tail)| (head, Some(tail)));
            let target = decode_percent(target);
            if target.ends_with(".md") || Path::new(&target).extension().is_none() {
                return None;
            }
            Some(Link {
                line: content[..span.start]
                    .bytes()
                    .filter(|&byte| byte == b'\n')
                    .count()
                    + 1,
                span,
                target,
                heading: heading.map(decode_percent),
                alias: Some(captures.get(2)?.as_str().into()),
                embed: !captures.get(1)?.as_str().is_empty(),
                kind: LinkKind::Markdown,
            })
        })
        .collect()
}

pub(super) fn decode_percent(value: &str) -> String {
    fn nibble(byte: u8) -> Option<u8> {
        (byte as char).to_digit(16).map(|value| value as u8)
    }
    let bytes = value.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if let [b'%', hi, lo, ..] = &bytes[cursor..]
            && let (Some(hi), Some(lo)) = (nibble(*hi), nibble(*lo))
        {
            result.push(hi * 16 + lo);
            cursor += 3;
        } else {
            result.push(bytes[cursor]);
            cursor += 1;
        }
    }
    String::from_utf8(result).unwrap_or_else(|_| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (tempfile::TempDir, VaultManager) {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("Work");
        let destination = dir.path().join("Archive");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        let vault =
            VaultManager::with_lock_path(vec![source, destination], dir.path().join("lock"));
        (dir, vault)
    }

    fn params(moves: serde_json::Value) -> BatchMoveParams {
        serde_json::from_value(json!({"moves": moves})).unwrap()
    }

    #[test]
    fn batch_move_rechecks_plan_and_moves_without_overwrite() {
        let (dir, manager) = fixture();
        let current = dir.path().join("Work/A.md");
        fs::write(&current, "first").unwrap();
        let mut options = params(
            json!([{"sourceVault":"Work","sourcePath":"A.md","destinationVault":"Work","destinationPath":"B.md"}]),
        );
        let preview = manager.batch_move_notes(&options).unwrap();
        assert!(preview.blockers.is_empty());
        fs::write(&current, "changed").unwrap();
        options.preview_id = Some(preview.preview_id);
        let stale = manager.batch_move_notes(&options).unwrap();
        assert!(!stale.applied);
        assert!(!dir.path().join("Work/B.md").exists());
        options.preview_id = Some(stale.preview_id);
        let applied = manager.batch_move_notes(&options).unwrap();
        assert!(applied.applied);
        assert_eq!(applied.changes[0].status, "completed");
        assert_eq!(
            fs::read_to_string(dir.path().join("Work/B.md")).unwrap(),
            "changed"
        );
    }

    #[test]
    fn batch_cross_vault_links_and_attachments_require_consent() {
        let (dir, manager) = fixture();
        fs::write(
            dir.path().join("Work/A.md"),
            "![[pic.png]]\n![[B#^heading]]\n",
        )
        .unwrap();
        fs::write(dir.path().join("Work/B.md"), "[label](A.md#Heading)").unwrap();
        fs::write(dir.path().join("Work/pic.png"), [1, 2, 3]).unwrap();
        let mut options = params(
            json!([{"sourceVault":"Work","sourcePath":"A","destinationVault":"Archive","destinationPath":"Imported/A"}]),
        );
        let blocked = manager.batch_move_notes(&options).unwrap();
        assert_eq!(blocked.blockers.len(), 2);
        options.allow_attachment_copy = true;
        options.allow_embed_conversion = true;
        let preview = manager.batch_move_notes(&options).unwrap();
        assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
        options.preview_id = Some(preview.preview_id);
        assert!(
            manager
                .batch_move_notes(&options)
                .unwrap()
                .changes
                .iter()
                .all(|change| change.status == "completed")
        );
        let moved = fs::read_to_string(dir.path().join("Archive/Imported/A.md")).unwrap();
        assert!(moved.contains("![[pic.png]]"));
        assert!(moved.contains("obsidian://open"));
        assert!(moved.contains("%23%5Eheading"));
        assert_eq!(
            fs::read(dir.path().join("Archive/pic.png")).unwrap(),
            [1, 2, 3]
        );
        assert!(dir.path().join("Work/pic.png").exists());
    }

    #[test]
    fn batch_swaps_paths_without_swapping_link_targets() {
        let (dir, manager) = fixture();
        fs::write(dir.path().join("Work/A.md"), "A [[B]]").unwrap();
        fs::write(dir.path().join("Work/B.md"), "B [[A]]").unwrap();
        let mut options = params(json!([
            {"sourceVault":"Work","sourcePath":"A","destinationVault":"Work","destinationPath":"B"},
            {"sourceVault":"Work","sourcePath":"B","destinationVault":"Work","destinationPath":"A"}
        ]));
        let preview = manager.batch_move_notes(&options).unwrap();
        assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
        options.preview_id = Some(preview.preview_id);
        let result = manager.batch_move_notes(&options).unwrap();
        assert!(
            result
                .changes
                .iter()
                .all(|change| change.status == "completed"),
            "{result:?}"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("Work/A.md")).unwrap(),
            "B [[B]]"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("Work/B.md")).unwrap(),
            "A [[A]]"
        );
    }

    #[test]
    fn batch_recovery_restores_a_partially_published_swap() {
        let (dir, manager) = fixture();
        let root = dir.path().join("Work");
        let sources = [root.join("A.md"), root.join("B.md")];
        fs::write(&sources[0], "first").unwrap();
        fs::write(&sources[1], "second").unwrap();
        let id = plan_key(b"recovery");
        let request = |source: &str, destination: &str| BatchNoteMove {
            source_vault: "Work".into(),
            source_path: source.into(),
            destination_vault: "Work".into(),
            destination_path: destination.into(),
        };
        let journal = Journal {
            id: id.clone(),
            staged: true,
            entries: vec![
                JournalEntry {
                    request: request("A.md", "B.md"),
                    source_root: canonical_root(&root).unwrap(),
                    destination_root: canonical_root(&root).unwrap(),
                    revision: note_revision(b"first"),
                },
                JournalEntry {
                    request: request("B.md", "A.md"),
                    source_root: canonical_root(&root).unwrap(),
                    destination_root: canonical_root(&root).unwrap(),
                    revision: note_revision(b"second"),
                },
            ],
        };
        let journal_path = manager.journal_path(&id).unwrap();
        let paths = manager.journal_entries(&journal).unwrap();
        fs::rename(&sources[1], &paths[1].2).unwrap();
        fs::rename(&sources[0], &sources[1]).unwrap();
        write_journal(&journal_path, &journal, true).unwrap();
        let messages = manager.recover_batch_moves();
        assert!(messages[0].contains("Restored"), "{messages:?}");
        assert_eq!(fs::read_to_string(&sources[0]).unwrap(), "first");
        assert_eq!(fs::read_to_string(&sources[1]).unwrap(), "second");
        assert!(!journal_path.exists());
    }

    #[test]
    fn batch_preview_cannot_be_replayed_even_when_note_bytes_are_identical() {
        let (dir, manager) = fixture();
        fs::write(dir.path().join("Work/A.md"), "same").unwrap();
        fs::write(dir.path().join("Work/B.md"), "same").unwrap();
        let mut options = params(json!([
            {"sourceVault":"Work","sourcePath":"A","destinationVault":"Work","destinationPath":"B"},
            {"sourceVault":"Work","sourcePath":"B","destinationVault":"Work","destinationPath":"A"}
        ]));
        let preview = manager.batch_move_notes(&options).unwrap();
        options.preview_id = Some(preview.preview_id);
        let applied = manager.batch_move_notes(&options).unwrap();
        assert!(applied.applied, "{applied:?}");
        let replay = manager.batch_move_notes(&options).unwrap();
        assert!(!replay.applied);
        assert!(
            replay
                .blockers
                .iter()
                .any(|b| b.contains("already applied")),
            "{replay:?}"
        );
    }

    #[test]
    fn batch_keeps_an_occupied_source_for_manual_recovery() {
        let (dir, manager) = fixture();
        let root = dir.path().join("Work");
        let original = root.join("A.md");
        let destination = root.join("B.md");
        let id = plan_key(b"occupied");
        let journal = Journal {
            id: id.clone(),
            staged: false,
            entries: vec![JournalEntry {
                request: BatchNoteMove {
                    source_vault: "Work".into(),
                    source_path: "A.md".into(),
                    destination_vault: "Work".into(),
                    destination_path: "B.md".into(),
                },
                source_root: canonical_root(&root).unwrap(),
                destination_root: canonical_root(&root).unwrap(),
                revision: note_revision(b"original"),
            }],
        };
        let journal_path = manager.journal_path(&id).unwrap();
        let staged = manager.journal_entries(&journal).unwrap().pop().unwrap().2;
        fs::write(&original, "user edit").unwrap();
        fs::write(&staged, "original").unwrap();
        write_journal(&journal_path, &journal, true).unwrap();
        let messages = manager.recover_batch_moves();
        assert!(messages[0].contains("occupied"), "{messages:?}");
        assert_eq!(fs::read_to_string(&original).unwrap(), "user edit");
        assert_eq!(fs::read_to_string(&staged).unwrap(), "original");
        assert!(!destination.exists());
        assert!(journal_path.exists());
    }

    #[test]
    fn batch_cross_vault_canvas_file_card_needs_consent_and_keeps_geometry() {
        let (dir, manager) = fixture();
        fs::write(dir.path().join("Work/A.md"), "# Heading").unwrap();
        fs::write(dir.path().join("Work/Board.canvas"), r##"{"nodes":[{"id":"n","type":"file","file":"A.md","subpath":"#Heading","x":3,"y":5,"width":320,"height":180,"plugin":"kept"}],"edges":[{"id":"e","fromNode":"n","toNode":"n"}]}"##).unwrap();
        let mut options = params(json!([
            {"sourceVault":"Work","sourcePath":"A","destinationVault":"Archive","destinationPath":"A"}
        ]));
        assert!(
            manager.batch_move_notes(&options).unwrap().blockers[0].contains("Canvas file card")
        );
        options.allow_canvas_conversion = true;
        let preview = manager.batch_move_notes(&options).unwrap();
        assert!(preview.blockers.is_empty());
        assert!(
            preview
                .changes
                .iter()
                .any(|change| change.kind == "update_canvas" && change.description.is_some())
        );
        options.preview_id = Some(preview.preview_id);
        let result = manager.batch_move_notes(&options).unwrap();
        assert!(result.applied, "{result:?}");
        let board: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join("Work/Board.canvas")).unwrap())
                .unwrap();
        assert_eq!(board["nodes"][0]["id"], "n");
        assert_eq!(board["nodes"][0]["type"], "text");
        assert_eq!(board["nodes"][0]["plugin"], "kept");
        assert_eq!(board["nodes"][0]["x"], 3);
        assert!(
            board["nodes"][0]["text"]
                .as_str()
                .unwrap()
                .contains("vault=Archive&file=A.md%23Heading")
        );
        assert_eq!(board["edges"][0]["fromNode"], "n");
    }

    #[test]
    fn batch_parses_attachment_paths_in_angles_with_spaces() {
        let links = attachment_links(
            "![alt](<photos/my diagram.png>)
![alt](pics%20now.png)",
        );
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "photos/my diagram.png");
        assert_eq!(links[1].target, "pics now.png");
    }
}
