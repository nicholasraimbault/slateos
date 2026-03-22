// In-memory conversation model.
//
// Stores the chat history between the user and the Rhea assistant.
// Persistence is handled by Rhea; this is purely the local
// view shown in the panel UI.

use std::time::SystemTime;

/// The role of a chat message sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
}

/// A single message in the conversation.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub timestamp: SystemTime,
}

/// A conversation (ordered list of messages).
#[derive(Debug, Clone, Default)]
pub struct Conversation {
    messages: Vec<ChatMessage>,
}

impl Conversation {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    /// Append a user message.
    pub fn add_user_message(&mut self, content: String) {
        self.messages.push(ChatMessage {
            role: ChatRole::User,
            content,
            timestamp: SystemTime::now(),
        });
    }

    /// Append an assistant message.
    pub fn add_assistant_message(&mut self, content: String) {
        self.messages.push(ChatMessage {
            role: ChatRole::Assistant,
            content,
            timestamp: SystemTime::now(),
        });
    }

    /// Append text to the last assistant message. If the most recent message
    /// is not from the assistant, a new assistant message is created.
    pub fn append_to_assistant(&mut self, chunk: &str) {
        if let Some(last) = self.messages.last_mut() {
            if last.role == ChatRole::Assistant {
                last.content.push_str(chunk);
                return;
            }
        }
        // No existing assistant message to append to.
        self.add_assistant_message(chunk.to_string());
    }

    /// Remove all messages.
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Read-only view of all messages.
    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Number of messages in the conversation.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Whether the conversation is empty.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

/// Detect fenced code blocks (triple-backtick) in markdown content.
///
/// Returns a list of `(language, code)` tuples for each code block found.
pub fn extract_code_blocks(content: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut language = String::new();
    let mut code = String::new();

    for line in content.lines() {
        if !in_block && line.trim_start().starts_with("```") {
            in_block = true;
            language = line.trim_start().trim_start_matches('`').trim().to_string();
            code.clear();
        } else if in_block && line.trim_start().starts_with("```") {
            in_block = false;
            blocks.push((language.clone(), code.clone()));
        } else if in_block {
            if !code.is_empty() {
                code.push('\n');
            }
            code.push_str(line);
        }
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_messages_preserves_order() {
        let mut conv = Conversation::new();
        conv.add_user_message("Hello".to_string());
        conv.add_assistant_message("Hi there!".to_string());
        conv.add_user_message("How are you?".to_string());

        assert_eq!(conv.len(), 3);
        assert_eq!(conv.messages()[0].role, ChatRole::User);
        assert_eq!(conv.messages()[0].content, "Hello");
        assert_eq!(conv.messages()[1].role, ChatRole::Assistant);
        assert_eq!(conv.messages()[1].content, "Hi there!");
        assert_eq!(conv.messages()[2].role, ChatRole::User);
        assert_eq!(conv.messages()[2].content, "How are you?");
    }

    #[test]
    fn clear_empties_conversation() {
        let mut conv = Conversation::new();
        conv.add_user_message("Test".to_string());
        conv.add_assistant_message("Response".to_string());
        assert_eq!(conv.len(), 2);

        conv.clear();
        assert!(conv.is_empty());
        assert_eq!(conv.len(), 0);
    }

    #[test]
    fn append_to_assistant_extends_last_message() {
        let mut conv = Conversation::new();
        conv.add_user_message("Question".to_string());
        conv.add_assistant_message("Part 1".to_string());
        conv.append_to_assistant(" Part 2");

        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages()[1].content, "Part 1 Part 2");
    }

    #[test]
    fn append_to_assistant_creates_new_when_last_is_user() {
        let mut conv = Conversation::new();
        conv.add_user_message("Question".to_string());
        conv.append_to_assistant("Answer start");

        assert_eq!(conv.len(), 2);
        assert_eq!(conv.messages()[1].role, ChatRole::Assistant);
        assert_eq!(conv.messages()[1].content, "Answer start");
    }

    #[test]
    fn append_to_assistant_on_empty_conversation() {
        let mut conv = Conversation::new();
        conv.append_to_assistant("Unsolicited");

        assert_eq!(conv.len(), 1);
        assert_eq!(conv.messages()[0].role, ChatRole::Assistant);
    }

    #[test]
    fn default_conversation_is_empty() {
        let conv = Conversation::default();
        assert!(conv.is_empty());
    }

    #[test]
    fn messages_have_timestamps() {
        let before = SystemTime::now();
        let mut conv = Conversation::new();
        conv.add_user_message("Timed".to_string());
        let after = SystemTime::now();

        let ts = conv.messages()[0].timestamp;
        assert!(ts >= before);
        assert!(ts <= after);
    }

    #[test]
    fn extract_code_blocks_finds_fenced_blocks() {
        let content = "Here is some code:\n```rust\nfn main() {}\n```\nAnd more text.";
        let blocks = extract_code_blocks(content);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].0, "rust");
        assert_eq!(blocks[0].1, "fn main() {}");
    }

    #[test]
    fn extract_code_blocks_handles_multiple() {
        let content = "```python\nprint('hi')\n```\ntext\n```\nplain\n```";
        let blocks = extract_code_blocks(content);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].0, "python");
        assert_eq!(blocks[0].1, "print('hi')");
        assert_eq!(blocks[1].0, "");
        assert_eq!(blocks[1].1, "plain");
    }

    #[test]
    fn extract_code_blocks_returns_empty_when_none() {
        let blocks = extract_code_blocks("No code blocks here.");
        assert!(blocks.is_empty());
    }
}
