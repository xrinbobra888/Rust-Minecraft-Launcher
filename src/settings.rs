use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LauncherSettings {
    pub username: String,
    pub selected_version: String,
    pub ram_gb: u32,
    pub java_path: String,
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            username: "Player".to_string(),
            selected_version: "1.21.1".to_string(),
            ram_gb: 4,
            java_path: "java".to_string(),
        }
    }
}

pub fn get_settings_path() -> PathBuf {
    let mut path = dirs::data_dir().expect("Не вдалося визначити системну папку AppData");
    path.push(".minecraft");
    path.push("launcher_settings.json");
    path
}

pub fn load_settings() -> LauncherSettings {
    let path = get_settings_path();
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str(&content) {
                return settings;
            }
        }
    }
    let default_settings = LauncherSettings::default();
    let _ = save_settings(&default_settings);
    default_settings
}

pub fn save_settings(settings: &LauncherSettings) -> Result<(), Box<dyn std::error::Error>> {
    let path = get_settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(settings)?;
    fs::write(path, json)?;
    Ok(())
}