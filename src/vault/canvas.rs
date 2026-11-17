use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
};

use jsonc_parser::{
    ParseOptions,
    cst::{CstInputValue, CstObject, CstRootNode},
};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::{
    error::VaultError,
    tools::canvas::{CanvasPatch, CreateCanvasParams, EditCanvasParams, ReadCanvasParams},
};

use super::{
    VaultManager, check_note_revision, note_revision,
    path::safe_join,
    rel_path,
    write::{atomic_write, atomic_write_new},
};

#[derive(Debug, Serialize, JsonSchema)]
pub struct CanvasResult {
    pub path: String,
    pub revision: String,
    pub nodes: Vec<Value>,
    pub edges: Vec<Value>,
}

#[derive(Debug)]
pub(crate) struct CanvasMove {
    pub source_vault: String,
    pub source_path: String,
    pub destination_vault: String,
    pub destination_path: String,
}

pub(crate) struct CanvasLinkUpdate {
    pub vault: String,
    pub path: String,
    pub(crate) revision: String,
    pub(crate) content: String,
    pub(crate) converted: usize,
}

impl VaultManager {
    pub fn read_canvas(&self, params: &ReadCanvasParams) -> Result<CanvasResult, VaultError> {
        let root = self.resolve_vault(&params.vault)?;
        let path = board_path(root, &params.path)?;
        let body = read_board(&path, &params.vault)?;
        board_result(root, &path, &body)
    }

    pub fn create_canvas(&self, params: &CreateCanvasParams) -> Result<CanvasResult, VaultError> {
        let _guard = self.write_guard();
        let root = self.resolve_vault(&params.vault)?;
        let path = board_path(root, &params.path)?;
        self.ensure_write_allowed(&params.vault, &path)?;
        let mut nodes = params.nodes.clone();
        let edges = params.edges.clone();
        place_nodes(&[], &mut nodes)?;
        for node in &nodes {
            validate_node(node, root, true)?;
        }
        for edge in &edges {
            validate_edge(edge)?;
        }
        let body = serde_json::to_string_pretty(&json!({"nodes": nodes, "edges": edges}))
            .map_err(|e| invalid(e.to_string()))?
            + "\n";
        let result = board_result(root, &path, &body)?;
        ensure_parent(&path)?;
        atomic_write_new(&path, body.as_bytes()).map_err(|e| match e {
            VaultError::Io(_, error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                VaultError::NoteAlreadyExists(result.path.clone(), params.vault.clone())
            }
            other => other,
        })?;
        Ok(result)
    }

    pub fn edit_canvas(&self, params: &EditCanvasParams) -> Result<CanvasResult, VaultError> {
        let _guard = self.write_guard();
        let root = self.resolve_vault(&params.vault)?;
        let path = board_path(root, &params.path)?;
        self.ensure_write_allowed(&params.vault, &path)?;
        let original = read_board(&path, &params.vault)?;
        check_note_revision(&path, original.as_bytes(), &params.expected_revision)?;
        let edit = parse_cst(&original)?;
        let object = edit
            .object_value()
            .ok_or_else(|| invalid("Canvas must be an object."))?;
        let mut existing_nodes = board_records(&original, "nodes")?;
        let mut existing_edges = board_records(&original, "edges")?;
        amend_records(
            &mut existing_nodes,
            &params.update_nodes,
            &params.remove_nodes,
            true,
            root,
        )?;
        amend_records(
            &mut existing_edges,
            &params.update_edges,
            &params.remove_edges,
            false,
            root,
        )?;
        let removed: HashSet<_> = params.remove_nodes.iter().map(String::as_str).collect();
        existing_edges.retain(|edge| !edge_touches(edge, &removed));
        let mut new_nodes = params.add_nodes.clone();
        place_nodes(&existing_nodes, &mut new_nodes)?;
        for node in &new_nodes {
            validate_node(node, root, true)?;
        }
        for edge in &params.add_edges {
            validate_edge(edge)?;
        }
        existing_nodes.extend(new_nodes);
        existing_edges.extend(params.add_edges.clone());
        validate_graph(&existing_nodes, &existing_edges)?;

        edit_records(&object, "nodes", &existing_nodes)?;
        edit_records(&object, "edges", &existing_edges)?;
        let updated = edit.to_string();
        let result = board_result(root, &path, &updated)?;
        if updated != original {
            atomic_write(&path, updated.as_bytes())?;
        }
        Ok(result)
    }

    pub(crate) fn plan_canvas_note_moves(
        &self,
        moves: &[CanvasMove],
        convert: bool,
    ) -> Result<Vec<CanvasLinkUpdate>, VaultError> {
        let mut updates = Vec::new();
        let mut blockers = Vec::new();
        for (vault, root) in self.list_vaults() {
            if !moves.iter().any(|entry| entry.source_vault == vault) {
                continue;
            }
            for entry in ignore::WalkBuilder::new(&root).follow_links(false).build() {
                let entry = entry.map_err(|e| {
                    invalid(format!("Cannot inspect Canvas boards in {vault}: {e}"))
                })?;
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("canvas")
                    || !entry.file_type().is_some_and(|t| t.is_file())
                {
                    continue;
                }
                let relative = rel_path(&root, path);
                let path = safe_join(&root, None, &relative)?;
                let body = read_board(&path, &vault)?;
                let board = parse_cst(&body)?;
                let Some(nodes) = board
                    .object_value()
                    .and_then(|object| object.array_value("nodes"))
                else {
                    continue;
                };
                let mut touched = false;
                let mut converted = 0;
                for node in nodes.elements() {
                    let Some(node) = node.as_object() else {
                        continue;
                    };
                    let Some(data) = node.to_serde_value() else {
                        continue;
                    };
                    if data["type"] != "file" {
                        continue;
                    }
                    let Some(file) = data["file"].as_str() else {
                        continue;
                    };
                    let Some(entry) = moves
                        .iter()
                        .find(|entry| entry.source_vault == vault && entry.source_path == file)
                    else {
                        continue;
                    };

                    if entry.destination_vault == vault {
                        set_field(
                            &node,
                            "file",
                            &Value::String(entry.destination_path.clone()),
                        )?;
                    } else if convert {
                        let anchor = data["subpath"].as_str().unwrap_or_default();
                        let label = Path::new(&entry.destination_path)
                            .file_stem()
                            .and_then(|name| name.to_str())
                            .unwrap_or(&entry.destination_path);
                        set_field(&node, "type", &json!("text"))?;
                        set_field(
                            &node,
                            "text",
                            &json!(format!(
                                "[{}]({})",
                                markdown_label(label),
                                self.obsidian_note_uri(
                                    &entry.destination_vault,
                                    &entry.destination_path,
                                    anchor
                                )?
                            )),
                        )?;
                        if let Some(file) = node.get("file") {
                            file.remove();
                        }
                        if let Some(subpath) = node.get("subpath") {
                            subpath.remove();
                        }
                        converted += 1;
                    } else {
                        blockers.push(format!("{vault}/{relative} has a Canvas file card. Allow conversion to a text card with a link before moving it across vaults."));
                        continue;
                    }
                    touched = true;
                }
                if touched {
                    if let Err(error) = self.ensure_write_allowed(&vault, &path) {
                        blockers.push(format!("{vault}/{relative}: {error}"));
                    } else {
                        updates.push(CanvasLinkUpdate {
                            vault: vault.clone(),
                            path: relative,
                            revision: note_revision(body.as_bytes()),
                            content: board.to_string(),
                            converted,
                        });
                    }
                }
            }
        }
        if !blockers.is_empty() {
            blockers.sort();
            blockers.dedup();
            return Err(invalid(blockers.join("\n")));
        }
        updates.sort_by(|a, b| (&a.vault, &a.path).cmp(&(&b.vault, &b.path)));
        Ok(updates)
    }

    pub(crate) fn apply_canvas_link_update(
        &self,
        update: &CanvasLinkUpdate,
    ) -> Result<(), VaultError> {
        let root = self.resolve_vault(&update.vault)?;
        let path = board_path(root, &update.path)?;
        self.ensure_write_allowed(&update.vault, &path)?;
        let current = read_board(&path, &update.vault)?;
        check_note_revision(&path, current.as_bytes(), &update.revision)?;
        atomic_write(&path, update.content.as_bytes())
    }
}

fn board_path(root: &Path, path: &str) -> Result<PathBuf, VaultError> {
    validate_relative(path)?;
    let path = if path.ends_with(".canvas") {
        path.to_owned()
    } else {
        format!("{path}.canvas")
    };
    safe_join(root, None, &path)
}

pub(super) fn ensure_parent(path: &Path) -> Result<(), VaultError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Destination has no parent folder."))?;
    fs::create_dir_all(parent).map_err(|error| VaultError::io(parent.display().to_string(), error))
}

pub(super) fn validate_relative(path: &str) -> Result<(), VaultError> {
    if path.is_empty()
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(VaultError::InvalidPath(format!(
            "Use a path from the vault root: '{path}'."
        )));
    }
    Ok(())
}

fn read_board(path: &Path, vault: &str) -> Result<String, VaultError> {
    fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            invalid(format!(
                "Canvas '{}' was not found in vault '{vault}'.",
                path.display()
            ))
        } else {
            VaultError::io(path.display().to_string(), e)
        }
    })
}

fn invalid(message: impl Into<String>) -> VaultError {
    VaultError::MissingArgument(message.into())
}

fn parse_cst(body: &str) -> Result<CstRootNode, VaultError> {
    CstRootNode::parse(body, &ParseOptions::default())
        .map_err(|e| invalid(format!("Invalid JSON Canvas: {e}")))
}

fn board_records(body: &str, key: &str) -> Result<Vec<Map<String, Value>>, VaultError> {
    let document: Value =
        serde_json::from_str(body).map_err(|e| invalid(format!("Invalid JSON Canvas: {e}")))?;
    let board = document
        .as_object()
        .ok_or_else(|| invalid("Canvas must be an object."))?;
    board.get(key).map_or_else(
        || Ok(Vec::new()),
        |records| {
            let array = records
                .as_array()
                .ok_or_else(|| invalid(format!("Canvas {key} must be an array.")))?;
            array
                .iter()
                .map(|record| {
                    record
                        .as_object()
                        .cloned()
                        .ok_or_else(|| invalid(format!("Each element of {key} must be an object.")))
                })
                .collect()
        },
    )
}

fn board_result(root: &Path, path: &Path, body: &str) -> Result<CanvasResult, VaultError> {
    let nodes = board_records(body, "nodes")?;
    let edges = board_records(body, "edges")?;
    validate_graph(&nodes, &edges)?;
    Ok(CanvasResult {
        path: rel_path(root, path),
        revision: note_revision(body.as_bytes()),
        nodes: nodes.into_iter().map(Value::Object).collect(),
        edges: edges.into_iter().map(Value::Object).collect(),
    })
}

fn required_string<'a>(record: &'a Map<String, Value>, key: &str) -> Result<&'a str, VaultError> {
    record
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid(format!("Canvas {key} must be a non-empty string.")))
}

fn validate_graph(
    nodes: &[Map<String, Value>],
    edges: &[Map<String, Value>],
) -> Result<(), VaultError> {
    let mut identifiers = HashSet::new();
    for node in nodes {
        let id = required_string(node, "id")?;
        if !identifiers.insert(id) {
            return Err(invalid(format!("Canvas node ID '{id}' is repeated.")));
        }
    }
    let mut edge_ids = HashSet::new();
    for edge in edges {
        let id = required_string(edge, "id")?;
        if !edge_ids.insert(id) {
            return Err(invalid(format!("Canvas edge ID '{id}' is repeated.")));
        }
        for key in ["fromNode", "toNode"] {
            let target = required_string(edge, key)?;
            if !identifiers.contains(target) {
                return Err(invalid(format!(
                    "Canvas edge '{id}' refers to missing node '{target}'."
                )));
            }
        }
    }
    Ok(())
}

fn validate_node(
    node: &Map<String, Value>,
    root: &Path,
    validate_reference: bool,
) -> Result<(), VaultError> {
    required_string(node, "id")?;
    rectangle(node)?;
    for key in ["text", "label", "subpath", "background", "backgroundStyle"] {
        if node.get(key).is_some_and(|value| !value.is_string()) {
            return Err(invalid(format!("Canvas {key} must be a string.")));
        }
    }
    match required_string(node, "type")? {
        "text" if node.get("text").is_some_and(Value::is_string) => {}
        "text" => return Err(invalid("A text card needs a text field.")),
        "file" => {
            let file = required_string(node, "file")?;
            if validate_reference {
                validate_board_file(root, file)?;
            }
            if node
                .get("subpath")
                .and_then(Value::as_str)
                .is_some_and(|subpath| !subpath.starts_with('#') || subpath.len() < 2)
            {
                return Err(invalid(
                    "A Canvas subpath must start with # and name a heading or block.",
                ));
            }
        }
        "link" => {
            let url = required_string(node, "url")?;
            if !(url.starts_with("https://")
                || url.starts_with("http://")
                || url.starts_with("obsidian://"))
                || url.chars().any(char::is_whitespace)
            {
                return Err(invalid(
                    "A link card needs an HTTP, HTTPS, or Obsidian URL.",
                ));
            }
        }
        "group" => {
            if let Some(background) = node.get("background").and_then(Value::as_str)
                && validate_reference
            {
                validate_board_file(root, background)?;
            }
            validate_option(node, "backgroundStyle", &["cover", "ratio", "repeat"])?;
        }
        kind => return Err(invalid(format!("Unsupported Canvas node type '{kind}'."))),
    }
    validate_color(node)
}

fn validate_board_file(root: &Path, file: &str) -> Result<(), VaultError> {
    validate_relative(file)?;
    let path = safe_join(root, None, file)?;
    if !path.is_file() {
        return Err(invalid(format!("Canvas file '{file}' does not exist.")));
    }
    Ok(())
}

fn validate_option(
    item: &Map<String, Value>,
    key: &str,
    allowed: &[&str],
) -> Result<(), VaultError> {
    if item
        .get(key)
        .is_some_and(|value| !value.as_str().is_some_and(|text| allowed.contains(&text)))
    {
        return Err(invalid(format!(
            "Invalid Canvas {key}. Use one of {}.",
            allowed.join(", ")
        )));
    }
    Ok(())
}

fn validate_color(item: &Map<String, Value>) -> Result<(), VaultError> {
    if let Some(color) = item.get("color") {
        let valid = color.as_str().is_some_and(|color| {
            matches!(color, "1" | "2" | "3" | "4" | "5" | "6")
                || color
                    .strip_prefix('#')
                    .is_some_and(|rgb| rgb.len() == 6 && rgb.bytes().all(|b| b.is_ascii_hexdigit()))
        });
        if !valid {
            return Err(invalid(
                "Canvas color must be a preset 1–6 or a six-digit hexadecimal color.",
            ));
        }
    }
    Ok(())
}

fn validate_edge(edge: &Map<String, Value>) -> Result<(), VaultError> {
    for field in ["id", "fromNode", "toNode"] {
        required_string(edge, field)?;
    }
    for field in ["fromSide", "toSide"] {
        validate_option(edge, field, &["top", "right", "bottom", "left"])?;
    }
    for field in ["fromEnd", "toEnd"] {
        validate_option(edge, field, &["none", "arrow"])?;
    }
    if edge.get("label").is_some_and(|value| !value.is_string()) {
        return Err(invalid("Canvas edge label must be a string."));
    }
    validate_color(edge)
}

#[derive(Clone, Copy)]
struct Rectangle {
    x: i64,
    y: i64,
    width: i64,
    height: i64,
}

impl Rectangle {
    fn overlaps(self, other: Self) -> bool {
        self.x < other.x + other.width
            && other.x < self.x + self.width
            && self.y < other.y + other.height
            && other.y < self.y + self.height
    }
}

fn rectangle(node: &Map<String, Value>) -> Result<Rectangle, VaultError> {
    fn integer(node: &Map<String, Value>, key: &str, min: i64) -> Result<i64, VaultError> {
        node.get(key)
            .and_then(Value::as_i64)
            .filter(|value| (min..=i64::from(i32::MAX)).contains(value))
            .ok_or_else(|| {
                invalid(format!(
                    "Canvas {key} is outside the supported coordinate range."
                ))
            })
    }
    Ok(Rectangle {
        x: integer(node, "x", i64::from(i32::MIN))?,
        y: integer(node, "y", i64::from(i32::MIN))?,
        width: integer(node, "width", 1)?,
        height: integer(node, "height", 1)?,
    })
}

fn place_nodes(
    existing: &[Map<String, Value>],
    new: &mut [Map<String, Value>],
) -> Result<(), VaultError> {
    let mut occupied: Vec<_> = existing
        .iter()
        .filter_map(|node| rectangle(node).ok())
        .collect();
    for node in new.iter_mut() {
        node.entry("width").or_insert_with(|| json!(320));
        node.entry("height").or_insert_with(|| json!(180));
        match (node.contains_key("x"), node.contains_key("y")) {
            (true, true) => occupied.push(rectangle(node)?),
            (false, false) => {}
            _ => {
                return Err(invalid(
                    "Set both x and y or omit both for automatic placement.",
                ));
            }
        }
    }
    for node in new.iter_mut().filter(|node| !node.contains_key("x")) {
        node.insert("x".into(), json!(0));
        node.insert("y".into(), json!(0));
        let mut area = rectangle(node)?;
        while let Some(right) = occupied
            .iter()
            .filter(|other| area.overlaps(**other))
            .map(|other| other.x + other.width)
            .max()
        {
            area.x = right + 48;
        }
        node.insert("x".into(), json!(area.x));
        occupied.push(rectangle(node)?);
    }
    Ok(())
}

fn edge_touches(edge: &Map<String, Value>, removed: &HashSet<&str>) -> bool {
    ["fromNode", "toNode"].iter().any(|key| {
        edge.get(*key)
            .and_then(Value::as_str)
            .is_some_and(|node| removed.contains(node))
    })
}

fn amend_records(
    records: &mut Vec<Map<String, Value>>,
    updates: &[CanvasPatch],
    removals: &[String],
    nodes: bool,
    root: &Path,
) -> Result<(), VaultError> {
    let mut ids = HashSet::<&str>::new();
    for id in removals
        .iter()
        .map(String::as_str)
        .chain(updates.iter().map(|patch| patch.id.as_str()))
    {
        if !ids.insert(id) {
            return Err(invalid(format!("Canvas item '{id}' has multiple changes.")));
        }
        if !records
            .iter()
            .any(|record| record.get("id").and_then(Value::as_str) == Some(id))
        {
            return Err(invalid(format!("Canvas item '{id}' was not found.")));
        }
    }
    for patch in updates {
        let record = records
            .iter_mut()
            .find(|record| record.get("id").and_then(Value::as_str) == Some(&patch.id))
            .ok_or_else(|| invalid("Canvas item was not found."))?;
        for (key, value) in &patch.fields {
            if key == "id" || (nodes && key == "type") {
                return Err(invalid("Canvas item IDs and node types cannot be changed."));
            }
            if value.is_null() {
                record.remove(key);
            } else {
                record.insert(key.clone(), value.clone());
            }
        }
        if nodes {
            validate_node(
                record,
                root,
                patch.fields.contains_key("file") || patch.fields.contains_key("background"),
            )?;
        } else {
            validate_edge(record)?;
        }
    }
    records.retain(|record| {
        !record
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| removals.iter().any(|removed| removed == id))
    });
    Ok(())
}

fn edit_records(
    root: &CstObject,
    key: &str,
    expected: &[Map<String, Value>],
) -> Result<(), VaultError> {
    let array = match root.array_value(key) {
        Some(array) => array,
        None if expected.is_empty() => return Ok(()),
        None => root.array_value_or_set(key),
    };
    let by_id: HashMap<_, _> = expected
        .iter()
        .filter_map(|record| Some((record.get("id")?.as_str()?, record)))
        .collect();
    let mut seen = HashSet::new();
    for element in array.elements() {
        let object = element
            .as_object()
            .ok_or_else(|| invalid("Invalid Canvas item."))?;
        let before = object
            .to_serde_value()
            .and_then(|value| value.as_object().cloned())
            .ok_or_else(|| invalid("Invalid Canvas item."))?;
        let id = required_string(&before, "id")?;
        if let Some(after) = by_id.get(id) {
            seen.insert((*after).get("id").and_then(Value::as_str).unwrap());
            for property in before.keys().filter(|name| !after.contains_key(*name)) {
                if let Some(current) = object.get(property) {
                    current.remove();
                }
            }
            for (key, value) in *after {
                if before.get(key) != Some(value) {
                    set_field(&object, key, value)?;
                }
            }
        } else {
            object.clone().remove();
        }
    }
    for item in expected {
        if !seen.contains(required_string(item, "id")?) {
            array.append(to_cst(&Value::Object(item.clone()))?);
        }
    }
    Ok(())
}

fn to_cst(value: &Value) -> Result<CstInputValue, VaultError> {
    Ok(match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(value) => CstInputValue::Bool(*value),
        Value::Number(value) => CstInputValue::Number(value.to_string()),
        Value::String(value) => CstInputValue::String(value.clone()),
        Value::Array(values) => {
            CstInputValue::Array(values.iter().map(to_cst).collect::<Result<_, _>>()?)
        }
        Value::Object(values) => CstInputValue::Object(
            values
                .iter()
                .map(|(key, value)| Ok((key.clone(), to_cst(value)?)))
                .collect::<Result<_, VaultError>>()?,
        ),
    })
}

fn set_field(object: &CstObject, key: &str, value: &Value) -> Result<(), VaultError> {
    let value = to_cst(value)?;
    if let Some(prop) = object.get(key) {
        prop.set_value(value);
    } else {
        object.append(key, value);
    }
    Ok(())
}

pub(super) fn markdown_label(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

pub(super) fn encode_uri(text: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

impl VaultManager {
    pub(super) fn obsidian_note_uri(
        &self,
        vault: &str,
        path: &str,
        anchor: &str,
    ) -> Result<String, VaultError> {
        let root = self.resolve_vault(vault)?;
        let name = root.file_name().and_then(|name| name.to_str());
        if let Some(name) = name
            && self
                .list_vaults()
                .iter()
                .filter(|(_, candidate)| {
                    candidate.file_name().and_then(|name| name.to_str()) == Some(name)
                })
                .count()
                == 1
        {
            return Ok(format!(
                "obsidian://open?vault={}&file={}",
                encode_uri(name),
                encode_uri(&format!("{path}{anchor}"))
            ));
        }
        let joined = safe_join(root, None, path)?;
        let absolute = std::path::absolute(&joined)
            .map_err(|e| VaultError::io(joined.display().to_string(), e))?;
        let absolute = absolute
            .to_str()
            .ok_or_else(|| invalid("Obsidian links require a valid UTF-8 file path."))?;
        Ok(format!(
            "obsidian://open?path={}",
            encode_uri(&format!("{absolute}{anchor}"))
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> (tempfile::TempDir, VaultManager, String) {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("Work");
        fs::create_dir(&vault).unwrap();
        let manager = VaultManager::with_lock_path(vec![vault], dir.path().join("lock"));
        (dir, manager, "Work".into())
    }

    #[test]
    fn canvas_edit_places_new_cards_and_preserves_unknown_fields_and_old_format() {
        let (dir, manager, vault) = workspace();
        let original = "{\n  \"plugin\": { \"keep\":  1 },\n  \"nodes\": [{\"id\":\"a\",\"type\":\"text\",\"text\":\"old\",\"x\":0,\"y\":0,\"width\":320,\"height\":180,\"extra\": \"keep\"}]\n}\n";
        fs::write(dir.path().join("Work/Board.canvas"), original).unwrap();
        let input: EditCanvasParams = serde_json::from_value(json!({
            "vault": vault,
            "path": "Board",
            "expectedRevision": note_revision(original.as_bytes()),
            "addNodes": [{"id":"b","type":"text","text":"new"}],
            "addEdges": [{"id":"edge","fromNode":"a","toNode":"b"}]
        }))
        .unwrap();
        let result = manager.edit_canvas(&input).unwrap();
        assert_eq!(result.nodes[0]["x"], 0);
        assert_eq!(result.nodes[0]["extra"], "keep");
        assert_eq!(result.nodes[1]["x"], 368);
        assert_eq!(result.edges[0]["fromNode"], "a");
        assert!(
            fs::read_to_string(dir.path().join("Work/Board.canvas"))
                .unwrap()
                .contains("\"plugin\": { \"keep\":  1 }")
        );
        assert!(manager.edit_canvas(&input).is_err());
    }

    #[test]
    fn canvas_create_validates_refs_and_never_overwrites() {
        let (dir, manager, vault) = workspace();
        fs::write(dir.path().join("Work/Source.md"), "# Next").unwrap();
        let input: CreateCanvasParams = serde_json::from_value(json!({
            "vault": vault, "path": "Board.canvas",
            "nodes": [{"id":"file","type":"file","file":"Source.md","subpath":"#Next"}]
        }))
        .unwrap();
        let result = manager.create_canvas(&input).unwrap();
        assert_eq!(result.nodes[0]["subpath"], "#Next");
        assert!(manager.create_canvas(&input).is_err());
        let invalid: CreateCanvasParams =
            serde_json::from_value(json!({"vault": "Work", "path": "../Board", "nodes":[]}))
                .unwrap();
        assert!(manager.create_canvas(&invalid).is_err());
    }

    #[test]
    fn single_move_updates_canvas_card_and_blocks_a_protected_board() {
        let (dir, manager, vault) = workspace();
        let root = dir.path().join("Work");
        fs::create_dir(root.join("Inbox")).unwrap();
        fs::write(root.join("Inbox/Old.md"), "# Next").unwrap();
        let input: CreateCanvasParams = serde_json::from_value(json!({
            "vault": vault, "path": "Board",
            "nodes": [{"id":"file","type":"file","file":"Inbox/Old.md","subpath":"#Next", "x":42, "y":5}]
        })).unwrap();
        manager.create_canvas(&input).unwrap();
        let restricted = VaultManager::with_lock_path(vec![root.clone()], dir.path().join("lock"))
            .with_write_folders(&["Work=Inbox".into()])
            .unwrap();
        let blocked = restricted
            .move_note("Work", "Inbox/Old.md", None, None, Some("New"))
            .unwrap_err();
        assert!(blocked.to_string().contains("Board.canvas"));
        assert!(root.join("Inbox/Old.md").exists());
        manager
            .move_note("Work", "Inbox/Old.md", None, None, Some("New"))
            .unwrap();
        let board = manager
            .read_canvas(&ReadCanvasParams {
                vault: "Work".into(),
                path: "Board".into(),
            })
            .unwrap();
        assert_eq!(board.nodes[0]["file"], "Inbox/New.md");
        assert_eq!(board.nodes[0]["subpath"], "#Next");
        assert_eq!(board.nodes[0]["x"], 42);
    }
}
