use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimMode {
    Normal,
    Insert,
    Visual,
    VisualLine,
    Command,
}

impl VimMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
            Self::VisualLine => "VISUAL LINE",
            Self::Command => "COMMAND",
        }
    }

    pub fn is_visual(&self) -> bool {
        matches!(self, Self::Visual | Self::VisualLine)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimOperator {
    Delete,
    Yank,
    Change,
    Indent,
    Outdent,
}

#[derive(Debug, Clone)]
pub struct VimState {
    pub enabled: bool,
    pub mode: VimMode,
    pub count: String,
    pub operator: Option<VimOperator>,
    pub operator_count: usize,
    pub pending_g: bool,
    pub pending_replace: bool,
    pub register: Option<(String, bool)>,
    pub command_line: Option<String>,
    pub status_message: Option<(String, Instant)>,
    pub last_search: Option<String>,
}

impl Default for VimState {
    fn default() -> Self {
        Self::new(false)
    }
}

impl VimState {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            mode: VimMode::Normal,
            count: String::new(),
            operator: None,
            operator_count: 1,
            pending_g: false,
            pending_replace: false,
            register: None,
            command_line: None,
            status_message: None,
            last_search: None,
        }
    }

    pub fn reset_to_normal(&mut self) {
        self.mode = VimMode::Normal;
        self.count.clear();
        self.operator = None;
        self.operator_count = 1;
        self.pending_g = false;
        self.pending_replace = false;
        self.command_line = None;
    }

    pub fn take_count(&mut self) -> usize {
        let count = self.count.parse::<usize>().unwrap_or(1).max(1);
        self.count.clear();
        count
    }

    pub fn peek_count(&self) -> usize {
        self.count.parse::<usize>().unwrap_or(1).max(1)
    }

    pub fn status(&mut self) -> Option<String> {
        if let Some((message, at)) = &self.status_message {
            if at.elapsed().as_secs() < 5 {
                return Some(message.clone());
            }
            self.status_message = None;
        }
        None
    }

    pub fn set_status<S: Into<String>>(&mut self, message: S) {
        self.status_message = Some((message.into(), Instant::now()));
    }

    pub fn indicator(&self) -> String {
        let mut text = self.mode.label().to_string();
        if !self.count.is_empty() {
            text.push(' ');
            text.push_str(&self.count);
        }
        if let Some(operator) = self.operator {
            text.push(' ');
            text.push_str(match operator {
                VimOperator::Delete => "d",
                VimOperator::Yank => "y",
                VimOperator::Change => "c",
                VimOperator::Indent => ">",
                VimOperator::Outdent => "<",
            });
        }
        if let Some(command) = &self.command_line {
            text = format!(":{command}");
        }
        text
    }
}
