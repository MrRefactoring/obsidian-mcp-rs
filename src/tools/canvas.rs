use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadCanvasParams {
    #[doc = "Name of the vault."]
    pub vault: String,
    #[doc = "Path to the board from the vault root. The .canvas extension is optional."]
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateCanvasParams {
    #[doc = "Name of the vault."]
    pub vault: String,
    #[doc = "Path to the new board from the vault root."]
    pub path: String,
    #[doc = "JSON Canvas nodes. Omit both x and y for automatic placement."]
    #[serde(default)]
    pub nodes: Vec<Map<String, Value>>,
    #[doc = "JSON Canvas edges. Use node IDs in fromNode and toNode."]
    #[serde(default)]
    pub edges: Vec<Map<String, Value>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EditCanvasParams {
    #[doc = "Name of the vault."]
    pub vault: String,
    #[doc = "Path to the board from the vault root."]
    pub path: String,
    #[doc = "Revision returned by read_canvas."]
    pub expected_revision: String,
    #[doc = "New JSON Canvas nodes. Omit both x and y for automatic placement."]
    #[serde(default)]
    pub add_nodes: Vec<Map<String, Value>>,
    #[doc = "Node fields to update, by ID. Set an optional field to null to remove it."]
    #[serde(default)]
    pub update_nodes: Vec<CanvasPatch>,
    #[doc = "IDs of nodes to remove, with their connected edges."]
    #[serde(default)]
    pub remove_nodes: Vec<String>,
    #[doc = "New JSON Canvas edges."]
    #[serde(default)]
    pub add_edges: Vec<Map<String, Value>>,
    #[doc = "Edge fields to update, by ID. Set an optional field to null to remove it."]
    #[serde(default)]
    pub update_edges: Vec<CanvasPatch>,
    #[doc = "IDs of edges to remove."]
    #[serde(default)]
    pub remove_edges: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CanvasPatch {
    #[doc = "ID from read_canvas."]
    pub id: String,
    #[doc = "Changed JSON Canvas fields. Omitted fields stay unchanged."]
    pub fields: Map<String, Value>,
}
