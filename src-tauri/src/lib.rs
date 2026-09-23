const APP_NAME: &str = "PaneDeck";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .unwrap_or_else(|error| panic!("failed to run {APP_NAME}: {error}"));
}

#[cfg(test)]
mod tests {
    use super::APP_NAME;

    #[test]
    fn app_name_is_stable() {
        assert_eq!(APP_NAME, "PaneDeck");
    }
}
