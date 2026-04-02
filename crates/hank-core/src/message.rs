use std::path::Path;

const MAX_RESULT_SIZE: usize = 50_000;
const PREVIEW_SIZE: usize = 2_000;
const CLEARED_PLACEHOLDER: &str = "[Old tool result content cleared]";

/// Format a tool result for the API.
pub fn format_tool_result(tool_use_id: &str, content: &str, is_error: bool) -> serde_json::Value {
    serde_json::json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": if is_error { format!("Error: {content}") } else { content.to_string() },
        "is_error": is_error,
    })
}

/// Persist large result to disk, return persisted-output wrapper.
pub fn persist_large_result(
    tool_use_id: &str,
    content: &str,
    working_dir: &Path,
) -> Option<String> {
    if content.len() <= MAX_RESULT_SIZE {
        return None;
    }
    let dir = working_dir.join(".claude").join("tool-results");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{tool_use_id}.txt"));
    let _ = std::fs::write(&path, content);

    let preview = if content.len() > PREVIEW_SIZE {
        &content[..PREVIEW_SIZE]
    } else {
        content
    };

    Some(format!(
        "<persisted-output>\nOutput too large ({} chars). Full output saved to: {}\n\n{preview}\n</persisted-output>",
        content.len(),
        path.display(),
    ))
}

/// Wrap content in system-reminder tags.
pub fn wrap_system_reminder(content: &str) -> String {
    format!("<system-reminder>\n{content}\n</system-reminder>")
}

/// Generate a nag reminder for task tracking.
pub fn nag_reminder(task_list: &str) -> String {
    wrap_system_reminder(&format!(
        "The task tools haven't been used recently. Consider using TaskCreate/TaskUpdate to track progress.\n\n{task_list}\n\nMake sure that you NEVER mention this reminder to the user"
    ))
}

/// Replace cleared tool results with placeholder.
pub fn cleared_result_placeholder() -> &'static str {
    CLEARED_PLACEHOLDER
}

/// Merge consecutive user messages in a message list.
pub fn merge_consecutive_user_messages(messages: &mut Vec<serde_json::Value>) {
    let mut i = 0;
    while i + 1 < messages.len() {
        let curr_role = messages[i].get("role").and_then(|v| v.as_str()).unwrap_or("");
        let next_role = messages[i + 1].get("role").and_then(|v| v.as_str()).unwrap_or("");
        if curr_role == "user" && next_role == "user" {
            let next = messages.remove(i + 1);
            if let Some(content) = next.get("content").and_then(|v| v.as_str()) {
                let curr_content = messages[i].get("content").and_then(|v| v.as_str()).unwrap_or("");
                messages[i]["content"] = serde_json::Value::String(
                    format!("{curr_content}\n\n{content}")
                );
            }
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("hank-{prefix}-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn format_tool_result_marks_errors() {
        let result = format_tool_result("tool-1", "boom", true);

        assert_eq!(result["tool_use_id"], "tool-1");
        assert_eq!(result["content"], "Error: boom");
        assert_eq!(result["is_error"], true);
    }

    #[test]
    fn persist_large_result_saves_full_output_and_returns_wrapper() {
        let dir = temp_dir("message-persist");
        let content = "a".repeat(MAX_RESULT_SIZE + 1);

        let wrapper = persist_large_result("tool-2", &content, &dir).unwrap();
        let saved_path = dir.join(".claude").join("tool-results").join("tool-2.txt");

        assert!(wrapper.contains("<persisted-output>"));
        assert!(wrapper.contains("Output too large"));
        assert!(wrapper.contains("tool-2.txt"));
        assert_eq!(fs::read_to_string(saved_path).unwrap(), content);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn merge_consecutive_user_messages_combines_adjacent_entries() {
        let mut messages = vec![
            json!({"role": "user", "content": "first"}),
            json!({"role": "user", "content": "second"}),
            json!({"role": "assistant", "content": "reply"}),
        ];

        merge_consecutive_user_messages(&mut messages);

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["content"], "first\n\nsecond");
    }

    #[test]
    fn wrap_system_reminder_adds_expected_tags() {
        let wrapped = wrap_system_reminder("remember this");
        assert_eq!(wrapped, "<system-reminder>\nremember this\n</system-reminder>");
    }

    #[test]
    fn nag_reminder_keeps_hidden_instruction() {
        let reminder = nag_reminder("- pending item");
        assert!(reminder.contains("TaskCreate/TaskUpdate"));
        assert!(reminder.contains("Make sure that you NEVER mention this reminder to the user"));
    }

    #[test]
    fn cleared_result_placeholder_is_stable() {
        assert_eq!(cleared_result_placeholder(), "[Old tool result content cleared]");
    }
}
