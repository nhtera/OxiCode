//! Integration tests for tool registry and tool schemas.

use oxicode_tools::{default_registry, ToolRegistry};

#[test]
fn test_default_registry_has_tools() {
    let registry = default_registry();
    assert!(!registry.is_empty(), "Default registry should have tools");
    assert!(
        registry.len() >= 10,
        "Should have at least 10 built-in tools"
    );
}

#[test]
fn test_registry_contains_core_tools() {
    let registry = default_registry();
    let names = registry.names();

    let expected = [
        "bash",
        "file_read",
        "file_write",
        "glob",
        "grep",
        "file_edit",
    ];
    for tool in &expected {
        assert!(
            names.iter().any(|n| n == tool),
            "Registry should contain tool: {tool}. Available: {names:?}"
        );
    }
}

#[test]
fn test_tool_schema_generation() {
    let registry = default_registry();
    let schemas = registry.schemas_json();

    assert!(!schemas.is_empty());
    for schema in &schemas {
        assert!(schema.get("name").is_some(), "Schema should have 'name'");
        assert!(
            schema.get("description").is_some(),
            "Schema should have 'description'"
        );
    }
}

#[test]
fn test_registry_get_by_name() {
    let registry = default_registry();

    assert!(registry.get("bash").is_some(), "Should find 'bash' tool");
    assert!(registry.get("nonexistent_tool").is_none());
}

#[test]
fn test_registry_list_returns_schemas() {
    let registry = default_registry();
    let list = registry.list();

    assert!(!list.is_empty());
    for schema in &list {
        assert!(!schema.name.is_empty());
        assert!(!schema.description.is_empty());
    }
}

#[test]
fn test_empty_registry() {
    let registry = ToolRegistry::new();
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);
    assert!(registry.names().is_empty());
    assert!(registry.get("bash").is_none());
}
