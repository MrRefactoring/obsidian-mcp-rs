use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Component, Path, PathBuf},
};

use serde::Serialize;

use crate::error::VaultError;

use super::{
    LinkKind, VaultManager,
    batch_move::{attachment_links, decode_percent, relative_link},
    canvas::encode_uri,
    links, note_revision,
    path::{normalize_relative, projected_path},
    safe_join,
    write::{atomic_write_new, copy_verified, stream_revision},
};

pub struct ImportRequest {
    pub source: PathBuf,
    pub vault: String,
    pub folder: String,
    pub exclude: Vec<String>,
    pub allow_missing_attachments: bool,
    pub read_only: bool,
    pub connection_revision: String,
}

#[derive(Debug, Serialize)]
pub struct ImportReport {
    pub plan_id: String,
    pub applied: bool,
    pub files: Vec<ImportFile>,
    pub link_changes: Vec<LinkChange>,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ImportFile {
    pub source: String,
    pub destination: String,
    pub kind: &'static str,
    pub bytes: u64,
    pub status: &'static str,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LinkChange {
    pub note: String,
    pub line: usize,
    pub from: String,
    pub to: String,
}

struct Entry {
    source: String,
    destination: String,
    revision: String,
    bytes: u64,
    note: Option<String>,
}

struct Plan {
    source: PathBuf,
    entries: Vec<Entry>,
    report: ImportReport,
}

impl VaultManager {
    pub fn import_markdown(
        &self,
        request: &ImportRequest,
        apply: Option<&str>,
    ) -> Result<ImportReport, VaultError> {
        let _guard = apply.map(|_| self.write_guard());
        let mut plan = self.plan_import(request)?;
        let Some(expected) = apply else {
            return Ok(plan.report);
        };
        if expected != plan.report.plan_id {
            plan.report
                .blockers
                .push("The import changed. Preview and confirm the new plan.".into());
        }
        if !plan.report.blockers.is_empty() {
            return Ok(plan.report);
        }
        let root = self.resolve_vault(&request.vault)?;
        for file in &mut plan.report.files {
            file.status = "not_started";
        }
        for (entry, file) in plan.entries.iter().zip(&mut plan.report.files) {
            let written = (|| {
                let source = source_file(&plan.source, &entry.source)?;
                let destination = safe_join(root, None, &entry.destination)?;
                self.ensure_write_allowed(&request.vault, &destination)?;
                let parent = destination
                    .parent()
                    .ok_or_else(|| invalid("Missing output folder"))?;
                self.ensure_write_allowed(&request.vault, parent)?;
                fs::create_dir_all(parent).map_err(|error| file_error(parent, error))?;
                if let Some(note) = &entry.note {
                    if stream_revision(&source)? != entry.revision {
                        return Err(VaultError::RevisionConflict(entry.source.clone()));
                    }
                    atomic_write_new(&destination, note.as_bytes())
                } else {
                    copy_verified(&source, &destination, &entry.revision)
                }
            })();
            match written {
                Ok(()) => file.status = "written",
                Err(error) => {
                    file.status = "failed";
                    file.error = Some(error.to_string());
                    return Ok(plan.report);
                }
            }
        }
        plan.report.applied = true;
        Ok(plan.report)
    }

    fn plan_import(&self, request: &ImportRequest) -> Result<Plan, VaultError> {
        let root = self.resolve_vault(&request.vault)?;
        let source =
            fs::canonicalize(&request.source).map_err(|e| file_error(&request.source, e))?;
        if !source.is_dir() {
            return Err(invalid("The import source must be a local folder"));
        }
        let folder = normalize_relative(Path::new(&request.folder))?;
        let folder = portable_path(&folder)?;
        let target = safe_join(root, None, &folder)?;
        if projected_path(&target)?.starts_with(&source) {
            return Err(invalid("The destination must be outside the source folder"));
        }
        let exclusions = request
            .exclude
            .iter()
            .map(|path| normalize_relative(Path::new(path)).and_then(|path| portable_path(&path)))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if exclusions.contains("") {
            return Err(invalid(
                "Exclude individual files or subfolders instead of the source root",
            ));
        }
        let inventory = source_inventory(&source)?;
        let notes: Vec<_> = inventory
            .iter()
            .filter(|path| is_markdown(path) && !excluded(path, &exclusions))
            .cloned()
            .collect();
        let mut report = ImportReport {
            plan_id: String::new(),
            applied: false,
            files: Vec::new(),
            link_changes: Vec::new(),
            warnings: Vec::new(),
            blockers: Vec::new(),
        };
        if notes.is_empty() {
            report
                .blockers
                .push("No Markdown notes were selected.".into());
        }
        if request.read_only {
            report
                .blockers
                .push("This connection uses --no-edit.".into());
        }
        let mut assets = BTreeSet::new();
        let mut entries = Vec::new();
        for note in notes {
            let original_path = source_file(&source, &note)?;
            let original =
                fs::read_to_string(&original_path).map_err(|e| file_error(&original_path, e))?;
            let transformed = rewrite_note(
                &original,
                &note,
                &folder,
                &inventory,
                &exclusions,
                root,
                request.allow_missing_attachments,
                &mut assets,
                &mut report,
            );
            entries.push(Entry {
                destination: join_portable(&folder, &note),
                source: note,
                bytes: transformed.len() as u64,
                revision: note_revision(original.as_bytes()),
                note: Some(transformed),
            });
        }
        for asset in assets {
            let file = source_file(&source, &asset)?;
            entries.push(Entry {
                destination: join_portable(&folder, &asset),
                source: asset,
                bytes: fs::metadata(&file)
                    .map_err(|error| file_error(&file, error))?
                    .len(),
                revision: stream_revision(&file)?,
                note: None,
            });
        }
        entries.sort_by(|a, b| a.destination.cmp(&b.destination));
        let mut destinations = BTreeMap::new();
        for entry in &entries {
            let target = safe_join(root, None, &entry.destination)?;
            if let Some(previous) =
                destinations.insert(entry.destination.to_lowercase(), &entry.destination)
            {
                report.blockers.push(format!(
                    "Destination names collide: {previous} and {}",
                    entry.destination
                ));
            }
            match fs::symlink_metadata(&target) {
                Ok(_) => report
                    .blockers
                    .push(format!("Destination already exists: {}", entry.destination)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => report.blockers.push(file_error(&target, error).to_string()),
            }
            if let Err(error) = self.ensure_write_allowed(&request.vault, &target) {
                report.blockers.push(error.to_string());
            }
            if let Some(parent) = target.parent()
                && let Err(error) = self.ensure_write_allowed(&request.vault, parent)
            {
                report.blockers.push(error.to_string());
            }
            report.files.push(ImportFile {
                source: entry.source.clone(),
                destination: entry.destination.clone(),
                kind: if entry.note.is_some() {
                    "note"
                } else {
                    "attachment"
                },
                bytes: entry.bytes,
                status: "planned",
                error: None,
            });
        }
        report.blockers.sort();
        report.blockers.dedup();
        report.warnings.sort();
        report.warnings.dedup();
        let changes: Vec<_> = entries
            .iter()
            .map(|entry| (&entry.source, &entry.destination, &entry.revision))
            .collect();
        let fingerprint = serde_json::json!({
            "source": &source,
            "vault": projected_path(root)?,
            "folder": folder,
            "connection": &request.connection_revision,
            "inventory": inventory,
            "excluded": exclusions,
            "allowMissing": request.allow_missing_attachments,
            "entries": changes,
            "report": &report,
        });
        report.plan_id =
            note_revision(&serde_json::to_vec(&fingerprint).expect("serialize import plan"));
        Ok(Plan {
            source,
            entries,
            report,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn rewrite_note(
    original: &str,
    note: &str,
    folder: &str,
    inventory: &BTreeSet<String>,
    exclusions: &BTreeSet<String>,
    target_root: &Path,
    allow_missing: bool,
    assets: &mut BTreeSet<String>,
    report: &mut ImportReport,
) -> String {
    let mut references: Vec<_> = links::parse_links(original)
        .into_iter()
        .map(|link| (link, false))
        .chain(
            attachment_links(original)
                .into_iter()
                .map(|link| (link, true)),
        )
        .collect();
    references.sort_by_key(|(link, attachment)| (link.span.start, !attachment));
    references.dedup_by(|(a, _), (b, _)| a.span == b.span);
    let parent = Path::new(note).parent().unwrap_or_else(|| Path::new(""));
    let mut edits = Vec::new();
    for (mut reference, markdown_attachment) in references {
        if reference.kind == LinkKind::Markdown
            && !markdown_attachment
            && let Some((_, raw)) = original[reference.span.clone()].rsplit_once("](")
            && let Some(raw) = raw.strip_suffix(')')
        {
            let (target, heading) = raw
                .split_once('#')
                .map_or((raw, None), |(target, heading)| (target, Some(heading)));
            reference.target = decode_percent(target);
            reference.heading = heading.map(decode_percent);
        }
        if reference.target.contains(':') || reference.target.starts_with("//") {
            continue;
        }
        let mut attachment = markdown_attachment
            || (reference.kind == LinkKind::Wiki
                && Path::new(&reference.target).extension().is_some()
                && !is_markdown(&reference.target));
        let mut found = resolve_source(
            inventory,
            parent,
            &reference.target,
            reference.kind,
            attachment,
        );
        if found.is_empty() && attachment && reference.kind == LinkKind::Wiki {
            found = resolve_source(inventory, parent, &reference.target, reference.kind, false);
            if !found.is_empty() {
                attachment = false;
            }
        }
        match found.as_slice() {
            [path] if !excluded(path, exclusions) => {
                if !is_markdown(path) {
                    assets.insert(path.clone());
                }
                let destination = join_portable(folder, path);
                let rewrite_wikilink = reference.kind == LinkKind::Wiki
                    && reference.target != destination
                    && reference.target != destination.strip_suffix(".md").unwrap_or(&destination);
                let rewrite_root_link = reference.kind == LinkKind::Markdown
                    && reference.target.starts_with('/')
                    && !folder.is_empty();
                if rewrite_wikilink || rewrite_root_link {
                    let replacement =
                        relink(&reference, &join_portable(folder, note), &destination);
                    report.link_changes.push(LinkChange {
                        note: note.into(),
                        line: reference.line,
                        from: original[reference.span.clone()].into(),
                        to: replacement.clone(),
                    });
                    edits.push((reference.span, replacement));
                }
            }
            _ => {
                let destination_candidates =
                    source_candidates(parent, &reference.target, reference.kind, attachment);
                let candidates = destination_candidates.into_iter().flat_map(|path| {
                    let in_import = join_portable(folder, &path);
                    if reference.kind == LinkKind::Wiki {
                        vec![in_import, path]
                    } else {
                        vec![in_import]
                    }
                });
                if found.is_empty()
                    && candidates.into_iter().any(|path| {
                        safe_join(target_root, None, &path).is_ok_and(|path| path.is_file())
                    })
                {
                    continue;
                }
                if found.is_empty() && !attachment && !reference.embed {
                    continue;
                }
                let state = if found.len() > 1 {
                    "ambiguous"
                } else if found.is_empty() {
                    "missing"
                } else {
                    "excluded"
                };
                let message = format!(
                    "{note}:{}: {state} target '{}'",
                    reference.line, reference.target
                );
                if attachment && reference.embed && !allow_missing {
                    report.blockers.push(message);
                } else {
                    report.warnings.push(message);
                }
            }
        }
    }
    let mut output = original.to_owned();
    for (span, replacement) in edits.into_iter().rev() {
        output.replace_range(span, &replacement);
    }
    output
}

fn relink(reference: &links::Link, note: &str, destination: &str) -> String {
    let bang = if reference.embed { "!" } else { "" };
    if reference.kind == LinkKind::Wiki {
        let target = if reference.target.ends_with(".md") {
            destination
        } else {
            destination.strip_suffix(".md").unwrap_or(destination)
        };
        let anchor = reference
            .heading
            .as_deref()
            .map_or(String::new(), |s| format!("#{s}"));
        let alias = reference
            .alias
            .as_deref()
            .map_or(String::new(), |s| format!("|{s}"));
        format!("{bang}[[{target}{anchor}{alias}]]")
    } else {
        let path = relative_link(note, destination)
            .split('/')
            .map(encode_uri)
            .collect::<Vec<_>>()
            .join("/");
        let anchor = reference
            .heading
            .as_deref()
            .map_or(String::new(), |anchor| format!("#{}", encode_uri(anchor)));
        let label = reference.alias.as_deref().unwrap_or("");
        format!("{bang}[{label}]({path}{anchor})")
    }
}

fn source_candidates(parent: &Path, target: &str, kind: LinkKind, asset: bool) -> Vec<String> {
    let at_root = target.starts_with('/');
    let target = target.trim_start_matches('/');
    let filename = if asset || is_markdown(target) {
        target.to_owned()
    } else {
        format!("{target}.md")
    };
    let rooted = || {
        normalize_relative(Path::new(&filename))
            .ok()
            .and_then(|p| portable_path(&p).ok())
    };
    let relative = || {
        normalize_relative(&parent.join(&filename))
            .ok()
            .and_then(|p| portable_path(&p).ok())
    };
    if at_root {
        return rooted().into_iter().collect();
    }
    if kind == LinkKind::Wiki && !target.starts_with("./") && !target.starts_with("../") {
        if target.contains('/') {
            [rooted(), relative()]
        } else {
            [relative(), rooted()]
        }
        .into_iter()
        .flatten()
        .collect()
    } else {
        relative().into_iter().collect()
    }
}

fn resolve_source(
    inventory: &BTreeSet<String>,
    parent: &Path,
    target: &str,
    kind: LinkKind,
    asset: bool,
) -> Vec<String> {
    let candidates = source_candidates(parent, target, kind, asset);
    if let Some(path) = candidates.iter().find(|path| inventory.contains(*path)) {
        return vec![path.clone()];
    }
    if kind == LinkKind::Wiki && !target.contains('/') {
        let name = if asset || is_markdown(target) {
            target.to_owned()
        } else {
            format!("{target}.md")
        };
        return inventory
            .iter()
            .filter(|path| path.rsplit('/').next() == Some(name.as_str()))
            .cloned()
            .collect();
    }
    Vec::new()
}

fn join_portable(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.into()
    } else {
        format!("{parent}/{child}")
    }
}

fn excluded(path: &str, exclusions: &BTreeSet<String>) -> bool {
    exclusions
        .iter()
        .any(|exclude| Path::new(path).starts_with(exclude))
}

fn is_markdown(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

fn source_inventory(root: &Path) -> Result<BTreeSet<String>, VaultError> {
    let mut inventory = BTreeSet::new();
    let mut stack = vec![root.to_owned()];
    while let Some(folder) = stack.pop() {
        for entry in fs::read_dir(&folder).map_err(|e| file_error(&folder, e))? {
            let entry = entry.map_err(|e| file_error(&folder, e))?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| file_error(&path, e))?;
            if kind.is_dir()
                && !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | ".obsidian" | ".trash")
                )
            {
                stack.push(path);
            } else if kind.is_file() {
                inventory.insert(portable_path(
                    path.strip_prefix(root)
                        .map_err(|_| invalid("Source path escaped its folder"))?,
                )?);
            }
        }
    }
    Ok(inventory)
}

fn source_file(root: &Path, relative: &str) -> Result<PathBuf, VaultError> {
    let path = safe_join(root, None, relative)?;
    let resolved = fs::canonicalize(&path).map_err(|e| file_error(&path, e))?;
    if resolved != path || !resolved.is_file() {
        return Err(invalid(format!(
            "Source is not a regular file inside the selected folder: {relative}"
        )));
    }
    Ok(resolved)
}

fn portable_path(path: &Path) -> Result<String, VaultError> {
    path.components()
        .map(|part| match part {
            Component::Normal(name) => name
                .to_str()
                .filter(|name| !name.contains(['\\', ':']))
                .map(str::to_owned)
                .ok_or_else(|| invalid(format!("Unsupported file name: {}", path.display()))),
            _ => Err(invalid(format!(
                "Invalid relative path: {}",
                path.display()
            ))),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("/"))
}

fn invalid(message: impl Into<String>) -> VaultError {
    VaultError::InvalidPath(message.into())
}

fn file_error(path: &Path, error: io::Error) -> VaultError {
    VaultError::io(path.display().to_string(), error)
}
