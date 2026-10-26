//! Presentation-only contract for the native desktop menu. No file paths or writes.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum DesktopLanguageDto {
    #[serde(rename = "en")]
    #[default]
    English,
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DesktopCommandDto {
    CopyToOther,
    MoveToOther,
    Copy,
    Cut,
    Paste,
    Rename,
    NewFolder,
    Trash,
    Refresh,
    ToggleHidden,
    Diagnostics,
    Uninstall,
    OperationLog,
    LanguageEnglish,
    LanguageChinese,
}

impl DesktopCommandDto {
    pub const ALL: [Self; 15] = [
        Self::CopyToOther,
        Self::MoveToOther,
        Self::Copy,
        Self::Cut,
        Self::Paste,
        Self::Rename,
        Self::NewFolder,
        Self::Trash,
        Self::Refresh,
        Self::ToggleHidden,
        Self::Diagnostics,
        Self::Uninstall,
        Self::OperationLog,
        Self::LanguageEnglish,
        Self::LanguageChinese,
    ];

    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::CopyToOther => "copyToOther",
            Self::MoveToOther => "moveToOther",
            Self::Copy => "copy",
            Self::Cut => "cut",
            Self::Paste => "paste",
            Self::Rename => "rename",
            Self::NewFolder => "newFolder",
            Self::Trash => "trash",
            Self::Refresh => "refresh",
            Self::ToggleHidden => "toggleHidden",
            Self::Diagnostics => "diagnostics",
            Self::Uninstall => "uninstall",
            Self::OperationLog => "operationLog",
            Self::LanguageEnglish => "languageEnglish",
            Self::LanguageChinese => "languageChinese",
        }
    }

    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }

    #[must_use]
    pub const fn accelerator(self) -> Option<&'static str> {
        match self {
            Self::CopyToOther => Some("F5"),
            Self::MoveToOther => Some("F6"),
            Self::Copy => Some("CmdOrCtrl+C"),
            Self::Cut => Some("CmdOrCtrl+X"),
            Self::Paste => Some("CmdOrCtrl+V"),
            Self::Rename => Some("F2"),
            Self::NewFolder => Some("CmdOrCtrl+Shift+N"),
            Self::Trash => Some("CmdOrCtrl+Backspace"),
            Self::Refresh => Some("CmdOrCtrl+R"),
            Self::ToggleHidden => Some("CmdOrCtrl+Shift+Period"),
            Self::OperationLog
            | Self::Diagnostics
            | Self::Uninstall
            | Self::LanguageEnglish
            | Self::LanguageChinese => None,
        }
    }

    /// Translation catalogs keyed by the same stable command identifiers as IPC.
    #[must_use]
    pub const fn label(self, language: DesktopLanguageDto) -> &'static str {
        match (language, self) {
            (DesktopLanguageDto::English, Self::CopyToOther) => "Copy to Other Pane",
            (DesktopLanguageDto::English, Self::MoveToOther) => "Move to Other Pane",
            (DesktopLanguageDto::English, Self::Copy) => "Copy",
            (DesktopLanguageDto::English, Self::Cut) => "Cut",
            (DesktopLanguageDto::English, Self::Paste) => "Paste…",
            (DesktopLanguageDto::English, Self::Rename) => "Rename…",
            (DesktopLanguageDto::English, Self::NewFolder) => "New Folder",
            (DesktopLanguageDto::English, Self::Trash) => "Move to Trash…",
            (DesktopLanguageDto::English, Self::Refresh) => "Refresh",
            (DesktopLanguageDto::English, Self::ToggleHidden) => "Show Hidden Items",
            (DesktopLanguageDto::English, Self::OperationLog) => "Operation Log…",
            (DesktopLanguageDto::SimplifiedChinese, Self::OperationLog) => "操作日志…",
            (DesktopLanguageDto::English, Self::Diagnostics) => "Export Diagnostics…",
            (DesktopLanguageDto::English, Self::Uninstall) => "Uninstall PaneDeck…",
            (_, Self::LanguageEnglish) => "English",
            (_, Self::LanguageChinese) => "简体中文",
            (DesktopLanguageDto::SimplifiedChinese, Self::CopyToOther) => "复制到另一窗格",
            (DesktopLanguageDto::SimplifiedChinese, Self::MoveToOther) => "移动到另一窗格",
            (DesktopLanguageDto::SimplifiedChinese, Self::Copy) => "复制",
            (DesktopLanguageDto::SimplifiedChinese, Self::Cut) => "剪切",
            (DesktopLanguageDto::SimplifiedChinese, Self::Paste) => "粘贴…",
            (DesktopLanguageDto::SimplifiedChinese, Self::Rename) => "重命名…",
            (DesktopLanguageDto::SimplifiedChinese, Self::NewFolder) => "新建文件夹",
            (DesktopLanguageDto::SimplifiedChinese, Self::Trash) => "移至废纸篓…",
            (DesktopLanguageDto::SimplifiedChinese, Self::Refresh) => "刷新",
            (DesktopLanguageDto::SimplifiedChinese, Self::ToggleHidden) => "显示隐藏项目",
            (DesktopLanguageDto::SimplifiedChinese, Self::Diagnostics) => "导出诊断…",
            (DesktopLanguageDto::SimplifiedChinese, Self::Uninstall) => "卸载 PaneDeck…",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopMenuSnapshotDto {
    pub revision: String,
    pub language: DesktopLanguageDto,
    pub enabled_commands: Vec<DesktopCommandDto>,
    pub text_editing: bool,
    pub show_hidden: bool,
}

impl DesktopMenuSnapshotDto {
    #[must_use]
    pub fn enabled(&self, command: DesktopCommandDto) -> bool {
        self.enabled_commands.contains(&command)
    }

    #[must_use]
    pub fn revision_number(&self) -> Option<u64> {
        if self.revision.is_empty() || !self.revision.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        self.revision.parse().ok()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopMenuEventDto {
    pub command: DesktopCommandDto,
    pub revision: String,
    pub event_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_actions_have_stable_ids_translations_and_unique_accelerators() {
        let mut ids = std::collections::HashSet::new();
        let mut accelerators = std::collections::HashSet::new();
        for command in DesktopCommandDto::ALL {
            assert!(ids.insert(command.id()));
            assert_eq!(DesktopCommandDto::from_id(command.id()), Some(command));
            for language in [
                DesktopLanguageDto::English,
                DesktopLanguageDto::SimplifiedChinese,
            ] {
                assert!(!command.label(language).is_empty());
            }
            if let Some(accelerator) = command.accelerator() {
                assert!(accelerators.insert(accelerator));
            }
        }
        assert_eq!(DesktopCommandDto::from_id("deleteForever"), None);
    }

    #[test]
    fn menu_snapshot_rejects_paths_and_unknown_commands() {
        assert!(serde_json::from_str::<DesktopMenuSnapshotDto>(r#"{"revision":"1","language":"en","enabledCommands":[],"textEditing":false,"showHidden":false,"path":"/private"}"#).is_err());
        assert!(serde_json::from_str::<DesktopCommandDto>("\"deleteForever\"").is_err());
        let snapshot = DesktopMenuSnapshotDto {
            revision: "2".to_owned(),
            enabled_commands: vec![DesktopCommandDto::Diagnostics],
            ..Default::default()
        };
        assert_eq!(snapshot.revision_number(), Some(2));
        assert!(!snapshot.enabled(DesktopCommandDto::Trash));
        assert!(snapshot.enabled(DesktopCommandDto::Diagnostics));
    }
}
