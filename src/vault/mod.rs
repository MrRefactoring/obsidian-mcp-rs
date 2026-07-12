mod frontmatter;
mod info;
mod links;
mod patch;
mod path;
mod search;
mod tags;
mod walk;
mod write;

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use rayon::prelude::*;

use crate::error::VaultError;

use frontmatter::content_has_tag;
use path::{ensure_md_extension, safe_join};
use tags::{
    add_tags_to_content, add_tags_to_frontmatter, normalize_tag, remove_tags_from_note,
    rename_tag_in_note,
};
use walk::md_files;
use write::atomic_write;

pub use info::{DEFAULT_RECENT, InfoOutput, InfoQuery, RecentNote, Stats, TagCount};
pub use links::{LinkKind, LinkRef};
pub use patch::TargetKind;
pub use search::{
    DEFAULT_LIMIT, DEFAULT_MAX_MATCHES_PER_FILE, SearchLimits, SearchOutput, SearchResult,
    SearchType, Snippet,
};

/// What an edit does to the note — or, with a `Target`, to that part of it.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EditOperation {
    /// Add to the end.
    Append,
    /// Add to the start.
    Prepend,
    /// Overwrite entirely.
    Replace,
    /// Replace the first occurrence of `search`.
    FindAndReplace,
}

/// The part of a note an edit is confined to.
pub struct Target<'a> {
    pub kind: &'a TargetKind,
    /// A heading (with or without its `#`) or a block id (with or without `^`).
    pub name: &'a str,
}

/// One edit to one note. Without a `target` the operation applies to the whole
/// note, which is what `edit-note` did before patching existed.
pub struct Edit<'a> {
    pub operation: EditOperation,
    pub content: &'a str,
    /// The needle for `find_and_replace`.
    pub search: Option<&'a str>,
    pub target: Option<Target<'a>>,
}

/// How much of a note to return.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NoteView {
    /// The note's full text.
    #[default]
    Content,
    /// Only what an edit can be aimed at — headings, block references and
    /// frontmatter keys. Cheap to read, and it saves the model from guessing a
    /// target and missing.
    Outline,
}

/// What the `frontmatter` tool should do with a note's YAML frontmatter.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FrontmatterAction {
    /// Read the frontmatter — all of it, or one `key`.
    Get,
    /// Write `value` to `key`, leaving every other line of the note alone.
    Set,
    /// Delete `key`.
    Remove,
}

/// A note's frontmatter, after the requested action.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct FrontmatterOutput {
    /// Vault-relative path of the note.
    pub path: String,
    /// The frontmatter as it now stands — or, for a `get` naming a key, that
    /// key's value alone (`null` when the note doesn't carry it).
    pub frontmatter: serde_json::Value,
    /// Whether the note was rewritten. `false` for `get`, and for a write that
    /// changed nothing.
    pub changed: bool,
}

/// Where a deleted note goes. Hidden, so `md_files` — and therefore search, the
/// link graph and `rename-tag` — never sees it again.
const TRASH: &str = ".trash";

/// What `delete-note` did.
#[derive(Debug, Clone)]
pub struct DeleteOutcome {
    /// Where the note now sits, vault-relative — or `None` when it was erased.
    pub trashed_to: Option<String>,
}

/// What `move-note` did. `relinked` names the notes whose links were updated to
/// follow the moved note — empty when nothing pointed at it, or when the links
/// still resolve on their own (a bare `[[Note]]` survives a folder move).
#[derive(Debug, Clone)]
pub struct MoveOutcome {
    pub path: PathBuf,
    pub relinked: Vec<String>,
}

/// Which slice of the link graph the `wikilinks` tool should return.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LinkQuery {
    /// Notes that link *to* the given note.
    Backlinks,
    /// Links *from* the given note.
    Outgoing,
    /// Links whose target does not exist.
    Broken,
    /// Notes nothing links to.
    Orphans,
}

/// Answer to a `wikilinks` query.
#[derive(Debug, Clone, serde::Serialize, schemars::JsonSchema)]
pub struct LinkOutput {
    /// Populated for `backlinks`, `outgoing` and `broken`.
    pub links: Vec<LinkRef>,
    /// Populated for `orphans` — vault-relative paths.
    pub notes: Vec<String>,
    pub total: usize,
}

#[derive(Debug)]
pub struct VaultManager {
    vaults: HashMap<String, PathBuf>,
    /// Serialises every mutation. See `write_guard`.
    write_lock: Mutex<()>,
}

impl VaultManager {
    pub fn new(vault_paths: Vec<PathBuf>) -> Self {
        let mut vaults = HashMap::new();
        for path in vault_paths {
            let base = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("vault")
                .to_string();
            // Disambiguate when the basename collides — `~/work/notes` and
            // `~/personal/notes` would otherwise shadow each other silently.
            let name = if vaults.contains_key(&base) {
                let mut n = 2;
                loop {
                    let candidate = format!("{base}-{n}");
                    if !vaults.contains_key(&candidate) {
                        tracing::warn!(
                            vault = %candidate,
                            original = %base,
                            path = %path.display(),
                            "vault basename collision — registered under disambiguated name"
                        );
                        break candidate;
                    }
                    n += 1;
                }
            } else {
                base
            };
            vaults.insert(name, path);
        }
        Self {
            vaults,
            write_lock: Mutex::new(()),
        }
    }

    /// Hold this for the whole of any mutation.
    ///
    /// Every write tool is a read-modify-write: read the note, edit the text,
    /// write it back. `atomic_write` makes the *write* atomic, but not the pair —
    /// and the MCP server answers requests concurrently, so two calls against one
    /// note would both read the old text and the second write would silently
    /// discard the first one's edit. Reads deliberately don't take this lock:
    /// `atomic_write` renames into place, so a reader sees the old note or the
    /// new one, never a torn one.
    ///
    /// One lock for all writes, rather than one per note: vault mutations are
    /// short, tool calls are not a hot loop, and a single lock cannot deadlock.
    fn write_guard(&self) -> std::sync::MutexGuard<'_, ()> {
        // A poisoned lock means some earlier call panicked mid-edit. Refusing
        // every write from then on is worse than carrying on.
        self.write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn list_vaults(&self) -> Vec<(String, PathBuf)> {
        let mut list: Vec<(String, PathBuf)> = self
            .vaults
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        list.sort_by(|a, b| a.0.cmp(&b.0));
        list
    }

    pub fn resolve_vault(&self, name: &str) -> Result<&Path, VaultError> {
        self.vaults.get(name).map(|p| p.as_path()).ok_or_else(|| {
            let available = self.vaults.keys().cloned().collect::<Vec<_>>().join(", ");
            VaultError::VaultNotFound(name.to_string(), available)
        })
    }

    pub fn note_path(
        &self,
        vault: &str,
        filename: &str,
        folder: Option<&str>,
    ) -> Result<PathBuf, VaultError> {
        let root = self.resolve_vault(vault)?;
        let filename = ensure_md_extension(filename);
        safe_join(root, folder, &filename)
    }

    pub fn read_note(
        &self,
        vault: &str,
        filename: &str,
        folder: Option<&str>,
        view: &NoteView,
    ) -> Result<String, VaultError> {
        let content = self.note_content(vault, filename, folder)?;
        Ok(match view {
            NoteView::Content => content,
            NoteView::Outline => patch::outline(&content),
        })
    }

    /// A note's text, or `NoteNotFound`. The read every note-scoped tool starts
    /// from.
    fn note_content(
        &self,
        vault: &str,
        filename: &str,
        folder: Option<&str>,
    ) -> Result<String, VaultError> {
        let path = self.note_path(vault, filename, folder)?;
        if !path.exists() {
            return Err(VaultError::NoteNotFound(
                path.display().to_string(),
                vault.to_string(),
            ));
        }
        fs::read_to_string(&path).map_err(|e| VaultError::io(path.display().to_string(), e))
    }

    pub fn create_note(
        &self,
        vault: &str,
        filename: &str,
        content: &str,
        folder: Option<&str>,
    ) -> Result<PathBuf, VaultError> {
        let _guard = self.write_guard();
        let path = self.note_path(vault, filename, folder)?;
        if path.exists() {
            return Err(VaultError::NoteAlreadyExists(
                path.display().to_string(),
                vault.to_string(),
            ));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| VaultError::io(parent.display().to_string(), e))?;
        }
        atomic_write(&path, content.as_bytes())?;
        Ok(path)
    }

    /// Apply `edit` to a note, returning its text before and after so the caller
    /// can show a diff.
    ///
    /// With `edit.target` set, only the bytes that target covers are rewritten —
    /// the rest of the note is passed through untouched. That is the point: a
    /// whole-note `replace` silently loses anything the model failed to
    /// reproduce, and on a long note it usually fails to reproduce something.
    pub fn edit_note(
        &self,
        vault: &str,
        filename: &str,
        folder: Option<&str>,
        edit: &Edit<'_>,
    ) -> Result<(String, String), VaultError> {
        let _guard = self.write_guard();
        let path = self.note_path(vault, filename, folder)?;
        let old = self.note_content(vault, filename, folder)?;

        let needle = || {
            edit.search.ok_or_else(|| {
                VaultError::InvalidPath(
                    "find_and_replace requires a 'search' parameter".to_string(),
                )
            })
        };

        let new = match &edit.target {
            None => match edit.operation {
                EditOperation::Append => format!("{}\n{}", old.trim_end(), edit.content),
                EditOperation::Prepend => format!("{}\n{}", edit.content, old.trim_start()),
                EditOperation::Replace => edit.content.to_string(),
                EditOperation::FindAndReplace => {
                    let needle = needle()?;
                    if !old.contains(needle) {
                        return Err(VaultError::SearchTextNotFound(filename.to_string()));
                    }
                    old.replacen(needle, edit.content, 1)
                }
            },
            Some(target) => {
                let region =
                    patch::find_region(&old, target.kind, target.name).ok_or_else(|| {
                        VaultError::TargetNotFound(target.name.to_string(), filename.to_string())
                    })?;
                match edit.operation {
                    EditOperation::Append => patch::append(&old, &region, edit.content),
                    EditOperation::Prepend => patch::prepend(&old, &region, edit.content),
                    EditOperation::Replace => patch::replace(&old, &region, edit.content),
                    EditOperation::FindAndReplace => {
                        patch::find_and_replace(&old, &region, needle()?, edit.content)
                            .ok_or_else(|| VaultError::SearchTextNotFound(filename.to_string()))?
                    }
                }
            }
        };

        atomic_write(&path, new.as_bytes())?;
        Ok((old, new))
    }

    /// Read or rewrite a note's YAML frontmatter.
    ///
    /// A write is line surgery on the one key named — every other line, comment
    /// and key ordering survives byte-for-byte. A YAML round-trip would be far
    /// less code and would reformat the user's whole block.
    pub fn frontmatter(
        &self,
        vault: &str,
        filename: &str,
        folder: Option<&str>,
        action: &FrontmatterAction,
        key: Option<&str>,
        value: Option<&serde_json::Value>,
    ) -> Result<FrontmatterOutput, VaultError> {
        let _guard = self.write_guard();
        let root = self.resolve_vault(vault)?.to_path_buf();
        let path = self.note_path(vault, filename, folder)?;
        let content = self.note_content(vault, filename, folder)?;
        let rel = rel_path(&root, &path);

        let key = || {
            key.filter(|k| !k.is_empty()).ok_or_else(|| {
                VaultError::InvalidPath(format!("the '{:?}' action needs a 'key'", action))
            })
        };
        let invalid = |e: String| VaultError::InvalidFrontmatter(rel.clone(), e);

        let updated = match action {
            FrontmatterAction::Get => None,
            FrontmatterAction::Set => {
                let value = value.ok_or_else(|| {
                    VaultError::InvalidPath("the 'set' action needs a 'value'".to_string())
                })?;
                Some(frontmatter::set_field(&content, key()?, value).map_err(invalid)?)
            }
            FrontmatterAction::Remove => Some(frontmatter::remove_field(&content, key()?)),
        };

        // Setting a key to what it already says isn't a write.
        let changed = updated.as_deref().is_some_and(|new| new != content);
        if changed {
            atomic_write(&path, updated.as_deref().unwrap_or_default().as_bytes())?;
        }
        let current = if changed {
            updated.unwrap_or_default()
        } else {
            content
        };

        let fields = frontmatter::parse_fields(&current).map_err(invalid)?;
        let frontmatter = match action {
            // `get` naming a key answers with that key, not the whole block.
            FrontmatterAction::Get if key().is_ok() => fields
                .get(key()?)
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            _ => serde_json::Value::Object(fields),
        };

        Ok(FrontmatterOutput {
            path: rel,
            frontmatter,
            changed,
        })
    }

    /// Delete a note — by default to the vault's `.trash/`, so the user can get
    /// it back.
    ///
    /// An agent deleting the wrong note is a plausible mistake and an
    /// unrecoverable one, so the default is the recoverable path. This is also
    /// what Obsidian itself does. `.trash` is hidden, and `md_files` skips hidden
    /// directories, so a trashed note disappears from search and the link graph
    /// exactly as if it were gone (test: `hidden_directories_are_not_walked`).
    pub fn delete_note(
        &self,
        vault: &str,
        filename: &str,
        folder: Option<&str>,
        permanent: bool,
    ) -> Result<DeleteOutcome, VaultError> {
        let _guard = self.write_guard();
        let root = self.resolve_vault(vault)?.to_path_buf();
        let path = self.note_path(vault, filename, folder)?;
        if !path.exists() {
            return Err(VaultError::NoteNotFound(
                path.display().to_string(),
                vault.to_string(),
            ));
        }

        // Emptying the trash is the one delete that must actually erase.
        let already_trashed = path.strip_prefix(&root).is_ok_and(|r| r.starts_with(TRASH));
        if permanent || already_trashed {
            fs::remove_file(&path).map_err(|e| VaultError::io(path.display().to_string(), e))?;
            prune_empty_parent(&path, &root);
            return Ok(DeleteOutcome { trashed_to: None });
        }

        // Mirror the note's folder inside the trash, so `a/note.md` and
        // `b/note.md` don't land on top of each other.
        let rel = path.strip_prefix(&root).unwrap_or(&path).to_path_buf();
        let dest = free_path(&root.join(TRASH).join(&rel));
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| VaultError::io(parent.display().to_string(), e))?;
        }
        fs::rename(&path, &dest).map_err(|e| VaultError::io(path.display().to_string(), e))?;
        prune_empty_parent(&path, &root);

        Ok(DeleteOutcome {
            trashed_to: Some(rel_path(&root, &dest)),
        })
    }

    /// Move or rename a note, updating every `[[wikilink]]` and markdown link
    /// that pointed at it.
    ///
    /// Returns the new path and the notes whose links were rewritten. The move
    /// itself is a single `fs::rename`, and each link rewrite is an
    /// `atomic_write` — but the operation *as a whole* is not atomic. A journal
    /// would be needed for that, which is a large bet for a local single-user
    /// tool; instead a failed rewrite is reported rather than swallowed.
    pub fn move_note(
        &self,
        vault: &str,
        filename: &str,
        folder: Option<&str>,
        new_folder: Option<&str>,
        new_filename: Option<&str>,
    ) -> Result<MoveOutcome, VaultError> {
        let _guard = self.write_guard();
        let root = self.resolve_vault(vault)?.to_path_buf();
        let src = self.note_path(vault, filename, folder)?;
        if !src.exists() {
            return Err(VaultError::NoteNotFound(
                src.display().to_string(),
                vault.to_string(),
            ));
        }
        let dest_filename = new_filename.unwrap_or(filename);
        let dest = self.note_path(vault, dest_filename, new_folder)?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| VaultError::io(parent.display().to_string(), e))?;
        }

        // Resolve links against the vault as it stands *before* the move —
        // afterwards `src` no longer exists and nothing would resolve to it.
        let files = md_files(&root);
        let resolver = links::Resolver::new(&root, &files);

        fs::rename(&src, &dest).map_err(|e| VaultError::io(src.display().to_string(), e))?;
        prune_empty_parent(&src, &root);

        let dest_rel = dest
            .strip_prefix(&root)
            .unwrap_or(&dest)
            .to_string_lossy()
            .replace('\\', "/");

        // The moved note can link to itself; it is already at its new path, so
        // rewrite it there rather than at the path it just left.
        let mut relinked: Vec<String> = files
            .par_iter()
            .filter_map(|path| {
                let read_at = if path == &src { &dest } else { path };
                let content = fs::read_to_string(read_at).ok()?;
                let updated =
                    links::rewrite_links(&content, path, &src, &dest_rel, &resolver)?;
                match atomic_write(read_at, updated.as_bytes()) {
                    Ok(()) => Some(
                        read_at
                            .strip_prefix(&root)
                            .unwrap_or(read_at)
                            .to_string_lossy()
                            .replace('\\', "/"),
                    ),
                    Err(e) => {
                        tracing::warn!(note = %read_at.display(), error = %e, "failed to update links");
                        None
                    }
                }
            })
            .collect();
        relinked.sort();

        Ok(MoveOutcome {
            path: dest,
            relinked,
        })
    }

    pub fn create_directory(
        &self,
        vault: &str,
        path: &str,
        recursive: bool,
    ) -> Result<PathBuf, VaultError> {
        let _guard = self.write_guard();
        let root = self.resolve_vault(vault)?;
        let dir = safe_join(root, None, path)?;
        if dir.exists() {
            return Err(VaultError::DirectoryAlreadyExists(
                dir.display().to_string(),
            ));
        }
        if recursive {
            fs::create_dir_all(&dir).map_err(|e| VaultError::io(dir.display().to_string(), e))?;
        } else {
            fs::create_dir(&dir).map_err(|e| VaultError::io(dir.display().to_string(), e))?;
        }
        Ok(dir)
    }

    pub fn search_vault(
        &self,
        vault: &str,
        query: &str,
        search_path: Option<&str>,
        case_sensitive: bool,
        search_type: &SearchType,
        limits: &SearchLimits,
    ) -> Result<SearchOutput, VaultError> {
        let root = self.resolve_vault(vault)?;
        let search_root = match search_path {
            Some(p) if !p.is_empty() => safe_join(root, None, p)?,
            _ => root.to_path_buf(),
        };
        Ok(search::search(
            root,
            &search_root,
            query,
            case_sensitive,
            search_type,
            limits,
        ))
    }

    /// Query the vault's link graph. One parallel pass builds the whole graph,
    /// so no index is kept and nothing can go stale.
    pub fn wikilinks(
        &self,
        vault: &str,
        query: &LinkQuery,
        filename: Option<&str>,
        folder: Option<&str>,
    ) -> Result<LinkOutput, VaultError> {
        let root = self.resolve_vault(vault)?.to_path_buf();
        let (files, _, refs) = links::link_graph(&root);

        // `backlinks` and `outgoing` are about one note, so they need one.
        let note = match query {
            LinkQuery::Backlinks | LinkQuery::Outgoing => {
                let filename = filename.ok_or_else(|| {
                    VaultError::InvalidPath(format!(
                        "the '{}' query needs a 'filename'",
                        match query {
                            LinkQuery::Backlinks => "backlinks",
                            _ => "outgoing",
                        }
                    ))
                })?;
                let path = self.note_path(vault, filename, folder)?;
                if !path.exists() {
                    return Err(VaultError::NoteNotFound(
                        path.display().to_string(),
                        vault.to_string(),
                    ));
                }
                Some(
                    path.strip_prefix(&root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/"),
                )
            }
            _ => None,
        };

        let (links, notes) = match query {
            LinkQuery::Backlinks => {
                let note = note.expect("checked above");
                let mut hits: Vec<LinkRef> = refs
                    .into_iter()
                    .filter(|r| r.resolved.as_deref() == Some(note.as_str()))
                    .collect();
                hits.sort_by(|a, b| (&a.from, a.line).cmp(&(&b.from, b.line)));
                (hits, Vec::new())
            }
            LinkQuery::Outgoing => {
                let note = note.expect("checked above");
                let mut hits: Vec<LinkRef> = refs.into_iter().filter(|r| r.from == note).collect();
                hits.sort_by_key(|r| r.line);
                (hits, Vec::new())
            }
            LinkQuery::Broken => {
                let mut hits: Vec<LinkRef> =
                    refs.into_iter().filter(|r| r.resolved.is_none()).collect();
                hits.sort_by(|a, b| (&a.from, a.line).cmp(&(&b.from, b.line)));
                (hits, Vec::new())
            }
            LinkQuery::Orphans => {
                let linked: std::collections::HashSet<&str> =
                    refs.iter().filter_map(|r| r.resolved.as_deref()).collect();
                let mut notes: Vec<String> = files
                    .iter()
                    .map(|p| {
                        p.strip_prefix(&root)
                            .unwrap_or(p)
                            .to_string_lossy()
                            .replace('\\', "/")
                    })
                    .filter(|rel| !linked.contains(rel.as_str()))
                    .collect();
                notes.sort();
                (Vec::new(), notes)
            }
        };

        let total = links.len() + notes.len();
        Ok(LinkOutput {
            links,
            notes,
            total,
        })
    }

    /// Describe the vault: its tags, its recently touched notes, or its size.
    ///
    /// A read-only orientation step — a model that has just been pointed at a
    /// vault can't search it usefully until it knows what's in there.
    pub fn vault_info(
        &self,
        vault: &str,
        query: &InfoQuery,
        limit: usize,
    ) -> Result<InfoOutput, VaultError> {
        let root = self.resolve_vault(vault)?;
        Ok(info::info(root, query, limit))
    }

    pub fn add_tags(
        &self,
        vault: &str,
        files: &[String],
        tags: &[String],
        location: &str,
        normalize: bool,
        position: &str,
    ) -> Result<Vec<String>, VaultError> {
        let _guard = self.write_guard();
        let root = self.resolve_vault(vault)?;
        let mut modified = Vec::new();

        for file in files {
            let path = safe_join(root, None, file)?;
            if !path.exists() {
                continue;
            }

            let content = fs::read_to_string(&path)
                .map_err(|e| VaultError::io(path.display().to_string(), e))?;

            let processed_tags: Vec<String> = tags
                .iter()
                .map(|t| {
                    if normalize {
                        normalize_tag(t)
                    } else {
                        t.clone()
                    }
                })
                .collect();

            let new_content = match location {
                "content" => add_tags_to_content(&content, &processed_tags, position),
                "frontmatter" => add_tags_to_frontmatter(&content, &processed_tags),
                _ => {
                    let with_front = add_tags_to_frontmatter(&content, &processed_tags);
                    add_tags_to_content(&with_front, &processed_tags, position)
                }
            };

            atomic_write(&path, new_content.as_bytes())?;
            modified.push(file.clone());
        }

        Ok(modified)
    }

    pub fn remove_tags(
        &self,
        vault: &str,
        files: &[String],
        tags: &[String],
    ) -> Result<Vec<String>, VaultError> {
        let _guard = self.write_guard();
        let root = self.resolve_vault(vault)?;
        let mut modified = Vec::new();

        for file in files {
            let path = safe_join(root, None, file)?;
            if !path.exists() {
                continue;
            }

            let content = fs::read_to_string(&path)
                .map_err(|e| VaultError::io(path.display().to_string(), e))?;

            let new_content = remove_tags_from_note(&content, tags);
            atomic_write(&path, new_content.as_bytes())?;
            modified.push(file.clone());
        }

        Ok(modified)
    }

    pub fn rename_tag(
        &self,
        vault: &str,
        old_tag: &str,
        new_tag: &str,
    ) -> Result<Vec<String>, VaultError> {
        let _guard = self.write_guard();
        let root = self.resolve_vault(vault)?;

        let mut modified: Vec<String> = md_files(root)
            .par_iter()
            .map(|path| -> Result<Option<String>, VaultError> {
                let content = fs::read_to_string(path)
                    .map_err(|e| VaultError::io(path.display().to_string(), e))?;
                if !content_has_tag(&content, old_tag) {
                    return Ok(None);
                }
                let new_content = rename_tag_in_note(&content, old_tag, new_tag);
                atomic_write(path, new_content.as_bytes())?;
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(path)
                    .display()
                    .to_string();
                Ok(Some(rel))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        modified.sort();

        Ok(modified)
    }
}

/// A free path at or beside `wanted`: `note.md`, then `note-2.md`, and so on.
/// Two notes with the same name deleted from different folders must both survive
/// in the trash.
fn free_path(wanted: &Path) -> PathBuf {
    if !wanted.exists() {
        return wanted.to_path_buf();
    }
    let stem = wanted
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = wanted
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();

    (2..)
        .map(|n| wanted.with_file_name(format!("{}-{}{}", stem, n, ext)))
        .find(|candidate| !candidate.exists())
        .expect("the integers run out long after the filesystem does")
}

/// A note's path as the vault refers to it: relative to the root, `/`-separated
/// on every platform.
fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Remove `note`'s parent directory if the just-completed operation left it
/// empty — but never the vault `root`. Best-effort: a failed cleanup is logged,
/// not propagated, so it can't fail the move/delete that triggered it.
fn prune_empty_parent(note: &Path, root: &Path) {
    if let Some(parent) = note.parent()
        && parent != root
        && fs::read_dir(parent).is_ok_and(|mut d| d.next().is_none())
        && let Err(e) = fs::remove_dir(parent)
    {
        tracing::warn!(dir = %parent.display(), error = %e, "failed to remove emptied source folder");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // An absolute path outside any vault, for the current OS. A `/etc/…` string
    // is absolute on Unix but NOT on Windows (no drive prefix), where it would be
    // rejected by the escape check instead of the absolute-path check — so we use
    // a genuine per-OS absolute path to exercise the `is_absolute()` branch.
    #[cfg(unix)]
    const ABS_FILE: &str = "/etc/passwd";
    #[cfg(not(unix))]
    const ABS_FILE: &str = r"C:\Windows\System32\drivers\etc\hosts";
    #[cfg(unix)]
    const ABS_FOLDER: &str = "/tmp";
    #[cfg(not(unix))]
    const ABS_FOLDER: &str = r"C:\Windows";

    fn make_vault() -> (TempDir, VaultManager) {
        let dir = TempDir::new().unwrap();
        let manager = VaultManager::new(vec![dir.path().to_path_buf()]);
        (dir, manager)
    }

    fn vault_name(dir: &TempDir) -> String {
        dir.path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    fn write_note(dir: &TempDir, filename: &str, content: &str) {
        fs::write(dir.path().join(filename), content).unwrap();
    }

    /// A whole-note edit — the shape every pre-patch caller used.
    fn edit<'a>(operation: EditOperation, content: &'a str, search: Option<&'a str>) -> Edit<'a> {
        Edit {
            operation,
            content,
            search,
            target: None,
        }
    }

    /// An edit aimed at one heading or block.
    fn patch<'a>(
        operation: EditOperation,
        content: &'a str,
        kind: &'a TargetKind,
        name: &'a str,
    ) -> Edit<'a> {
        Edit {
            operation,
            content,
            search: None,
            target: Some(Target { kind, name }),
        }
    }

    #[test]
    fn append_adds_content_to_end() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "hello");

        let (old, new) = vault
            .edit_note(
                &name,
                "note",
                None,
                &edit(EditOperation::Append, " world", None),
            )
            .unwrap();

        assert_eq!(old, "hello");
        assert_eq!(new, "hello\n world");
    }

    #[test]
    fn prepend_adds_content_to_start() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "world");

        let (old, new) = vault
            .edit_note(
                &name,
                "note",
                None,
                &edit(EditOperation::Prepend, "hello\n", None),
            )
            .unwrap();

        assert_eq!(old, "world");
        assert_eq!(new, "hello\n\nworld");
    }

    #[test]
    fn replace_overwrites_entire_content() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "old content");

        let (old, new) = vault
            .edit_note(
                &name,
                "note",
                None,
                &edit(EditOperation::Replace, "new content", None),
            )
            .unwrap();

        assert_eq!(old, "old content");
        assert_eq!(new, "new content");
        assert_eq!(
            fs::read_to_string(dir.path().join("note.md")).unwrap(),
            "new content"
        );
    }

    #[test]
    fn find_and_replace_substitutes_first_occurrence() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "foo bar foo");

        let (old, new) = vault
            .edit_note(
                &name,
                "note",
                None,
                &edit(EditOperation::FindAndReplace, "baz", Some("foo")),
            )
            .unwrap();

        assert_eq!(old, "foo bar foo");
        assert_eq!(new, "baz bar foo");
        assert_eq!(
            fs::read_to_string(dir.path().join("note.md")).unwrap(),
            "baz bar foo"
        );
    }

    #[test]
    fn find_and_replace_returns_error_when_search_text_not_found() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "hello world");

        let result = vault.edit_note(
            &name,
            "note",
            None,
            &edit(
                EditOperation::FindAndReplace,
                "replacement",
                Some("missing"),
            ),
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Search text not found")
        );
    }

    #[test]
    fn find_and_replace_returns_error_when_search_param_missing() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "hello");

        let result = vault.edit_note(
            &name,
            "note",
            None,
            &edit(EditOperation::FindAndReplace, "replacement", None),
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("search"));
    }

    // An unknown operation can no longer reach the domain: `EditOperation` is a
    // typed enum, so serde rejects it as INVALID_PARAMS at the tool boundary.
    // That rejection is covered in `tools::edit_note`.

    #[test]
    fn edit_returns_error_for_missing_note() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);

        let result = vault.edit_note(
            &name,
            "nonexistent",
            None,
            &edit(EditOperation::Append, "data", None),
        );

        assert!(result.is_err());
    }

    #[test]
    fn append_persists_to_disk() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "line1");

        vault
            .edit_note(
                &name,
                "note",
                None,
                &edit(EditOperation::Append, "line2", None),
            )
            .unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("note.md")).unwrap(),
            "line1\nline2"
        );
    }

    // ── Concurrent writes ─────────────────────────────────────────────────────

    #[test]
    fn concurrent_edits_to_one_note_do_not_lose_updates() {
        // The MCP server answers requests concurrently. Every write tool is a
        // read-modify-write, so without serialisation these threads all read the
        // same original note and all but the last write is silently discarded.
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "start\n");

        std::thread::scope(|scope| {
            for i in 0..8 {
                let (vault, name) = (&vault, &name);
                scope.spawn(move || {
                    let line = format!("entry {}", i);
                    vault
                        .edit_note(
                            name,
                            "note",
                            None,
                            &edit(EditOperation::Append, &line, None),
                        )
                        .unwrap();
                });
            }
        });

        let out = fs::read_to_string(dir.path().join("note.md")).unwrap();
        for i in 0..8 {
            assert!(
                out.contains(&format!("entry {}", i)),
                "edit {i} was lost:\n{out}"
            );
        }
    }

    #[test]
    fn concurrent_tag_and_frontmatter_writes_do_not_lose_each_other() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntitle: T\n---\nbody\n");

        std::thread::scope(|scope| {
            let (v, n) = (&vault, &name);
            scope.spawn(move || {
                v.add_tags(
                    n,
                    &["note.md".into()],
                    &["work".into()],
                    "frontmatter",
                    false,
                    "end",
                )
                .unwrap();
            });
            let (v, n) = (&vault, &name);
            scope.spawn(move || {
                v.frontmatter(
                    n,
                    "note",
                    None,
                    &FrontmatterAction::Set,
                    Some("status"),
                    Some(&serde_json::json!("draft")),
                )
                .unwrap();
            });
        });

        let out = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(out.contains("work"), "the tag write was lost:\n{out}");
        assert!(
            out.contains("status: draft"),
            "the frontmatter write was lost:\n{out}"
        );
        assert!(out.contains("title: T"));
    }

    // ── edit_note: patch targets ──────────────────────────────────────────────

    const SECTIONED: &str =
        "---\ntitle: T\n---\n# Top\n\nintro\n\n## Log\n\nfirst\n\n## Notes\n\nkeep me ^n1\n";

    #[test]
    fn patching_a_section_leaves_the_rest_of_the_note_byte_for_byte() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", SECTIONED);

        vault
            .edit_note(
                &name,
                "note",
                None,
                &patch(
                    EditOperation::Append,
                    "second",
                    &TargetKind::Heading,
                    "## Log",
                ),
            )
            .unwrap();

        let out = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert_eq!(
            out,
            "---\ntitle: T\n---\n# Top\n\nintro\n\n## Log\n\nfirst\nsecond\n\n## Notes\n\nkeep me ^n1\n"
        );
    }

    #[test]
    fn replacing_a_section_keeps_its_heading_and_its_siblings() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", SECTIONED);

        vault
            .edit_note(
                &name,
                "note",
                None,
                &patch(
                    EditOperation::Replace,
                    "rewritten",
                    &TargetKind::Heading,
                    "Log",
                ),
            )
            .unwrap();

        let out = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(out.contains("## Log\nrewritten\n\n## Notes"), "{out}");
        assert!(
            out.contains("intro"),
            "the other sections must survive: {out}"
        );
        assert!(out.contains("keep me ^n1"));
    }

    #[test]
    fn patching_a_block_touches_only_that_block() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", SECTIONED);

        vault
            .edit_note(
                &name,
                "note",
                None,
                &patch(
                    EditOperation::Replace,
                    "swapped ^n1",
                    &TargetKind::Block,
                    "n1",
                ),
            )
            .unwrap();

        let out = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(out.contains("swapped ^n1"), "{out}");
        assert!(!out.contains("keep me"), "{out}");
        assert!(
            out.contains("## Log\n\nfirst"),
            "other sections untouched: {out}"
        );
    }

    #[test]
    fn a_missing_target_is_an_error_not_a_whole_note_overwrite() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", SECTIONED);

        let err = vault
            .edit_note(
                &name,
                "note",
                None,
                &patch(EditOperation::Replace, "x", &TargetKind::Heading, "Ghost"),
            )
            .unwrap_err();

        assert!(err.to_string().contains("Ghost"));
        assert_eq!(
            fs::read_to_string(dir.path().join("note.md")).unwrap(),
            SECTIONED,
            "a missed target must not have rewritten the note"
        );
    }

    // ── read_note: outline ────────────────────────────────────────────────────

    #[test]
    fn outline_lists_the_targets_an_edit_can_aim_at() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", SECTIONED);

        let out = vault
            .read_note(&name, "note", None, &NoteView::Outline)
            .unwrap();

        assert!(out.contains("frontmatter keys: title"), "{out}");
        assert!(out.contains("# Top"), "{out}");
        assert!(out.contains("## Log"), "{out}");
        assert!(out.contains("^n1"), "{out}");
        assert!(
            !out.contains("intro"),
            "an outline is not the note body: {out}"
        );
    }

    // ── frontmatter ───────────────────────────────────────────────────────────

    #[test]
    fn frontmatter_get_reads_every_key() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntitle: T\ncount: 3\n---\nbody\n");

        let out = vault
            .frontmatter(&name, "note", None, &FrontmatterAction::Get, None, None)
            .unwrap();

        assert_eq!(out.frontmatter["title"], serde_json::json!("T"));
        assert_eq!(out.frontmatter["count"], serde_json::json!(3));
        assert!(!out.changed);
    }

    #[test]
    fn frontmatter_get_with_a_key_answers_with_just_that_key() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntitle: T\ncount: 3\n---\nbody\n");

        let out = vault
            .frontmatter(
                &name,
                "note",
                None,
                &FrontmatterAction::Get,
                Some("title"),
                None,
            )
            .unwrap();

        assert_eq!(out.frontmatter, serde_json::json!("T"));
    }

    #[test]
    fn frontmatter_set_writes_one_key_and_preserves_the_rest() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(
            &dir,
            "note.md",
            "---\n# a comment\ntitle: T\ntags:\n  - a\n---\nbody\n",
        );

        let out = vault
            .frontmatter(
                &name,
                "note",
                None,
                &FrontmatterAction::Set,
                Some("status"),
                Some(&serde_json::json!("draft")),
            )
            .unwrap();

        assert!(out.changed);
        assert_eq!(
            fs::read_to_string(dir.path().join("note.md")).unwrap(),
            "---\n# a comment\ntitle: T\ntags:\n  - a\nstatus: draft\n---\nbody\n",
            "the comment, the key order and the tag list must all survive"
        );
    }

    #[test]
    fn frontmatter_set_to_the_same_value_is_not_a_write() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntitle: T\n---\nbody\n");

        let out = vault
            .frontmatter(
                &name,
                "note",
                None,
                &FrontmatterAction::Set,
                Some("title"),
                Some(&serde_json::json!("T")),
            )
            .unwrap();

        assert!(!out.changed);
    }

    #[test]
    fn frontmatter_remove_drops_the_key() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntitle: T\ndraft: true\n---\nbody\n");

        let out = vault
            .frontmatter(
                &name,
                "note",
                None,
                &FrontmatterAction::Remove,
                Some("draft"),
                None,
            )
            .unwrap();

        assert!(out.changed);
        assert_eq!(
            fs::read_to_string(dir.path().join("note.md")).unwrap(),
            "---\ntitle: T\n---\nbody\n"
        );
    }

    #[test]
    fn frontmatter_set_needs_a_key_and_a_value() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntitle: T\n---\nbody\n");

        assert!(
            vault
                .frontmatter(
                    &name,
                    "note",
                    None,
                    &FrontmatterAction::Set,
                    None,
                    Some(&serde_json::json!("x"))
                )
                .is_err()
        );
        assert!(
            vault
                .frontmatter(
                    &name,
                    "note",
                    None,
                    &FrontmatterAction::Set,
                    Some("k"),
                    None
                )
                .is_err()
        );
    }

    #[test]
    fn frontmatter_reports_malformed_yaml_rather_than_silently_reading_nothing() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\nkey: [unclosed\n---\nbody\n");

        let err = vault
            .frontmatter(&name, "note", None, &FrontmatterAction::Get, None, None)
            .unwrap_err();
        assert!(err.to_string().contains("frontmatter"), "{err}");
    }

    // ── VaultManager basics ───────────────────────────────────────────────────

    #[test]
    fn vault_basename_collisions_are_disambiguated() {
        use std::fs;
        let parent_a = TempDir::new().unwrap();
        let parent_b = TempDir::new().unwrap();
        fs::create_dir(parent_a.path().join("notes")).unwrap();
        fs::create_dir(parent_b.path().join("notes")).unwrap();
        let manager = VaultManager::new(vec![
            parent_a.path().join("notes"),
            parent_b.path().join("notes"),
        ]);
        let names: Vec<String> = manager.list_vaults().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names.len(), 2, "both vaults must be registered: {names:?}");
        assert!(names.iter().any(|n| n == "notes"));
        assert!(names.iter().any(|n| n == "notes-2"));
    }

    #[test]
    fn list_vaults_returns_sorted() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        let manager = VaultManager::new(vec![dir2.path().to_path_buf(), dir1.path().to_path_buf()]);
        let vaults = manager.list_vaults();
        assert!(!vaults.is_empty());
        // sorted
        let names: Vec<_> = vaults.iter().map(|(n, _)| n.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn resolve_vault_not_found_error() {
        let (_, vault) = make_vault();
        let err = vault.resolve_vault("nonexistent").unwrap_err();
        assert!(err.to_string().contains("nonexistent"));
    }

    #[test]
    fn note_path_adds_md_extension() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let p = vault.note_path(&name, "note", None).unwrap();
        assert!(p.to_str().unwrap().ends_with("note.md"));
    }

    #[test]
    fn note_path_keeps_md_extension() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let p = vault.note_path(&name, "note.md", None).unwrap();
        assert!(p.to_str().unwrap().ends_with("note.md"));
    }

    #[test]
    fn note_path_with_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let p = vault.note_path(&name, "note", Some("sub")).unwrap();
        assert!(p.to_str().unwrap().contains("sub"));
        assert!(p.to_str().unwrap().ends_with("note.md"));
    }

    #[test]
    fn note_path_empty_folder_ignores_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let with_empty = vault.note_path(&name, "note", Some("")).unwrap();
        let without = vault.note_path(&name, "note", None).unwrap();
        assert_eq!(with_empty, without);
    }

    // ── create_note ───────────────────────────────────────────────────────────

    #[test]
    fn create_note_writes_content() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        vault
            .create_note(&name, "new", "hello world", None)
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("new.md")).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn create_note_returns_path() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let p = vault.create_note(&name, "new", "", None).unwrap();
        assert!(p.exists());
    }

    #[test]
    fn create_note_creates_parent_directories() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        vault
            .create_note(&name, "deep", "content", Some("a/b/c"))
            .unwrap();
        assert!(dir.path().join("a/b/c/deep.md").exists());
    }

    #[test]
    fn create_note_errors_if_already_exists() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "exists.md", "");
        let err = vault
            .create_note(&name, "exists", "content", None)
            .unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    // ── read_note ─────────────────────────────────────────────────────────────

    #[test]
    fn read_note_returns_content() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "hello");
        assert_eq!(
            vault
                .read_note(&name, "note", None, &NoteView::Content)
                .unwrap(),
            "hello"
        );
    }

    #[test]
    fn read_note_accepts_md_extension() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "content");
        assert_eq!(
            vault
                .read_note(&name, "note.md", None, &NoteView::Content)
                .unwrap(),
            "content"
        );
    }

    #[test]
    fn read_note_error_if_not_found() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        assert!(
            vault
                .read_note(&name, "ghost", None, &NoteView::Content)
                .is_err()
        );
    }

    #[test]
    fn read_note_in_subfolder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/note.md"), "deep").unwrap();
        assert_eq!(
            vault
                .read_note(&name, "note", Some("sub"), &NoteView::Content)
                .unwrap(),
            "deep"
        );
    }

    // ── delete_note ───────────────────────────────────────────────────────────

    #[test]
    fn delete_note_removes_file() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "del.md", "bye");
        vault.delete_note(&name, "del", None, true).unwrap();
        assert!(!dir.path().join("del.md").exists());
    }

    #[test]
    fn delete_moves_the_note_to_the_trash_by_default() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "del.md", "precious");

        let out = vault.delete_note(&name, "del", None, false).unwrap();

        assert_eq!(out.trashed_to.as_deref(), Some(".trash/del.md"));
        assert!(!dir.path().join("del.md").exists());
        assert_eq!(
            fs::read_to_string(dir.path().join(".trash/del.md")).unwrap(),
            "precious",
            "the note must be recoverable, byte for byte"
        );
    }

    #[test]
    fn a_trashed_note_is_invisible_to_search() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "del.md", "needle");
        vault.delete_note(&name, "del", None, false).unwrap();

        let hits = vault
            .search_vault(
                &name,
                "needle",
                None,
                false,
                &SearchType::Content,
                &SearchLimits::default(),
            )
            .unwrap();
        assert!(
            hits.results.is_empty(),
            "a deleted note must behave as deleted: {hits:?}"
        );
    }

    #[test]
    fn delete_keeps_the_folder_structure_inside_the_trash() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("a")).unwrap();
        fs::create_dir_all(dir.path().join("b")).unwrap();
        fs::write(dir.path().join("a/note.md"), "from a").unwrap();
        fs::write(dir.path().join("b/note.md"), "from b").unwrap();

        vault.delete_note(&name, "note", Some("a"), false).unwrap();
        vault.delete_note(&name, "note", Some("b"), false).unwrap();

        // Same basename, different folders — neither may overwrite the other.
        assert_eq!(
            fs::read_to_string(dir.path().join(".trash/a/note.md")).unwrap(),
            "from a"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(".trash/b/note.md")).unwrap(),
            "from b"
        );
    }

    #[test]
    fn trashing_the_same_path_twice_does_not_overwrite_the_first() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "first");
        vault.delete_note(&name, "note", None, false).unwrap();
        write_note(&dir, "note.md", "second");
        let out = vault.delete_note(&name, "note", None, false).unwrap();

        assert_eq!(out.trashed_to.as_deref(), Some(".trash/note-2.md"));
        assert_eq!(
            fs::read_to_string(dir.path().join(".trash/note.md")).unwrap(),
            "first"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(".trash/note-2.md")).unwrap(),
            "second"
        );
    }

    #[test]
    fn permanent_delete_erases_the_note() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "del.md", "bye");

        let out = vault.delete_note(&name, "del", None, true).unwrap();

        assert!(out.trashed_to.is_none());
        assert!(!dir.path().join("del.md").exists());
        assert!(!dir.path().join(".trash").exists(), "nothing was trashed");
    }

    #[test]
    fn deleting_from_the_trash_erases_it() {
        // Emptying the trash is the one delete that has to actually erase.
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "del.md", "bye");
        vault.delete_note(&name, "del", None, false).unwrap();

        let out = vault
            .delete_note(&name, "del", Some(".trash"), false)
            .unwrap();

        assert!(
            out.trashed_to.is_none(),
            "must not re-trash into .trash/.trash"
        );
        assert!(!dir.path().join(".trash/del.md").exists());
    }

    #[test]
    fn delete_note_error_if_not_found() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        assert!(vault.delete_note(&name, "ghost", None, false).is_err());
    }

    #[test]
    fn delete_note_removes_emptied_source_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/note.md"), "body").unwrap();
        vault.delete_note(&name, "note", Some("sub"), true).unwrap();
        assert!(
            !dir.path().join("sub").exists(),
            "emptied source folder must be removed"
        );
    }

    #[test]
    fn delete_note_keeps_nonempty_source_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/a.md"), "a").unwrap();
        fs::write(dir.path().join("sub/b.md"), "b").unwrap();
        vault.delete_note(&name, "a", Some("sub"), true).unwrap();
        assert!(
            dir.path().join("sub").exists(),
            "source folder still has b.md and must stay"
        );
        assert!(dir.path().join("sub/b.md").exists());
    }

    #[test]
    fn delete_note_does_not_remove_vault_root() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "only.md", "body");
        vault.delete_note(&name, "only", None, true).unwrap();
        // The note lived directly in the vault root; the root must never be
        // pruned even though it is now empty of notes.
        assert!(dir.path().exists());
    }

    // ── move_note ─────────────────────────────────────────────────────────────

    #[test]
    fn move_note_renames_file() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "original.md", "body");
        let dest = vault
            .move_note(&name, "original", None, None, Some("renamed"))
            .unwrap();
        assert!(dest.path.exists());
        assert!(!dir.path().join("original.md").exists());
    }

    #[test]
    fn move_note_to_subfolder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "body");
        vault
            .move_note(&name, "note", None, Some("sub"), None)
            .unwrap();
        assert!(dir.path().join("sub/note.md").exists());
    }

    #[test]
    fn move_note_error_if_not_found() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        assert!(vault.move_note(&name, "ghost", None, None, None).is_err());
    }

    #[test]
    fn move_note_rewrites_inbound_links_on_rename() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "target.md", "the target\n");
        write_note(&dir, "a.md", "see [[target]] and ![[target#Sec|alias]]\n");
        write_note(&dir, "b.md", "and [label](target.md)\n");

        let out = vault
            .move_note(&name, "target", None, None, Some("renamed"))
            .unwrap();
        assert_eq!(out.relinked, vec!["a.md", "b.md"]);

        let a = fs::read_to_string(dir.path().join("a.md")).unwrap();
        assert_eq!(a, "see [[renamed]] and ![[renamed#Sec|alias]]\n");
        let b = fs::read_to_string(dir.path().join("b.md")).unwrap();
        assert_eq!(b, "and [label](renamed.md)\n");
    }

    #[test]
    fn move_note_leaves_links_in_code_blocks_alone() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "target.md", "x\n");
        write_note(&dir, "doc.md", "```\n[[target]]\n```\nreal [[target]]\n");

        vault
            .move_note(&name, "target", None, None, Some("renamed"))
            .unwrap();

        let doc = fs::read_to_string(dir.path().join("doc.md")).unwrap();
        assert_eq!(
            doc, "```\n[[target]]\n```\nreal [[renamed]]\n",
            "a link inside a code sample is documentation, not a reference"
        );
    }

    #[test]
    fn move_note_to_a_folder_leaves_bare_links_alone() {
        // The basename still resolves from anywhere, so there is nothing to fix.
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "target.md", "x\n");
        write_note(&dir, "a.md", "see [[target]]\n");

        let out = vault
            .move_note(&name, "target", None, Some("archive"), None)
            .unwrap();
        assert!(out.relinked.is_empty());
        assert_eq!(
            fs::read_to_string(dir.path().join("a.md")).unwrap(),
            "see [[target]]\n"
        );
    }

    // ── wikilinks ─────────────────────────────────────────────────────────────

    fn linked_vault() -> (TempDir, VaultManager, String) {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "hub.md", "x\n");
        write_note(&dir, "a.md", "see [[hub]]\n");
        write_note(&dir, "b.md", "also [[hub]] and [[ghost]]\n");
        write_note(&dir, "lonely.md", "nobody links here\n");
        (dir, vault, name)
    }

    #[test]
    fn wikilinks_backlinks_lists_the_linking_notes() {
        let (_dir, vault, name) = linked_vault();
        let out = vault
            .wikilinks(&name, &LinkQuery::Backlinks, Some("hub"), None)
            .unwrap();
        let from: Vec<&str> = out.links.iter().map(|l| l.from.as_str()).collect();
        assert_eq!(from, vec!["a.md", "b.md"]);
    }

    #[test]
    fn wikilinks_outgoing_lists_this_notes_links() {
        let (_dir, vault, name) = linked_vault();
        let out = vault
            .wikilinks(&name, &LinkQuery::Outgoing, Some("b"), None)
            .unwrap();
        let targets: Vec<&str> = out.links.iter().map(|l| l.target.as_str()).collect();
        assert_eq!(targets, vec!["hub", "ghost"]);
    }

    #[test]
    fn wikilinks_broken_finds_targets_that_do_not_exist() {
        let (_dir, vault, name) = linked_vault();
        let out = vault
            .wikilinks(&name, &LinkQuery::Broken, None, None)
            .unwrap();
        assert_eq!(out.links.len(), 1);
        assert_eq!(out.links[0].target, "ghost");
        assert!(out.links[0].resolved.is_none());
    }

    #[test]
    fn wikilinks_orphans_finds_notes_nothing_links_to() {
        let (_dir, vault, name) = linked_vault();
        let out = vault
            .wikilinks(&name, &LinkQuery::Orphans, None, None)
            .unwrap();
        assert_eq!(out.notes, vec!["a.md", "b.md", "lonely.md"]);
    }

    #[test]
    fn wikilinks_backlinks_requires_a_filename() {
        let (_dir, vault, name) = linked_vault();
        assert!(
            vault
                .wikilinks(&name, &LinkQuery::Backlinks, None, None)
                .is_err()
        );
    }

    #[test]
    fn wikilinks_blocks_traversal() {
        let (_dir, vault, name) = linked_vault();
        assert!(
            vault
                .wikilinks(
                    &name,
                    &LinkQuery::Backlinks,
                    Some("../../../etc/hosts"),
                    None
                )
                .is_err()
        );
    }

    #[test]
    fn move_note_removes_emptied_source_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/note.md"), "body").unwrap();
        vault
            .move_note(&name, "note", Some("src"), Some("dst"), None)
            .unwrap();
        assert!(dir.path().join("dst/note.md").exists());
        assert!(
            !dir.path().join("src").exists(),
            "emptied source folder must be removed"
        );
    }

    #[test]
    fn move_note_keeps_nonempty_source_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/a.md"), "a").unwrap();
        fs::write(dir.path().join("src/b.md"), "b").unwrap();
        vault
            .move_note(&name, "a", Some("src"), Some("dst"), None)
            .unwrap();
        assert!(
            dir.path().join("src").exists(),
            "source folder still has b.md and must stay"
        );
        assert!(dir.path().join("src/b.md").exists());
    }

    #[test]
    fn move_note_does_not_remove_vault_root() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "only.md", "body");
        vault
            .move_note(&name, "only", None, Some("sub"), None)
            .unwrap();
        // The note lived directly in the vault root; the root must never be
        // pruned even though it is now empty of notes.
        assert!(dir.path().exists());
        assert!(dir.path().join("sub/only.md").exists());
    }

    // ── create_directory ──────────────────────────────────────────────────────

    #[test]
    fn create_directory_recursive() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        vault.create_directory(&name, "a/b/c", true).unwrap();
        assert!(dir.path().join("a/b/c").is_dir());
    }

    #[test]
    fn create_directory_non_recursive() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        vault.create_directory(&name, "flat", false).unwrap();
        assert!(dir.path().join("flat").is_dir());
    }

    #[test]
    fn create_directory_error_if_already_exists() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("existing")).unwrap();
        assert!(vault.create_directory(&name, "existing", true).is_err());
    }

    // ── search_vault ──────────────────────────────────────────────────────────

    #[test]
    fn search_content_finds_matching_lines() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "a.md", "the quick brown fox");
        write_note(&dir, "b.md", "no match here");
        let results = vault
            .search_vault(
                &name,
                "quick",
                None,
                false,
                &SearchType::Content,
                &SearchLimits::default(),
            )
            .unwrap();
        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].filename, "a");
    }

    #[test]
    fn search_filename_finds_by_name() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "journal_2024.md", "");
        write_note(&dir, "other.md", "");
        let results = vault
            .search_vault(
                &name,
                "journal",
                None,
                false,
                &SearchType::Filename,
                &SearchLimits::default(),
            )
            .unwrap();
        assert_eq!(results.results.len(), 1);
    }

    #[test]
    fn search_both_finds_in_filename_and_content() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "target.md", "nothing special");
        write_note(&dir, "other.md", "has target word inside");
        let results = vault
            .search_vault(
                &name,
                "target",
                None,
                false,
                &SearchType::Both,
                &SearchLimits::default(),
            )
            .unwrap();
        assert_eq!(results.results.len(), 2);
    }

    #[test]
    fn search_tag_finds_frontmatter_tag() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "tagged.md", "---\ntags:\n  - work\n---\ncontent");
        write_note(&dir, "other.md", "no tags");
        let results = vault
            .search_vault(
                &name,
                "tag:work",
                None,
                false,
                &SearchType::Content,
                &SearchLimits::default(),
            )
            .unwrap();
        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].filename, "tagged");
    }

    #[test]
    fn search_tag_finds_inline_tag() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "inline.md", "some text #urgent here");
        let results = vault
            .search_vault(
                &name,
                "tag:urgent",
                None,
                false,
                &SearchType::Content,
                &SearchLimits::default(),
            )
            .unwrap();
        assert_eq!(results.results.len(), 1);
    }

    #[test]
    fn search_case_sensitive() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "Hello World");
        let insensitive = vault
            .search_vault(
                &name,
                "hello",
                None,
                false,
                &SearchType::Content,
                &SearchLimits::default(),
            )
            .unwrap();
        let sensitive = vault
            .search_vault(
                &name,
                "hello",
                None,
                true,
                &SearchType::Content,
                &SearchLimits::default(),
            )
            .unwrap();
        assert_eq!(insensitive.results.len(), 1);
        assert_eq!(sensitive.results.len(), 0);
    }

    #[test]
    fn search_with_path_limit() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/inner.md"), "needle").unwrap();
        write_note(&dir, "root.md", "needle");
        let results = vault
            .search_vault(
                &name,
                "needle",
                Some("sub"),
                false,
                &SearchType::Content,
                &SearchLimits::default(),
            )
            .unwrap();
        assert_eq!(results.results.len(), 1);
        assert_eq!(results.results[0].filename, "inner");
    }

    #[test]
    fn search_no_results() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "content");
        let results = vault
            .search_vault(
                &name,
                "zzz_not_here",
                None,
                false,
                &SearchType::Content,
                &SearchLimits::default(),
            )
            .unwrap();
        assert!(results.results.is_empty());
    }

    // ── add_tags ──────────────────────────────────────────────────────────────

    #[test]
    fn add_tags_to_frontmatter_block_list() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntags:\n  - existing\n---\nbody");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["new-tag".into()],
                "frontmatter",
                false,
                "end",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("new-tag"));
        assert!(content.contains("existing"));
    }

    #[test]
    fn add_tags_creates_frontmatter_if_absent() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "plain.md", "just content");
        vault
            .add_tags(
                &name,
                &["plain.md".into()],
                &["fresh".into()],
                "frontmatter",
                false,
                "end",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("plain.md")).unwrap();
        assert!(content.starts_with("---"));
        assert!(content.contains("fresh"));
    }

    #[test]
    fn add_tags_to_content_end() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "body text");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["inline".into()],
                "content",
                false,
                "end",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.ends_with("#inline"));
    }

    #[test]
    fn add_tags_to_content_start() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "body");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["first".into()],
                "content",
                false,
                "start",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("#first"));
    }

    #[test]
    fn add_tags_both_location() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "body");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["mytag".into()],
                "both",
                false,
                "end",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("mytag")); // in frontmatter
        assert!(content.contains("#mytag")); // inline
    }

    #[test]
    fn add_tags_skips_missing_files() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let modified = vault
            .add_tags(
                &name,
                &["ghost.md".into()],
                &["tag".into()],
                "frontmatter",
                false,
                "end",
            )
            .unwrap();
        assert!(modified.is_empty());
    }

    #[test]
    fn add_tags_normalizes() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "content");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["My Tag".into()],
                "frontmatter",
                true,
                "end",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("my-tag"));
    }

    #[test]
    fn add_tags_deduplicates() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntags:\n  - existing\n---\n");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["existing".into()],
                "frontmatter",
                false,
                "end",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        // should still only appear once
        assert_eq!(content.matches("existing").count(), 1);
    }

    #[test]
    fn add_tags_to_frontmatter_inline_tag_line() {
        // covers the inline (non-block) tags: branch in add_tags_to_frontmatter
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntags: [existing]\n---\nbody");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["added".into()],
                "frontmatter",
                false,
                "end",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("added"));
    }

    #[test]
    fn add_tags_to_content_start_with_frontmatter() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntitle: test\n---\nbody text");
        vault
            .add_tags(
                &name,
                &["note.md".into()],
                &["top".into()],
                "content",
                false,
                "start",
            )
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("#top"));
    }

    // ── remove_tags ───────────────────────────────────────────────────────────

    #[test]
    fn remove_tags_from_frontmatter() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(
            &dir,
            "note.md",
            "---\ntags:\n  - keep\n  - remove\n---\nbody",
        );
        vault
            .remove_tags(&name, &["note.md".into()], &["remove".into()])
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(!content.contains("  - remove"));
        assert!(content.contains("keep"));
    }

    #[test]
    fn remove_tags_from_content() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "text #remove more");
        vault
            .remove_tags(&name, &["note.md".into()], &["remove".into()])
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(!content.contains("#remove"));
    }

    #[test]
    fn remove_tags_skips_missing_files() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let modified = vault
            .remove_tags(&name, &["ghost.md".into()], &["t".into()])
            .unwrap();
        assert!(modified.is_empty());
    }

    // ── rename_tag ────────────────────────────────────────────────────────────

    #[test]
    fn rename_tag_in_frontmatter() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "---\ntags:\n  - old\n---\n");
        let modified = vault.rename_tag(&name, "old", "new").unwrap();
        assert_eq!(modified.len(), 1);
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("- new"));
        assert!(!content.contains("- old"));
    }

    #[test]
    fn rename_tag_inline() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "text #old-tag more");
        vault.rename_tag(&name, "old-tag", "new-tag").unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(content.contains("#new-tag"));
        assert!(!content.contains("#old-tag"));
    }

    #[test]
    fn rename_tag_does_not_corrupt_overlapping_inline_tags() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "text #foo and #foobar and #foo-extra end");
        vault.rename_tag(&name, "foo", "bar").unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        // exact #foo replaced; #foobar and #foo-extra preserved
        assert!(
            content.contains("text #bar and #foobar and #foo-extra end"),
            "got: {content:?}"
        );
    }

    #[test]
    fn remove_tags_does_not_corrupt_overlapping_inline_tags() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "#foo #foobar #foo-extra done");
        vault
            .remove_tags(&name, &["note.md".into()], &["foo".into()])
            .unwrap();
        let content = fs::read_to_string(dir.path().join("note.md")).unwrap();
        assert!(
            !content.contains("#foo "),
            "exact #foo must be gone: {content:?}"
        );
        assert!(content.contains("#foobar"), "got: {content:?}");
        assert!(content.contains("#foo-extra"), "got: {content:?}");
    }

    #[test]
    fn rename_tag_no_matches() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        write_note(&dir, "note.md", "no tags here");
        let modified = vault.rename_tag(&name, "absent", "new").unwrap();
        assert!(modified.is_empty());
    }

    // ── Path traversal / sandboxing ───────────────────────────────────────────

    #[test]
    fn rejects_parent_traversal_in_filename() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.note_path(&name, "../escaped", None);
        assert!(result.is_err(), "expected error, got {:?}", result);
        assert!(
            result.unwrap_err().to_string().contains("escapes vault"),
            "wrong error message"
        );
    }

    #[test]
    fn rejects_parent_traversal_in_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.note_path(&name, "note", Some("../.."));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("escapes vault"));
    }

    #[test]
    fn rejects_absolute_filename() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.note_path(&name, ABS_FILE, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("absolute"));
    }

    #[test]
    fn rejects_absolute_folder() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.note_path(&name, "note", Some(ABS_FOLDER));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("absolute"));
    }

    #[test]
    fn allows_normal_nested_path() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let p = vault
            .note_path(&name, "note", Some("subdir/inner"))
            .unwrap();
        assert!(p.starts_with(dir.path()));
        assert!(p.ends_with("note.md"));
    }

    #[test]
    fn create_note_blocks_traversal() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.create_note(&name, "../pwned", "x", None);
        assert!(result.is_err());
        // file must not have been created outside the vault
        assert!(!dir.path().parent().unwrap().join("pwned.md").exists());
    }

    #[test]
    fn delete_note_blocks_traversal() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        // create a sibling file outside the vault
        let outside = dir.path().parent().unwrap().join("sibling.md");
        fs::write(&outside, "private").unwrap();
        let result = vault.delete_note(&name, "../sibling", None, true);
        assert!(result.is_err());
        assert!(outside.exists(), "outside file must not be deleted");
        fs::remove_file(&outside).ok();
    }

    #[test]
    fn create_directory_blocks_traversal() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.create_directory(&name, "../escaped", true);
        assert!(result.is_err());
    }

    #[test]
    fn search_vault_blocks_traversal_in_path() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.search_vault(
            &name,
            "x",
            Some("../.."),
            false,
            &SearchType::Content,
            &SearchLimits::default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn add_tags_blocks_traversal() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        // create a sibling file outside the vault that must not be touched
        let outside = dir.path().parent().unwrap().join("sibling-add.md");
        fs::write(&outside, "untouched").unwrap();
        let result = vault.add_tags(
            &name,
            &["../sibling-add.md".into()],
            &["pwned".into()],
            "frontmatter",
            false,
            "end",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("escapes vault"));
        // outside file must be byte-identical
        assert_eq!(fs::read_to_string(&outside).unwrap(), "untouched");
        fs::remove_file(&outside).ok();
    }

    #[test]
    fn add_tags_blocks_absolute_path() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let result = vault.add_tags(
            &name,
            &[ABS_FILE.into()],
            &["x".into()],
            "frontmatter",
            false,
            "end",
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("absolute"));
    }

    #[test]
    fn frontmatter_blocks_traversal() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let outside = dir.path().parent().unwrap().join("sibling-fm.md");
        fs::write(&outside, "---\ntitle: private\n---\n").unwrap();

        let result = vault.frontmatter(
            &name,
            "../sibling-fm",
            None,
            &FrontmatterAction::Set,
            Some("pwned"),
            Some(&serde_json::json!(true)),
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("escapes vault"));
        assert_eq!(
            fs::read_to_string(&outside).unwrap(),
            "---\ntitle: private\n---\n",
            "the outside note must be byte-identical"
        );
        fs::remove_file(&outside).ok();
    }

    #[test]
    fn remove_tags_blocks_traversal() {
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        let outside = dir.path().parent().unwrap().join("sibling-rm.md");
        fs::write(&outside, "#keep").unwrap();
        let result = vault.remove_tags(&name, &["../sibling-rm.md".into()], &["keep".into()]);
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "#keep");
        fs::remove_file(&outside).ok();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        use std::os::unix::fs::symlink;
        let outer = TempDir::new().unwrap();
        let secret = outer.path().join("secret.md");
        fs::write(&secret, "top secret").unwrap();

        let (vault_dir, vault) = make_vault();
        let name = vault_name(&vault_dir);
        // symlink inside the vault pointing to a directory outside
        symlink(outer.path(), vault_dir.path().join("escape")).unwrap();

        let result = vault.read_note(&name, "secret", Some("escape"), &NoteView::Content);
        assert!(
            result.is_err(),
            "symlink escape must be rejected, got {:?}",
            result
        );
        assert!(result.unwrap_err().to_string().contains("escapes vault"));
    }

    #[cfg(unix)]
    #[test]
    fn allows_symlink_staying_inside_vault() {
        use std::os::unix::fs::symlink;
        let (dir, vault) = make_vault();
        let name = vault_name(&dir);
        fs::create_dir(dir.path().join("real")).unwrap();
        fs::write(dir.path().join("real/note.md"), "hello").unwrap();
        symlink(dir.path().join("real"), dir.path().join("link")).unwrap();

        let content = vault
            .read_note(&name, "note", Some("link"), &NoteView::Content)
            .unwrap();
        assert_eq!(content, "hello");
    }
}
