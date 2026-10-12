use panedeck_app::{
    ContextCommandDto, ContextMenuEventDto, ContextMenuKindDto, ContextMenuRequestDto,
};
use std::sync::Mutex;
use tauri::{
    menu::{CheckMenuItem, ContextMenu, Menu, MenuItem, PredefinedMenuItem},
    AppHandle, Emitter, LogicalPosition, Manager, Window, Wry,
};
#[derive(Default)]
pub struct ContextMenuState {
    current: Mutex<Option<ActiveMenu>>,
}
struct ActiveMenu {
    request: ContextMenuRequestDto,
    _menu: Menu<Wry>,
}
#[tauri::command]
pub async fn popup_context_menu(
    app: AppHandle,
    window: Window,
    state: tauri::State<'_, ContextMenuState>,
    request: ContextMenuRequestDto,
) -> Result<(), String> {
    if !request.valid() {
        return Err("invalid context menu request".into());
    }
    let menu = Menu::new(&app).map_err(|_| "could not create context menu")?;
    let mut previous_group = None;
    for item in request.items() {
        if previous_group.is_some_and(|group| group != item.group) {
            menu.append(
                &PredefinedMenuItem::separator(&app).map_err(|_| "could not create separator")?,
            )
            .map_err(|_| "could not append separator")?;
        }
        previous_group = Some(item.group);
        let id = format!("panedeck-context:{}:{}", request.token, item.command.id());
        if item.command == ContextCommandDto::ToggleHidden {
            menu.append(
                &CheckMenuItem::with_id(
                    &app,
                    id,
                    item.label,
                    item.enabled,
                    request.show_hidden,
                    item.command.accelerator(),
                )
                .map_err(|_| "could not create context item")?,
            )
            .map_err(|_| "could not append context item")?;
        } else {
            let accelerator = if item.command == ContextCommandDto::NewFolder
                && request.kind == ContextMenuKindDto::Directory
            {
                None
            } else {
                item.command.accelerator()
            };
            menu.append(
                &MenuItem::with_id(&app, id, item.label, item.enabled, accelerator)
                    .map_err(|_| "could not create context item")?,
            )
            .map_err(|_| "could not append context item")?;
        }
    }
    let position = LogicalPosition::new(request.x, request.y);
    *state
        .current
        .lock()
        .map_err(|_| "context menu unavailable")? = Some(ActiveMenu {
        request,
        _menu: menu.clone(),
    });
    if menu.popup_at(window, position).is_err() {
        state
            .current
            .lock()
            .map_err(|_| "context menu unavailable")?
            .take();
        return Err("could not show context menu".into());
    }
    // Keep one bounded snapshot: Cocoa selection delivery may follow popup completion.
    Ok(())
}
fn parse_id(id: &str) -> Option<(&str, ContextCommandDto)> {
    let (token, command) = id.strip_prefix("panedeck-context:")?.split_once(':')?;
    Some((token, ContextCommandDto::from_id(command)?))
}
pub fn handle_event(app: &AppHandle, id: &str) {
    let Some((token, command)) = parse_id(id) else {
        return;
    };
    let state = app.state::<ContextMenuState>();
    let Ok(mut current) = state.current.lock() else {
        return;
    };
    let Some(active) = current.as_ref() else {
        return;
    };
    if active.request.token != token
        || !active
            .request
            .items()
            .iter()
            .any(|item| item.command == command && item.enabled)
    {
        return;
    }
    let event = ContextMenuEventDto {
        token: token.into(),
        command,
    };
    let consumed = current.take();
    drop(current);
    if let Err(error) = app.emit("context-menu-command", event) {
        eprintln!("could not deliver context menu command: {error}");
    }
    drop(consumed);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_scoped_supported_context_events_parse() {
        assert_eq!(
            parse_id("panedeck-context:t1:copy"),
            Some(("t1", ContextCommandDto::Copy))
        );
        assert_eq!(parse_id("panedeck:7:copy"), None);
        assert_eq!(parse_id("panedeck-context:t1:deleteForever"), None);
    }
}
