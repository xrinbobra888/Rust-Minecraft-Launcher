mod settings;
use settings::{load_settings, save_settings, LauncherSettings};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct VersionManifest {
    pub versions: Vec<VersionInfo>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct VersionInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct VersionData {
    id: String,
    downloads: VersionDownloads,
    libraries: Vec<Library>,
    #[serde(rename = "mainClass")]
    pub main_class: Option<String>,
    #[serde(rename = "assetIndex")]
    pub asset_index: AssetIndexInfo,
}

#[derive(Debug, Deserialize)]
struct VersionDownloads {
    client: DownloadFile,
}

#[derive(Debug, Deserialize)]
struct DownloadFile {
    url: String,
}

#[derive(Debug, Deserialize)]
struct AssetIndexInfo {
    id: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct AssetIndexJson {
    objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Deserialize)]
struct AssetObject {
    hash: String,
    #[allow(dead_code)]
    size: u64,
}

#[derive(Debug, Deserialize)]
struct Library {
    #[allow(dead_code)]
    name: String,
    downloads: Option<LibraryDownloads>,
    rules: Option<Vec<Rule>>,
    #[allow(dead_code)]
    extract: Option<ExtractRule>,
}

#[derive(Debug, Deserialize)]
struct LibraryDownloads {
    artifact: Option<Artifact>,
    classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Debug, Deserialize)]
struct Artifact {
    path: Option<String>,
    url: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ExtractRule {
    exclude: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct Rule {
    action: String,
    os: Option<OsRule>,
}

#[derive(Debug, Deserialize)]
struct OsRule {
    name: String,
}

fn get_minecraft_dir() -> PathBuf {
    let mut path = dirs::data_dir().expect("Не вдалося визначити системну папку AppData");
    path.push(".minecraft");
    path
}

async fn download_file(client: &reqwest::Client, url: &str, target_path: &PathBuf) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if target_path.exists() {
        return Ok(());
    }

    let bytes = client.get(url).send().await?.bytes().await?;
    let mut file = File::create(target_path)?;
    file.write_all(&bytes)?;

    Ok(())
}

fn should_download_library(rules: &Option<Vec<Rule>>) -> bool {
    let Some(rules) = rules else {
        return true;
    };

    let mut allow = true;
    for rule in rules {
        let mut os_matches = true;
        if let Some(os) = &rule.os {
            if os.name != "windows" {
                os_matches = false;
            }
        }

        if os_matches {
            if rule.action == "allow" {
                allow = true;
            } else if rule.action == "disallow" {
                allow = false;
            }
        }
    }
    allow
}

fn extract_natives(jar_path: &PathBuf, target_dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(jar_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    fs::create_dir_all(target_dir)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();

        if name.ends_with(".class") || name.starts_with("META-INF") {
            continue;
        }

        let out_path = target_dir.join(&name);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut outfile = File::create(&out_path)?;
        std::io::copy(&mut file, &mut outfile)?;
    }

    Ok(())
}

#[tauri::command]
fn get_settings() -> LauncherSettings {
    load_settings()
}

#[tauri::command]
fn save_settings_cmd(settings: LauncherSettings) -> Result<(), String> {
    save_settings(&settings).map_err(|e| e.to_string())
}


#[tauri::command]
async fn launch_minecraft(username: String, ram_gb: u32, version: String) -> Result<String, String> {
    let result: Result<String, String> = async move {
        let mc_dir = get_minecraft_dir();
        let libraries_dir = mc_dir.join("libraries");
        let natives_dir = mc_dir.join("versions").join(&version).join("natives");
        let http_client = reqwest::Client::new();

        println!("[Launcher] Отримання маніфесту версій для завантаження: {}", version);
        let manifest_url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
        let manifest: VersionManifest = http_client.get(manifest_url).send().await.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;

        let version_info = manifest
            .versions
            .iter()
            .find(|v| v.id == version)
            .ok_or_else(|| format!("Версію {} не знайдено у маніфесті Mojang", version))?;

        let version_dir = mc_dir.join("versions").join(&version);
        let version_json_path = version_dir.join(format!("{}.json", version));
        let version_jar_path = version_dir.join(format!("{}.jar", version));

        println!("[Launcher] Завантаження JSON конфігурації версії...");
        download_file(&http_client, &version_info.url, &version_json_path).await.map_err(|e| e.to_string())?;

        let version_json_content = fs::read_to_string(&version_json_path).map_err(|e| e.to_string())?;
        let version_data: VersionData = serde_json::from_str(&version_json_content).map_err(|e| e.to_string())?;

        println!("[Launcher] Завантаження клієнтського файлу гри (.jar)...");
        download_file(&http_client, &version_data.downloads.client.url, &version_jar_path).await.map_err(|e| e.to_string())?;

        let mut classpath_entries = vec![version_jar_path.to_string_lossy().to_string()];

        println!("[Launcher] Перевірка та завантаження бібліотек...");
        for lib in &version_data.libraries {
            if !should_download_library(&lib.rules) {
                continue;
            }

            if let Some(downloads) = &lib.downloads {
                if let Some(classifiers) = &downloads.classifiers {
                    if let Some(native_artifact) = classifiers.get("natives-windows") {
                        let filename = native_artifact.url.split('/').last().unwrap_or("native.jar");
                        let native_jar_path = libraries_dir.join("natives_downloads").join(filename);
                        if !native_jar_path.exists() {
                            let _ = download_file(&http_client, &native_artifact.url, &native_jar_path).await;
                        }
                        let _ = extract_natives(&native_jar_path, &natives_dir);
                    }
                }

                if let Some(artifact) = &downloads.artifact {
                    if let Some(path_str) = &artifact.path {
                        let lib_path = libraries_dir.join(path_str);
                        if !lib_path.exists() {
                            let _ = download_file(&http_client, &artifact.url, &lib_path).await;
                        }
                        classpath_entries.push(lib_path.to_string_lossy().to_string());
                    }
                }
            }
        }

        println!("[Launcher] Перевірка та завантаження асетів (текстур, звуків)...");
        let asset_index_path = mc_dir.join("assets").join("indexes").join(format!("{}.json", version_data.asset_index.id));
        download_file(&http_client, &version_data.asset_index.url, &asset_index_path).await.map_err(|e| e.to_string())?;

        if let Ok(content) = fs::read_to_string(&asset_index_path) {
            if let Ok(asset_index_json) = serde_json::from_str::<AssetIndexJson>(&content) {
                let objects_dir = mc_dir.join("assets").join("objects");
                for (_, obj) in asset_index_json.objects {
                    let hash = obj.hash;
                    let sub_folder = &hash[0..2];
                    let asset_path = objects_dir.join(sub_folder).join(&hash);
                    if !asset_path.exists() {
                        let asset_url = format!("https://resources.download.minecraft.net/{}/{}", sub_folder, hash);
                        let _ = download_file(&http_client, &asset_url, &asset_path).await;
                    }
                }
            }
        }

        let classpath = classpath_entries.join(";");

        let offline_uuid = Uuid::new_v3(&Uuid::NAMESPACE_DNS, format!("OfflinePlayer:{}", username).as_bytes())
            .simple()
            .to_string();

        let assets_dir = mc_dir.join("assets");
        let main_class = version_data.main_class.unwrap_or_else(|| "net.minecraft.client.main.Main".to_string());

        println!("[Launcher] Запуск процесу Java (Гравця: {}, Версія: {}, RAM: {}GB)...", username, version, ram_gb);
        let status = Command::new("java")
            .arg(format!("-Xmx{}G", ram_gb))
            .arg(format!("-Djava.library.path={}", natives_dir.to_string_lossy()))
            .arg("-Dfile.encoding=UTF-8")
            .arg("-cp")
            .arg(classpath)
            .arg(main_class)
            .arg("--username").arg(&username)
            .arg("--version").arg(&version)
            .arg("--gameDir").arg(&mc_dir)
            .arg("--assetsDir").arg(&assets_dir)
            .arg("--assetIndex").arg(version_data.asset_index.id)
            .arg("--uuid").arg(offline_uuid)
            .arg("--accessToken").arg("0")
            .arg("--userType").arg("legacy")
            .status()
            .map_err(|e| e.to_string())?;

        if status.success() {
            Ok(format!("Minecraft {} успішно завершив роботу!", version))
        } else {
            Err(format!("Гра завершилася з кодом помилки: {:?}", status.code()))
        }
    }.await;

    result
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings_cmd,
            launch_minecraft
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}