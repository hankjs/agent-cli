use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::permission::PermissionDecision;
use crate::streaming::Message;

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub data: Value,
    pub new_messages: Option<Vec<Message>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Validation error: {0}")]
    ValidationError(String),
    #[error("Execution error: {0}")]
    ExecutionError(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Timeout")]
    Timeout,
}

pub struct ToolContext {
    pub working_dir: std::path::PathBuf,
    pub abort: tokio::sync::watch::Receiver<bool>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;
    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, ToolError>;

    fn format_result(&self, result: &ToolResult) -> String {
        result.data.to_string()
    }
    fn is_concurrency_safe(&self, _input: &Value) -> bool { false }
    fn is_read_only(&self, _input: &Value) -> bool { false }
    fn validate_input(&self, _input: &Value) -> Result<(), ToolError> { Ok(()) }
    fn check_permissions(&self, _input: &Value) -> PermissionDecision {
        PermissionDecision::Allow
    }
    fn prompt(&self) -> &str { "" }
}

/// Registry for tool registration and lookup.
pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
    by_name: HashMap<String, usize>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new(), by_name: HashMap::new() }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        if !self.by_name.contains_key(&name) {
            let idx = self.tools.len();
            self.by_name.insert(name, idx);
            self.tools.push(tool);
        }
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.by_name.get(name).map(|&i| self.tools[i].clone())
    }

    pub fn api_definitions(&self) -> Vec<Value> {
        self.tools.iter().map(|t| {
            serde_json::json!({
                "name": t.name(),
                "description": t.description(),
                "input_schema": t.input_schema(),
            })
        }).collect()
    }

    /// Merge new tools, skipping name collisions (existing tools take precedence).
    pub fn merge(&mut self, new_tools: Vec<Arc<dyn Tool>>) {
        for tool in new_tools {
            self.register(tool);
        }
    }

    pub fn all_tools(&self) -> &[Arc<dyn Tool>] {
        &self.tools
    }
}

/// Executes tool calls with concurrency control.
pub struct ToolExecutor;

impl ToolExecutor {
    pub async fn execute(
        registry: &ToolRegistry,
        calls: Vec<(String, String, Value)>, // (tool_use_id, tool_name, input)
        ctx: &ToolContext,
    ) -> Vec<(String, Result<ToolResult, ToolError>)> {
        let all_concurrent = calls.iter().all(|(_, name, input)| {
            registry.get(name).map_or(false, |t| t.is_concurrency_safe(input))
        });

        if all_concurrent && calls.len() > 1 {
            let futs: Vec<_> = calls.into_iter().map(|(id, name, input)| {
                let tool = registry.get(&name);
                async move {
                    match tool {
                        Some(t) => (id, t.call(input, ctx).await),
                        None => (id, Err(ToolError::ExecutionError(format!("Unknown tool: {name}")))),
                    }
                }
            }).collect();
            futures_util::future::join_all(futs).await
        } else {
            let mut results = Vec::new();
            for (id, name, input) in calls {
                let result = match registry.get(&name) {
                    Some(t) => t.call(input, ctx).await,
                    None => Err(ToolError::ExecutionError(format!("Unknown tool: {name}"))),
                };
                results.push((id, result));
            }
            results
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use tokio::sync::watch;

    struct MockTool {
        name: &'static str,
        concurrent: bool,
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str { self.name }
        fn description(&self) -> &str { "mock tool" }
        fn input_schema(&self) -> Value { json!({"type": "object"}) }
        fn is_concurrency_safe(&self, _input: &Value) -> bool { self.concurrent }

        async fn call(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, ToolError> {
            Ok(ToolResult {
                data: input.get("value").cloned().unwrap_or(Value::Null),
                new_messages: None,
            })
        }
    }

    fn context() -> ToolContext {
        let (_tx, rx) = watch::channel(false);
        ToolContext {
            working_dir: PathBuf::from("."),
            abort: rx,
        }
    }

    #[tokio::test]
    async fn registry_registers_and_executes_tool_calls() {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool { name: "echo", concurrent: true }));

        let defs = registry.api_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0]["name"], "echo");
        assert!(registry.get("echo").is_some());

        let results = ToolExecutor::execute(
            &registry,
            vec![("tool-1".into(), "echo".into(), json!({"value": "hello"}))],
            &context(),
        )
        .await;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "tool-1");
        assert_eq!(results[0].1.as_ref().unwrap().data, json!("hello"));
    }

    #[tokio::test]
    async fn executor_reports_unknown_tools() {
        let registry = ToolRegistry::new();
        let results = ToolExecutor::execute(
            &registry,
            vec![("tool-2".into(), "missing".into(), json!({}))],
            &context(),
        )
        .await;

        assert_eq!(results.len(), 1);
        match &results[0].1 {
            Err(ToolError::ExecutionError(message)) => {
                assert!(message.contains("Unknown tool: missing"));
            }
            other => panic!("expected unknown tool error, got {other:?}"),
        }
    }
}
