//! Integration tests for the agent orchestration subsystem.
//!
//! Tests cover: MessageBus concurrency, TeamManager lifecycle,
//! CoordinatorState agent tracking, AgentConfig type application,
//! and cross-crate team ↔ task ↔ messaging integration.
//!
//! No API key needed — pure in-process logic.
//! Run with: `cargo test -p oxicode-agents --test agent_orchestration_integration_tests`

use std::sync::Arc;

use oxicode_agents::built_in::AgentType;
use oxicode_agents::communication::{AgentMessage, MessageBus};
use oxicode_agents::coordinator::{
    filter_tools, filter_tools_by_whitelist, CoordinatorState, COORDINATOR_TOOLS,
};
use oxicode_agents::spawner::AgentConfig;
use oxicode_agents::team::TeamManager;

// ═══════════════════════════════════════════════════════════════════
// A. MessageBus — Concurrent Writer Stress
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_concurrent_writers_to_same_mailbox() {
    let bus = Arc::new(MessageBus::new());
    let mut handles = Vec::new();

    // 10 concurrent writers each send 10 messages to "target".
    for sender_id in 0..10 {
        let bus_clone = bus.clone();
        handles.push(tokio::spawn(async move {
            for msg_id in 0..10 {
                let msg = AgentMessage::new(
                    format!("sender-{sender_id}"),
                    "target",
                    format!("msg-{sender_id}-{msg_id}"),
                );
                bus_clone.send(msg).await;
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // All 100 messages should be delivered.
    let msgs = bus.receive("target").await;
    assert_eq!(msgs.len(), 100, "all concurrent messages should arrive");

    // Verify no duplicates — each id should be unique.
    let ids: std::collections::HashSet<_> = msgs.iter().map(|m| m.id.clone()).collect();
    assert_eq!(ids.len(), 100, "all message IDs should be unique");
}

#[tokio::test]
async fn test_concurrent_readers_and_writers() {
    let bus = Arc::new(MessageBus::new());

    // Writer: sends 50 messages to "agent-a".
    let bus_w = bus.clone();
    let writer = tokio::spawn(async move {
        for i in 0..50 {
            bus_w
                .send(AgentMessage::new("writer", "agent-a", format!("msg-{i}")))
                .await;
            // Tiny yield to interleave with reader.
            tokio::task::yield_now().await;
        }
    });

    // Peek reader: reads pending count in parallel.
    let bus_r = bus.clone();
    let reader = tokio::spawn(async move {
        let mut max_seen = 0usize;
        for _ in 0..100 {
            let count = bus_r.pending_count("agent-a").await;
            if count > max_seen {
                max_seen = count;
            }
            tokio::task::yield_now().await;
        }
        max_seen
    });

    writer.await.unwrap();
    let _max_seen = reader.await.unwrap();

    // After writer finishes, drain should get all messages.
    let msgs = bus.receive("agent-a").await;
    assert_eq!(msgs.len(), 50, "all messages should be present after drain");
}

#[tokio::test]
async fn test_multiple_recipients_isolated() {
    let bus = MessageBus::new();

    // Send interleaved messages to 3 different agents.
    for i in 0..30 {
        let to = match i % 3 {
            0 => "alpha",
            1 => "beta",
            _ => "gamma",
        };
        bus.send(AgentMessage::new("coordinator", to, format!("msg-{i}")))
            .await;
    }

    let alpha = bus.receive("alpha").await;
    let beta = bus.receive("beta").await;
    let gamma = bus.receive("gamma").await;

    assert_eq!(alpha.len(), 10);
    assert_eq!(beta.len(), 10);
    assert_eq!(gamma.len(), 10);

    // Messages should be correctly routed.
    for msg in &alpha {
        assert_eq!(msg.to, "alpha");
    }
    for msg in &beta {
        assert_eq!(msg.to, "beta");
    }
    for msg in &gamma {
        assert_eq!(msg.to, "gamma");
    }
}

// ═══════════════════════════════════════════════════════════════════
// B. TeamManager — Full Lifecycle
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_team_manager_create_and_get() {
    let mut mgr = TeamManager::new();
    mgr.create_team("project-alpha").unwrap();

    let team = mgr.get_team("project-alpha");
    assert!(team.is_some());
    assert_eq!(team.unwrap().name, "project-alpha");
}

#[test]
fn test_team_manager_multiple_teams_isolated() {
    let mut mgr = TeamManager::new();
    mgr.create_team("team-a").unwrap();
    mgr.create_team("team-b").unwrap();

    let teams = mgr.list_teams();
    assert_eq!(teams.len(), 2);
    assert!(teams.contains(&"team-a".to_string()));
    assert!(teams.contains(&"team-b".to_string()));
}

#[test]
fn test_team_delete_cleanup() {
    let mut mgr = TeamManager::new();
    mgr.create_team("ephemeral").unwrap();
    assert!(mgr.get_team("ephemeral").is_some());

    mgr.delete_team("ephemeral").unwrap();
    assert!(mgr.get_team("ephemeral").is_none());
    assert!(!mgr.list_teams().contains(&"ephemeral".to_string()));
}

#[test]
fn test_team_message_bus_per_team() {
    let mut mgr = TeamManager::new();
    mgr.create_team("team-x").unwrap();
    mgr.create_team("team-y").unwrap();

    // Each team should have its own bus (Arc pointer comparison).
    let team_x = mgr.get_team("team-x").unwrap();
    let team_y = mgr.get_team("team-y").unwrap();

    let ptr_x = Arc::as_ptr(&team_x.bus);
    let ptr_y = Arc::as_ptr(&team_y.bus);
    assert_ne!(ptr_x, ptr_y, "teams should have separate buses");
}

#[tokio::test]
async fn test_team_send_to_nonexistent_agent_errors() {
    let mut mgr = TeamManager::new();
    mgr.create_team("team-z").unwrap();

    let result = mgr.send_to_agent("team-z", "ghost-agent", "hello").await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not found"),
        "should mention agent not found, got: {err}"
    );
}

// ═══════════════════════════════════════════════════════════════════
// C. CoordinatorState — Agent Lifecycle
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_coordinator_state_add_and_list() {
    // We can't create real AgentHandles without spawning processes,
    // so test the filter and status functions independently.
    let state = CoordinatorState::new();
    assert!(state.list_agents().is_empty());
}

#[test]
fn test_coordinator_tool_filtering() {
    let all_tools = vec![
        "bash".to_string(),
        "file_read".to_string(),
        "team_create".to_string(),
        "team_delete".to_string(),
        "send_message".to_string(),
        "output".to_string(),
        "file_write".to_string(),
    ];

    let filtered = filter_tools(&all_tools);
    assert_eq!(filtered.len(), 4);
    assert!(filtered.contains(&"team_create".to_string()));
    assert!(filtered.contains(&"team_delete".to_string()));
    assert!(filtered.contains(&"send_message".to_string()));
    assert!(filtered.contains(&"output".to_string()));
    assert!(!filtered.contains(&"bash".to_string()));
    assert!(!filtered.contains(&"file_read".to_string()));
}

#[test]
fn test_coordinator_whitelist_filtering() {
    let tools = vec![
        "bash".to_string(),
        "glob".to_string(),
        "grep".to_string(),
        "file_write".to_string(),
    ];
    let whitelist = vec![
        "glob".to_string(),
        "grep".to_string(),
        "read_file".to_string(),
    ];

    let filtered = filter_tools_by_whitelist(&tools, &whitelist);
    assert_eq!(filtered.len(), 2);
    assert!(filtered.contains(&"glob".to_string()));
    assert!(filtered.contains(&"grep".to_string()));
    assert!(!filtered.contains(&"bash".to_string()));
}

#[test]
fn test_coordinator_tools_constant_complete() {
    // Coordinator tools should be exactly 4.
    assert_eq!(COORDINATOR_TOOLS.len(), 4);
    assert!(COORDINATOR_TOOLS.contains(&"team_create"));
    assert!(COORDINATOR_TOOLS.contains(&"team_delete"));
    assert!(COORDINATOR_TOOLS.contains(&"send_message"));
    assert!(COORDINATOR_TOOLS.contains(&"output"));
}

// ═══════════════════════════════════════════════════════════════════
// D. AgentConfig — Type Application Logic
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_agent_config_apply_all_types() {
    // Verify every agent type correctly sets prompt, model, and tools.
    for agent_type in AgentType::all() {
        let mut cfg = AgentConfig {
            prompt: "original task".to_string(),
            agent_type: Some(*agent_type),
            ..Default::default()
        };
        cfg.apply_agent_type();

        // System prompt should be prepended.
        assert!(
            cfg.prompt.contains("original task"),
            "{agent_type}: original prompt should be preserved"
        );
        assert!(
            cfg.prompt.len() > "original task".len(),
            "{agent_type}: system prompt should be prepended"
        );

        // Model should be set (not empty).
        assert!(
            !cfg.model.is_empty(),
            "{agent_type}: model should be set after apply"
        );

        // Tool whitelist: General = None, others = Some.
        if *agent_type == AgentType::General {
            assert!(
                cfg.allowed_tools.is_none(),
                "General should have no tool restrictions"
            );
        } else {
            assert!(
                cfg.allowed_tools.is_some(),
                "{agent_type}: should have tool restrictions"
            );
            assert!(
                !cfg.allowed_tools.as_ref().unwrap().is_empty(),
                "{agent_type}: tool whitelist should not be empty"
            );
        }
    }
}

#[test]
fn test_agent_config_model_override_respected() {
    // When model_override=true, agent type should NOT change the model.
    let custom_model = "custom-model-v2";
    let mut cfg = AgentConfig {
        prompt: "plan this".to_string(),
        model: custom_model.to_string(),
        model_override: true,
        agent_type: Some(AgentType::Plan),
        ..Default::default()
    };
    cfg.apply_agent_type();

    assert_eq!(
        cfg.model, custom_model,
        "model_override should preserve custom model"
    );
}

#[test]
fn test_agent_config_no_type_is_noop() {
    let original_prompt = "do something".to_string();
    let original_model = "claude-sonnet-4-20250514".to_string();
    let mut cfg = AgentConfig {
        prompt: original_prompt.clone(),
        model: original_model.clone(),
        agent_type: None,
        ..Default::default()
    };
    cfg.apply_agent_type();

    assert_eq!(cfg.prompt, original_prompt, "no type = no prompt change");
    assert_eq!(cfg.model, original_model, "no type = no model change");
    assert!(cfg.allowed_tools.is_none(), "no type = no tool whitelist");
}

#[test]
fn test_agent_config_explicit_tools_not_overridden() {
    // When allowed_tools is already set, agent type should not override it.
    let custom_tools = vec!["bash".to_string(), "file_read".to_string()];
    let mut cfg = AgentConfig {
        prompt: "explore".to_string(),
        agent_type: Some(AgentType::Explore),
        allowed_tools: Some(custom_tools.clone()),
        ..Default::default()
    };
    cfg.apply_agent_type();

    assert_eq!(
        cfg.allowed_tools.as_ref().unwrap(),
        &custom_tools,
        "explicit tools should not be overridden by agent type"
    );
}

#[test]
fn test_agent_config_serde_all_types() {
    // Verify all agent types survive JSON serialization roundtrip.
    for agent_type in AgentType::all() {
        let cfg = AgentConfig {
            name: format!("agent-{agent_type}"),
            prompt: "test".to_string(),
            agent_type: Some(*agent_type),
            ..Default::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: AgentConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed.agent_type,
            Some(*agent_type),
            "serde roundtrip failed for {agent_type}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// E. Agent Type — Tool Safety Invariants
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_plan_agent_cannot_execute_shell() {
    let tools = AgentType::Plan.allowed_tools().unwrap();
    assert!(
        !tools.contains(&"bash"),
        "Plan agent must NOT have bash access"
    );
}

#[test]
fn test_explore_agent_cannot_write_files() {
    let tools = AgentType::Explore.allowed_tools().unwrap();
    assert!(
        !tools.contains(&"file_write"),
        "Explore agent must NOT write files"
    );
    assert!(
        !tools.contains(&"file_edit"),
        "Explore agent must NOT edit files"
    );
}

#[test]
fn test_guide_agent_is_read_only() {
    let tools = AgentType::Guide.allowed_tools().unwrap();
    assert!(!tools.contains(&"file_write"), "Guide: no write");
    assert!(!tools.contains(&"file_edit"), "Guide: no edit");
    assert!(!tools.contains(&"bash"), "Guide: no bash");
    // Should have read-only tools.
    assert!(tools.contains(&"read_file"), "Guide: should read files");
    assert!(tools.contains(&"glob"), "Guide: should glob");
    assert!(tools.contains(&"grep"), "Guide: should grep");
}

#[test]
fn test_statusline_agent_minimal_tools() {
    let tools = AgentType::Statusline.allowed_tools().unwrap();
    assert_eq!(
        tools.len(),
        3,
        "Statusline should have exactly 3 tools: read, write, edit"
    );
    assert!(tools.contains(&"read_file"));
    assert!(tools.contains(&"file_write"));
    assert!(tools.contains(&"file_edit"));
}

#[test]
fn test_verify_agent_has_bash_and_write() {
    let tools = AgentType::Verify.allowed_tools().unwrap();
    assert!(
        tools.contains(&"bash"),
        "Verify: should have bash for tests"
    );
    assert!(
        tools.contains(&"file_write"),
        "Verify: should write results"
    );
}

// ═══════════════════════════════════════════════════════════════════
// F. Agent Type — Model Assignment
// ═══════════════════════════════════════════════════════════════════

#[test]
fn test_lightweight_agents_use_haiku() {
    // Explore, Guide, Statusline should use haiku (cheaper model).
    for at in &[AgentType::Explore, AgentType::Guide, AgentType::Statusline] {
        let model = at.default_model();
        assert!(
            model.contains("haiku"),
            "{at}: lightweight agent should use haiku, got: {model}"
        );
    }
}

#[test]
fn test_heavy_agents_use_sonnet() {
    // Plan, Verify, General should use sonnet (more capable model).
    for at in &[AgentType::Plan, AgentType::Verify, AgentType::General] {
        let model = at.default_model();
        assert!(
            model.contains("sonnet"),
            "{at}: heavy agent should use sonnet, got: {model}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// G. MessageBus — Edge Cases
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_receive_from_nonexistent_mailbox() {
    let bus = MessageBus::new();
    let msgs = bus.receive("nonexistent").await;
    assert!(msgs.is_empty(), "nonexistent mailbox should return empty");
}

#[tokio::test]
async fn test_peek_from_nonexistent_mailbox() {
    let bus = MessageBus::new();
    let msgs = bus.peek("nonexistent").await;
    assert!(msgs.is_empty(), "peek on nonexistent should return empty");
}

#[tokio::test]
async fn test_empty_content_message() {
    let bus = MessageBus::new();
    bus.send(AgentMessage::new("a", "b", "")).await;

    let msgs = bus.receive("b").await;
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content, "");
}

#[tokio::test]
async fn test_message_metadata() {
    let bus = MessageBus::new();
    let msg = AgentMessage::new("sender", "receiver", "test content");
    let original_id = msg.id.clone();
    let original_ts = msg.timestamp;

    bus.send(msg).await;
    let received = bus.receive("receiver").await;

    assert_eq!(
        received[0].id, original_id,
        "message ID should be preserved"
    );
    assert_eq!(received[0].from, "sender");
    assert_eq!(received[0].to, "receiver");
    assert_eq!(received[0].timestamp, original_ts);
}

#[tokio::test]
async fn test_large_message_payload() {
    let bus = MessageBus::new();
    let large_content = "x".repeat(100_000);
    bus.send(AgentMessage::new("a", "b", &large_content)).await;

    let msgs = bus.receive("b").await;
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content.len(), 100_000);
}
