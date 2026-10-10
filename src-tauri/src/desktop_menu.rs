use panedeck_app::{
    DesktopCommandDto as Command, DesktopLanguageDto as Language, DesktopMenuEventDto,
    DesktopMenuSnapshotDto,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    AppHandle, Emitter, Manager, Wry,
};

#[derive(Default)]
pub struct DesktopMenuState {
    snapshot: Mutex<DesktopMenuSnapshotDto>,
    next_event: AtomicU64,
}

fn item(
    app: &AppHandle,
    snapshot: &DesktopMenuSnapshotDto,
    command: Command,
) -> tauri::Result<MenuItem<Wry>> {
    MenuItem::with_id(
        app,
        format!("panedeck:{}:{}", snapshot.revision, command.id()),
        command.label(snapshot.language),
        snapshot.enabled(command),
        command.accelerator(),
    )
}

fn check(
    app: &AppHandle,
    snapshot: &DesktopMenuSnapshotDto,
    command: Command,
    checked: bool,
) -> tauri::Result<CheckMenuItem<Wry>> {
    CheckMenuItem::with_id(
        app,
        format!("panedeck:{}:{}", snapshot.revision, command.id()),
        command.label(snapshot.language),
        snapshot.enabled(command),
        checked,
        command.accelerator(),
    )
}

pub fn build(app: &AppHandle, snapshot: &DesktopMenuSnapshotDto) -> tauri::Result<Menu<Wry>> {
    let chinese = snapshot.language == Language::SimplifiedChinese;
    let languages = Submenu::with_items(
        app,
        if chinese { "语言" } else { "Language" },
        true,
        &[
            &check(app, snapshot, Command::LanguageEnglish, !chinese)?,
            &check(app, snapshot, Command::LanguageChinese, chinese)?,
        ],
    )?;
    let application = Submenu::with_items(
        app,
        "PaneDeck",
        true,
        &[
            &languages,
            &item(app, snapshot, Command::Diagnostics)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(
                app,
                Some(if chinese {
                    "隐藏 PaneDeck"
                } else {
                    "Hide PaneDeck"
                }),
            )?,
            &PredefinedMenuItem::hide_others(
                app,
                Some(if chinese {
                    "隐藏其他"
                } else {
                    "Hide Others"
                }),
            )?,
            &PredefinedMenuItem::show_all(
                app,
                Some(if chinese { "显示全部" } else { "Show All" }),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(
                app,
                Some(if chinese {
                    "退出 PaneDeck"
                } else {
                    "Quit PaneDeck"
                }),
            )?,
        ],
    )?;
    let file = Submenu::with_items(
        app,
        if chinese { "文件" } else { "File" },
        true,
        &[
            &item(app, snapshot, Command::CopyToOther)?,
            &item(app, snapshot, Command::MoveToOther)?,
            &PredefinedMenuItem::separator(app)?,
            &item(app, snapshot, Command::Rename)?,
            &item(app, snapshot, Command::NewFolder)?,
            &item(app, snapshot, Command::Trash)?,
        ],
    )?;
    // Text fields use the native responder chain, not file clipboard commands.
    let edit = if snapshot.text_editing {
        Submenu::with_items(
            app,
            if chinese { "编辑" } else { "Edit" },
            true,
            &[
                &PredefinedMenuItem::undo(app, Some(if chinese { "撤销" } else { "Undo" }))?,
                &PredefinedMenuItem::redo(app, Some(if chinese { "重做" } else { "Redo" }))?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::cut(app, Some(if chinese { "剪切" } else { "Cut" }))?,
                &PredefinedMenuItem::copy(app, Some(if chinese { "复制" } else { "Copy" }))?,
                &PredefinedMenuItem::paste(app, Some(if chinese { "粘贴" } else { "Paste" }))?,
                &PredefinedMenuItem::select_all(
                    app,
                    Some(if chinese { "全选" } else { "Select All" }),
                )?,
            ],
        )?
    } else {
        Submenu::with_items(
            app,
            if chinese { "编辑" } else { "Edit" },
            true,
            &[
                &item(app, snapshot, Command::Copy)?,
                &item(app, snapshot, Command::Cut)?,
                &item(app, snapshot, Command::Paste)?,
            ],
        )?
    };
    let view = Submenu::with_items(
        app,
        if chinese { "显示" } else { "View" },
        true,
        &[
            &item(app, snapshot, Command::Refresh)?,
            &check(app, snapshot, Command::ToggleHidden, snapshot.show_hidden)?,
            &PredefinedMenuItem::separator(app)?,
            &item(app, snapshot, Command::OperationLog)?,
        ],
    )?;
    Menu::with_items(app, &[&application, &file, &edit, &view])
}

#[tauri::command]
pub fn sync_desktop_menu(
    app: AppHandle,
    state: tauri::State<'_, DesktopMenuState>,
    snapshot: DesktopMenuSnapshotDto,
) -> Result<(), String> {
    let revision = snapshot.revision_number().ok_or("invalid menu revision")?;
    {
        let current = state
            .snapshot
            .lock()
            .map_err(|_| "menu state unavailable")?;
        if revision <= current.revision_number().unwrap_or(0) {
            return Ok(());
        }
    }
    let menu = build(&app, &snapshot).map_err(|_| "could not build desktop menu")?;
    app.set_menu(menu)
        .map_err(|_| "could not install desktop menu")?;
    *state
        .snapshot
        .lock()
        .map_err(|_| "menu state unavailable")? = snapshot;
    Ok(())
}

pub fn handle_event(app: &AppHandle, id: &str) {
    let Some((revision, command)) = parse_event_id(id) else {
        return;
    };
    let state = app.state::<DesktopMenuState>();
    let Ok(snapshot) = state.snapshot.lock() else {
        return;
    };
    if revision != snapshot.revision || !snapshot.enabled(command) {
        return;
    }
    let event = DesktopMenuEventDto {
        command,
        revision: snapshot.revision.clone(),
        event_id: state.next_event.fetch_add(1, Ordering::Relaxed).to_string(),
    };
    drop(snapshot);
    if let Err(error) = app.emit("desktop-menu-command", event) {
        eprintln!("could not deliver desktop menu command: {error}");
    }
}

fn parse_event_id(id: &str) -> Option<(&str, Command)> {
    let (revision, command) = id.strip_prefix("panedeck:")?.split_once(':')?;
    Some((revision, Command::from_id(command)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_events_are_revision_bound_and_ignore_system_or_unknown_actions() {
        assert_eq!(
            parse_event_id("panedeck:7:copy"),
            Some(("7", Command::Copy))
        );
        assert_eq!(parse_event_id("quit"), None);
        assert_eq!(parse_event_id("panedeck:7:deleteForever"), None);
    }
}
