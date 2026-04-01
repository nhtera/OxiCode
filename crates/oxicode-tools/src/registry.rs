use std::collections::HashMap;

use oxicode_common::{OxiError, OxiResult};

use crate::tool_trait::{Tool, ToolContext, ToolResult, ToolSchema};

/// Registry that holds all available tools and dispatches execution.
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool. Overwrites if name already exists.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Get a tool reference by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(AsRef::as_ref)
    }

    /// List all registered tool schemas.
    pub fn list(&self) -> Vec<ToolSchema> {
        self.tools.values().map(|t| t.schema()).collect()
    }

    /// List tool names.
    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Return tool schemas as JSON array for the LLM API `tools` parameter.
    pub fn schemas_json(&self) -> Vec<serde_json::Value> {
        self.tools
            .values()
            .map(|t| {
                let schema = t.schema();
                serde_json::json!({
                    "name": schema.name,
                    "description": schema.description,
                    "input_schema": schema.input_schema,
                })
            })
            .collect()
    }

    /// Execute a tool by name.
    pub async fn execute(
        &self,
        name: &str,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> OxiResult<ToolResult> {
        let tool = self.tools.get(name).ok_or_else(|| OxiError::Tool {
            name: name.to_string(),
            message: format!("Tool '{name}' not found"),
        })?;

        tool.execute(input, ctx).await
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_trait::PermissionLevel;
    use async_trait::async_trait;

    struct DummyTool;

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            "dummy"
        }
        fn description(&self) -> &str {
            "A dummy tool"
        }
        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "dummy".into(),
                description: "A dummy tool".into(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            }
        }
        fn permission_level(&self) -> PermissionLevel {
            PermissionLevel::ReadOnly
        }
        async fn execute(
            &self,
            _input: serde_json::Value,
            _ctx: &ToolContext,
        ) -> OxiResult<ToolResult> {
            Ok(ToolResult::success("ok"))
        }
    }

    #[test]
    fn test_register_and_get() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(DummyTool));
        assert_eq!(reg.len(), 1);
        assert!(reg.get("dummy").is_some());
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn test_list_schemas() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(DummyTool));
        let schemas = reg.list();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name, "dummy");
    }

    #[test]
    fn test_schemas_json() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(DummyTool));
        let json = reg.schemas_json();
        assert_eq!(json.len(), 1);
        assert_eq!(json[0]["name"], "dummy");
    }

    #[tokio::test]
    async fn test_execute_found() {
        let mut reg = ToolRegistry::new();
        reg.register(Box::new(DummyTool));
        let result = reg
            .execute("dummy", serde_json::json!({}), &ToolContext::default())
            .await
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content, "ok");
    }

    #[tokio::test]
    async fn test_execute_not_found() {
        let reg = ToolRegistry::new();
        let result = reg
            .execute("missing", serde_json::json!({}), &ToolContext::default())
            .await;
        assert!(result.is_err());
    }
}
