//! Native context-menu presentation. File capabilities remain in the captured UI command.
use crate::{DesktopCommandDto, DesktopLanguageDto, PaneIdDto};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ContextMenuKindDto {
    File,
    Directory,
    Multiple,
    Blank,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ContextCommandDto {
    Open,
    ToggleFolder,
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
    OperationLog,
    Diagnostics,
}
impl ContextCommandDto {
    pub const ALL: [Self; 14] = [
        Self::Open,
        Self::ToggleFolder,
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
        Self::OperationLog,
        Self::Diagnostics,
    ];
    pub fn desktop(self) -> Option<DesktopCommandDto> {
        Some(match self {
            Self::Open | Self::ToggleFolder => return None,
            Self::CopyToOther => DesktopCommandDto::CopyToOther,
            Self::MoveToOther => DesktopCommandDto::MoveToOther,
            Self::Copy => DesktopCommandDto::Copy,
            Self::Cut => DesktopCommandDto::Cut,
            Self::Paste => DesktopCommandDto::Paste,
            Self::Rename => DesktopCommandDto::Rename,
            Self::NewFolder => DesktopCommandDto::NewFolder,
            Self::Trash => DesktopCommandDto::Trash,
            Self::Refresh => DesktopCommandDto::Refresh,
            Self::ToggleHidden => DesktopCommandDto::ToggleHidden,
            Self::OperationLog => DesktopCommandDto::OperationLog,
            Self::Diagnostics => DesktopCommandDto::Diagnostics,
        })
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::ToggleFolder => "toggleFolder",
            _ => self.desktop().expect("desktop command").id(),
        }
    }
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|command| command.id() == id)
    }
    pub fn accelerator(self) -> Option<&'static str> {
        match self {
            Self::Open => Some("CmdOrCtrl+ArrowDown"),
            Self::ToggleFolder => None,
            _ => self.desktop().and_then(DesktopCommandDto::accelerator),
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextMenuRequestDto {
    pub token: String,
    pub kind: ContextMenuKindDto,
    pub language: DesktopLanguageDto,
    pub expanded: bool,
    pub show_hidden: bool,
    pub enabled_commands: Vec<ContextCommandDto>,
    pub target_pane: Option<PaneIdDto>,
    pub x: f64,
    pub y: f64,
}
pub struct ContextMenuItemDto {
    pub command: ContextCommandDto,
    pub label: String,
    pub enabled: bool,
    pub group: u8,
}
impl ContextMenuRequestDto {
    pub fn valid(&self) -> bool {
        !self.token.is_empty()
            && self.token.len() <= 64
            && self
                .token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && self.x.is_finite()
            && self.y.is_finite()
            && self.x >= 0.0
            && self.y >= 0.0
            && self.x <= 32768.0
            && self.y <= 32768.0
    }
    pub fn items(&self) -> Vec<ContextMenuItemDto> {
        use ContextCommandDto as C;
        let mut commands = Vec::new();
        if self.kind == ContextMenuKindDto::Directory {
            commands.extend([(C::Open, 0), (C::ToggleFolder, 0)]);
        }
        if self.kind != ContextMenuKindDto::Blank {
            commands.extend([
                (C::Copy, 1),
                (C::Cut, 1),
                (C::CopyToOther, 2),
                (C::MoveToOther, 2),
                (C::Rename, 3),
                (C::Trash, 3),
            ]);
        } else {
            commands.extend([
                (C::NewFolder, 0),
                (C::Paste, 1),
                (C::Refresh, 2),
                (C::ToggleHidden, 2),
                (C::OperationLog, 3),
                (C::Diagnostics, 3),
            ]);
        }
        commands
            .into_iter()
            .map(|(command, group)| {
                let chinese = self.language == DesktopLanguageDto::SimplifiedChinese;
                let label = match command {
                    C::Open => if chinese { "打开" } else { "Open" }.into(),
                    C::ToggleFolder => match (chinese, self.expanded) {
                        (true, true) => "折叠文件夹",
                        (true, false) => "展开文件夹",
                        (false, true) => "Collapse Folder",
                        (false, false) => "Expand Folder",
                    }
                    .into(),
                    C::CopyToOther | C::MoveToOther => {
                        match (chinese, self.target_pane, command == C::CopyToOther) {
                            (true, Some(PaneIdDto::Left), true) => "复制到左侧窗格",
                            (true, Some(PaneIdDto::Right), true) => "复制到右侧窗格",
                            (true, Some(PaneIdDto::Left), false) => "移动到左侧窗格",
                            (true, Some(PaneIdDto::Right), false) => "移动到右侧窗格",
                            (false, Some(PaneIdDto::Left), true) => "Copy to Left Pane",
                            (false, Some(PaneIdDto::Right), true) => "Copy to Right Pane",
                            (false, Some(PaneIdDto::Left), false) => "Move to Left Pane",
                            (false, Some(PaneIdDto::Right), false) => "Move to Right Pane",
                            _ => command.desktop().unwrap().label(self.language),
                        }
                        .into()
                    }
                    _ => command.desktop().unwrap().label(self.language).into(),
                };
                ContextMenuItemDto {
                    command,
                    label,
                    group,
                    enabled: self.enabled_commands.contains(&command)
                        && !(self.kind == ContextMenuKindDto::Multiple && command == C::Rename),
                }
            })
            .collect()
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextMenuEventDto {
    pub token: String,
    pub command: ContextCommandDto,
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request(kind: ContextMenuKindDto) -> ContextMenuRequestDto {
        ContextMenuRequestDto {
            token: "test-1".into(),
            kind,
            language: DesktopLanguageDto::English,
            expanded: false,
            show_hidden: false,
            enabled_commands: ContextCommandDto::ALL.to_vec(),
            target_pane: Some(PaneIdDto::Right),
            x: 10.0,
            y: 20.0,
        }
    }
    #[test]
    fn four_contexts_only_present_supported_grouped_commands() {
        for kind in [
            ContextMenuKindDto::File,
            ContextMenuKindDto::Directory,
            ContextMenuKindDto::Multiple,
            ContextMenuKindDto::Blank,
        ] {
            let menu = request(kind);
            let items = menu.items();
            assert!(menu.valid());
            assert!(items.windows(2).all(|pair| pair[0].group <= pair[1].group));
            assert_eq!(
                items
                    .iter()
                    .any(|item| item.command == ContextCommandDto::Open),
                kind == ContextMenuKindDto::Directory
            );
            assert_eq!(
                items
                    .iter()
                    .any(|item| item.command == ContextCommandDto::Paste),
                kind == ContextMenuKindDto::Blank
            );
            if kind == ContextMenuKindDto::Multiple {
                assert!(
                    !items
                        .iter()
                        .find(|item| item.command == ContextCommandDto::Rename)
                        .unwrap()
                        .enabled
                );
            }
            if kind != ContextMenuKindDto::Blank {
                assert!(items.iter().any(|item| item.label == "Move to Right Pane"));
            }
        }
    }
    #[test]
    fn menu_rejects_paths_bad_tokens_coordinates_and_unknown_commands() {
        let mut menu = request(ContextMenuKindDto::Directory);
        menu.x = f64::NAN;
        assert!(!menu.valid());
        menu.x = 1.0;
        menu.token = "../../path".into();
        assert!(!menu.valid());
        assert!(serde_json::from_str::<ContextMenuRequestDto>(
            r#"{"token":"1","nativePath":"/tmp/escape"}"#
        )
        .is_err());
        assert_eq!(ContextCommandDto::from_id("deleteForever"), None);
        for c in ContextCommandDto::ALL {
            assert_eq!(ContextCommandDto::from_id(c.id()), Some(c));
        }
    }
}
