// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  app_lib::run();
}
#[tauri::command]
fn launch_minecraft(username: String, ram_gb: u32, version: String) -> Result<String, String> {
    
    println!("Запуск Minecraft для гравця: {}, RAM: {}GB, Версія: {}", username, ram_gb, version);
    
    
    Ok("Гра успішно запущена!".to_string())
}