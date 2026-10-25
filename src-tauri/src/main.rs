#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    app_lib::run();
}

#[tauri::command]
fn launch_minecraft(username: String, ram_gb: u32, version: String) -> Result<String, String> {
    
    println!("Launching Minecraft for player: {}, RAM: {}GB, Version: {}", username, ram_gb, version);
    
    
    Ok("the game has been launched!".to_string())
}