use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")] 
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
            selected_version: "1.21.4".to_string(),
            ram_gb: 4,
            java_path: "java".to_string(),
        }
    }
}

pub fn get_settings_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("minecraft_launcher_rust");
    fs::create_dir_all(&path).ok();
    path.push("launcher_settings.json");
    path
}

pub fn load_settings() -> LauncherSettings {
    let path = get_settings_path();
    
    if path.exists() {
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str::<LauncherSettings>(&data) {
                println!("[Config] Прочитано конфіг: {:?}", path);
                return settings;
            }
        }
    }

    println!("[Config] Створюємо дефолтний конфіг: {:?}", path);
    let default_settings = LauncherSettings::default();
    let _ = save_settings(&default_settings);
    default_settings
}

pub fn save_settings(settings: &LauncherSettings) -> Result<(), std::io::Error> {
    let path = get_settings_path();
    let data = serde_json::to_string_pretty(settings)?;
    fs::write(&path, data)?;
    println!("[Config] Збережено конфіг у: {:?}", path);
    Ok(())
}