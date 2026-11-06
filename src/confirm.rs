use winit::keyboard::{Key, NamedKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmChoice {
    Save,
    SaveAll,
    Discard,
    DiscardAll,
    ReplaceAll,
    Cancel,
}

impl ConfirmChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Save => "Save",
            Self::SaveAll => "Save All",
            Self::Discard => "Discard",
            Self::DiscardAll => "Discard All",
            Self::ReplaceAll => "Replace All",
            Self::Cancel => "Cancel",
        }
    }

    pub fn is_destructive(self) -> bool {
        matches!(self, Self::Discard | Self::DiscardAll)
    }
}

#[derive(Debug, Clone)]
pub struct ConfirmDialog {
    pub title: String,
    pub message: String,
    pub files: Vec<String>,
    pub choices: Vec<ConfirmChoice>,
    pub selected: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConfirmLayout {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub buttons: Vec<(ConfirmChoice, f32, f32, f32, f32)>,
}

impl ConfirmDialog {
    pub fn close_document(files: Vec<String>, can_save: bool) -> Self {
        let mut choices = Vec::new();
        if can_save {
            choices.push(ConfirmChoice::Save);
        }
        choices.push(ConfirmChoice::Discard);
        choices.push(ConfirmChoice::Cancel);

        Self {
            title: "Unsaved changes".to_string(),
            message: if can_save {
                "Save this file before closing?".to_string()
            } else {
                "This untitled file cannot be saved automatically. Discard it?".to_string()
            },
            files,
            choices,
            selected: 0,
            error: None,
        }
    }

    pub fn replace_all(count: usize) -> Self {
        Self {
            title: "Replace all matches".to_string(),
            message: format!(
                "Replace {count} occurrence{} in the workspace? This writes files to disk.",
                if count == 1 { "" } else { "s" }
            ),
            files: Vec::new(),
            choices: vec![ConfirmChoice::ReplaceAll, ConfirmChoice::Cancel],
            selected: 0,
            error: None,
        }
    }

    pub fn quit(files: Vec<String>, can_save_all: bool) -> Self {
        let mut choices = Vec::new();
        if can_save_all {
            choices.push(ConfirmChoice::SaveAll);
        }
        choices.push(ConfirmChoice::DiscardAll);
        choices.push(ConfirmChoice::Cancel);

        Self {
            title: "Unsaved workspace".to_string(),
            message: if can_save_all {
                "You have unsaved changes. Save before exiting?".to_string()
            } else {
                "Some untitled files cannot be saved automatically. Exit and discard them?"
                    .to_string()
            },
            files,
            choices,
            selected: 0,
            error: None,
        }
    }

    pub fn selected_choice(&self) -> ConfirmChoice {
        self.choices
            .get(self.selected)
            .copied()
            .unwrap_or(ConfirmChoice::Cancel)
    }

    pub fn set_error<S: Into<String>>(&mut self, message: S) {
        self.error = Some(message.into());
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.choices.is_empty() {
            return;
        }
        let len = self.choices.len() as isize;
        self.selected = ((self.selected as isize + delta).rem_euclid(len)) as usize;
    }

    pub fn handle_key(&mut self, key: &Key, ctrl: bool) -> Option<ConfirmChoice> {
        if ctrl {
            return None;
        }
        match key {
            Key::Named(NamedKey::Escape) => return Some(ConfirmChoice::Cancel),
            Key::Named(NamedKey::ArrowLeft)
            | Key::Named(NamedKey::ArrowUp)
            | Key::Named(NamedKey::Tab) => {
                self.move_selection(-1);
                return None;
            }
            Key::Named(NamedKey::ArrowRight) | Key::Named(NamedKey::ArrowDown) => {
                self.move_selection(1);
                return None;
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                return Some(self.selected_choice());
            }
            Key::Character(text) => {
                let lower = text.to_ascii_lowercase();
                match lower.as_str() {
                    "s" => {
                        if self.choices.contains(&ConfirmChoice::Save) {
                            return Some(ConfirmChoice::Save);
                        }
                        if self.choices.contains(&ConfirmChoice::SaveAll) {
                            return Some(ConfirmChoice::SaveAll);
                        }
                    }
                    "d" => {
                        if self.choices.contains(&ConfirmChoice::Discard) {
                            return Some(ConfirmChoice::Discard);
                        }
                        if self.choices.contains(&ConfirmChoice::DiscardAll) {
                            return Some(ConfirmChoice::DiscardAll);
                        }
                    }
                    "r" => {
                        if self.choices.contains(&ConfirmChoice::ReplaceAll) {
                            return Some(ConfirmChoice::ReplaceAll);
                        }
                    }
                    "c" => return Some(ConfirmChoice::Cancel),
                    _ => {}
                }
            }
            _ => {}
        }
        None
    }

    pub fn layout(
        &self,
        win_w: f32,
        win_h: f32,
        cell_w: f32,
        cell_h: f32,
        scale_factor: f32,
        status_h: f32,
    ) -> ConfirmLayout {
        let line_h = cell_h.max(10.0);
        let sf = scale_factor.max(0.5);
        let w = (600.0 * sf).min(win_w - 40.0).max(280.0);
        let file_rows = self.files.len().min(8) as f32;
        let error_rows = if self.error.is_some() { 1.0 } else { 0.0 };
        let h = (line_h * 5.2 + file_rows * line_h + error_rows * line_h)
            .min((win_h - status_h - 40.0).max(line_h * 6.0));
        let x = ((win_w - w) * 0.5).round();
        let y = (((win_h - status_h) - h) * 0.5).round().max(10.0);

        let btn_h = (line_h + 8.0 * sf).round();
        let gap = (10.0 * sf).round();
        let count = self.choices.len().max(1) as f32;
        let btn_w = ((w - 28.0 * sf - gap * (count - 1.0)) / count)
            .floor()
            .max(54.0);
        let total_w = btn_w * count + gap * (count - 1.0);
        let start_x = x + ((w - total_w) * 0.5).round();
        let btn_y = y + h - btn_h - 14.0 * sf;

        let buttons = self
            .choices
            .iter()
            .enumerate()
            .map(|(index, choice)| {
                (
                    *choice,
                    start_x + index as f32 * (btn_w + gap),
                    btn_y,
                    btn_w,
                    btn_h,
                )
            })
            .collect();

        let _ = cell_w;
        ConfirmLayout {
            x,
            y,
            w,
            h,
            buttons,
        }
    }

    pub fn hit_test(&self, layout: &ConfirmLayout, px: f32, py: f32) -> Option<ConfirmChoice> {
        for (choice, x, y, w, h) in &layout.buttons {
            if px >= *x && px <= *x + *w && py >= *y && py <= *y + *h {
                return Some(*choice);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_dialog_choices_respect_saveability() {
        let saveable = ConfirmDialog::close_document(vec!["a.rs".into()], true);
        assert_eq!(saveable.selected_choice(), ConfirmChoice::Save);
        assert!(saveable.choices.contains(&ConfirmChoice::Save));

        let untitled = ConfirmDialog::close_document(vec!["[Untitled]".into()], false);
        assert!(!untitled.choices.contains(&ConfirmChoice::Save));
        assert_eq!(untitled.selected_choice(), ConfirmChoice::Discard);
    }

    #[test]
    fn quit_dialog_uses_save_all_and_cancel() {
        let dialog = ConfirmDialog::quit(vec!["a.rs".into()], true);
        assert_eq!(dialog.choices[0], ConfirmChoice::SaveAll);
        assert_eq!(dialog.choices.last().copied(), Some(ConfirmChoice::Cancel));
    }

    #[test]
    fn keyboard_navigation_wraps() {
        let mut dialog = ConfirmDialog::close_document(vec!["a".into()], true);
        assert_eq!(dialog.selected_choice(), ConfirmChoice::Save);
        dialog.handle_key(&Key::Named(NamedKey::ArrowLeft), false);
        assert_eq!(dialog.selected_choice(), ConfirmChoice::Cancel);
        dialog.handle_key(&Key::Named(NamedKey::ArrowRight), false);
        assert_eq!(dialog.selected_choice(), ConfirmChoice::Save);
        assert_eq!(
            dialog.handle_key(&Key::Named(NamedKey::Escape), false),
            Some(ConfirmChoice::Cancel)
        );
    }
}
