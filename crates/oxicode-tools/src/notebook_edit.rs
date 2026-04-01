use async_trait::async_trait;
use oxicode_common::OxiResult;

use crate::tool_trait::{PermissionLevel, Tool, ToolContext, ToolResult, ToolSchema};

/// Edit Jupyter notebook cells.
pub struct NotebookEditTool;

#[async_trait]
impl Tool for NotebookEditTool {
    fn name(&self) -> &str {
        "notebook_edit"
    }

    fn description(&self) -> &str {
        "Edit Jupyter notebook (.ipynb) cells — insert, replace, or delete."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().into(),
            description: self.description().into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the .ipynb file"
                    },
                    "cell_index": {
                        "type": "integer",
                        "description": "Index of cell to modify (0-based)"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["insert", "replace", "delete"],
                        "description": "Action to perform on the cell"
                    },
                    "content": {
                        "type": "string",
                        "description": "New cell content (for insert/replace)"
                    },
                    "cell_type": {
                        "type": "string",
                        "enum": ["code", "markdown"],
                        "description": "Cell type (default: code)"
                    }
                },
                "required": ["file_path", "cell_index", "action"]
            }),
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::FileWrite
    }

    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> OxiResult<ToolResult> {
        let file_path =
            input["file_path"]
                .as_str()
                .ok_or_else(|| oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: "file_path is required".into(),
                })?;

        let cell_index =
            input["cell_index"]
                .as_u64()
                .ok_or_else(|| oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: "cell_index is required".into(),
                })? as usize;

        let action = input["action"]
            .as_str()
            .ok_or_else(|| oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: "action is required".into(),
            })?;

        let path = if std::path::Path::new(file_path).is_absolute() {
            std::path::PathBuf::from(file_path)
        } else {
            ctx.working_dir.join(file_path)
        };

        let raw =
            tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: format!("Failed to read {}: {e}", path.display()),
                })?;

        let mut notebook: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: format!("Invalid notebook JSON: {e}"),
            })?;

        let cells =
            notebook["cells"]
                .as_array_mut()
                .ok_or_else(|| oxicode_common::OxiError::Tool {
                    name: self.name().into(),
                    message: "Notebook has no cells array".into(),
                })?;

        match action {
            "delete" => {
                if cell_index >= cells.len() {
                    return Ok(ToolResult::error(format!(
                        "Cell index {cell_index} out of range (0..{})",
                        cells.len()
                    )));
                }
                cells.remove(cell_index);
            }
            "replace" => {
                if cell_index >= cells.len() {
                    return Ok(ToolResult::error(format!(
                        "Cell index {cell_index} out of range (0..{})",
                        cells.len()
                    )));
                }
                let content = input["content"].as_str().unwrap_or("");
                let cell_type = input["cell_type"].as_str().unwrap_or("code");
                cells[cell_index] = make_cell(cell_type, content);
            }
            "insert" => {
                let insert_at = cell_index.min(cells.len());
                let content = input["content"].as_str().unwrap_or("");
                let cell_type = input["cell_type"].as_str().unwrap_or("code");
                cells.insert(insert_at, make_cell(cell_type, content));
            }
            other => {
                return Ok(ToolResult::error(format!("Unknown action: {other}")));
            }
        }

        let output = serde_json::to_string_pretty(&notebook).map_err(|e| {
            oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: format!("Failed to serialize notebook: {e}"),
            }
        })?;

        tokio::fs::write(&path, output)
            .await
            .map_err(|e| oxicode_common::OxiError::Tool {
                name: self.name().into(),
                message: format!("Failed to write {}: {e}", path.display()),
            })?;

        Ok(ToolResult::success(format!(
            "Notebook {action} at cell {cell_index} in {}",
            path.display()
        )))
    }
}

fn make_cell(cell_type: &str, content: &str) -> serde_json::Value {
    let source: Vec<String> = content.lines().map(|l| format!("{l}\n")).collect();
    serde_json::json!({
        "cell_type": cell_type,
        "metadata": {},
        "source": source,
        "outputs": if cell_type == "code" { serde_json::json!([]) } else { serde_json::json!(null) },
    })
}
