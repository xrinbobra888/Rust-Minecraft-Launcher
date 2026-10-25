mod settings;

use settings::{load_settings, save_settings, LauncherSettings};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct VersionManifest {
    versions: Vec<VersionInfo>,
}

#[derive(Debug, Deserialize, Clone)]
struct VersionInfo {
    id: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    version_type: String,
    url: String,
    #[serde(rename = "javaVersion")]
    java_version: Option<JavaVersionInfo>,
}

#[derive(Debug, Deserialize, Clone)]
struct JavaVersionInfo {
    #[allow(dead_code)]
    component: String,
    #[serde(rename = "majorVersion")]
    major_version: u32,
}

#[derive(Debug, Deserialize, Clone)]
struct VersionData {
    #[allow(dead_code)]
    id: String,
    downloads: Option<VersionDownloads>,
    libraries: Vec<Library>,
    #[serde(rename = "mainClass")]
    main_class: Option<String>,
    #[serde(rename = "assetIndex")]
    asset_index: Option<AssetIndexInfo>,
    #[serde(rename = "inheritsFrom")]
    #[allow(dead_code)]
    inherits_from: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct VersionDownloads {
    client: DownloadFile,
}

#[derive(Debug, Deserialize, Clone)]
struct DownloadFile {
    url: String,
}

#[derive(Debug, Deserialize, Clone)]
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
}

#[derive(Debug, Deserialize, Clone)]
struct Library {
    name: String,
    downloads: Option<LibraryDownloads>,
    rules: Option<Vec<Rule>>,
    url: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct LibraryDownloads {
    artifact: Option<Artifact>,
    classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Debug, Deserialize, Clone)]
struct Artifact {
    path: Option<String>,
    url: String,
}

#[derive(Debug, Deserialize, Clone)]
struct Rule {
    action: String,
    os: Option<OsRule>,
}

#[derive(Debug, Deserialize, Clone)]
struct OsRule {
    name: String,
}

#[derive(Debug, Deserialize)]
struct FabricLoaderResponse {
    loader: FabricLoaderDetails,
}

#[derive(Debug, Deserialize)]
struct FabricLoaderDetails {
    version: String,
}

fn get_minecraft_dir() -> PathBuf {
    let mut path = dirs::data_dir().expect("╨¥╨╡ ╨▓╨┤╨░╨╗╨╛╤ü╤Å ╨╖╨╜╨░╨╣╤é╨╕ AppData");
    path.push(".minecraft");
    path
}

fn get_runtimes_dir() -> PathBuf {
    let mut path = dirs::data_dir().expect("╨¥╨╡ ╨▓╨┤╨░╨╗╨╛╤ü╤Å ╨╖╨╜╨░╨╣╤é╨╕ AppData");
    path.push("minecraft_launcher_runtimes");
    path
}

async fn download_file(client: &reqwest::Client, url: &str, target_path: &PathBuf) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if target_path.exists() {
        if let Ok(metadata) = fs::metadata(target_path) {
            if metadata.len() > 100 {
                if let Ok(file) = File::open(target_path) {
                    if target_path.extension().map_or(false, |e| e == "zip" || e == "jar") {
                        if zip::ZipArchive::new(file).is_ok() {
                            return Ok(());
                        }
                    } else {
                        return Ok(());
                    }
                }
            }
        }
        let _ = fs::remove_file(target_path);
    }

    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(format!("HTTP ╤ü╤é╨░╤é╤â╤ü {} ╨┤╨╗╤Å {}", response.status(), url).into());
    }

    let bytes = response.bytes().await?;
    let mut file = File::create(target_path)?;
    file.write_all(&bytes)?;

    Ok(())
}

async fn ensure_java_runtime(client: &reqwest::Client, major_version: u32) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let runtimes_dir = get_runtimes_dir();
    let java_folder_name = format!("java-runtime-{}", major_version);
    let java_home = runtimes_dir.join(&java_folder_name);
    
    let java_exe = if cfg!(target_os = "windows") {
        java_home.join("bin").join("java.exe")
    } else {
        java_home.join("bin").join("java")
    };

    if java_exe.exists() {
        return Ok(java_exe);
    }

    let api_url = format!(
        "https://api.adoptium.net/v3/binary/latest/{}/ga/windows/x64/jdk/hotspot/normal/eclipse",
        major_version
    );

    let zip_path = runtimes_dir.join(format!("java_{}.zip", major_version));
    download_file(client, &api_url, &zip_path).await?;

    let file = File::open(&zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    
    fs::create_dir_all(&java_home)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();
        let parts: Vec<&str> = name.split('/').collect();
        if parts.len() <= 1 {
            continue;
        }
        let relative_path: PathBuf = parts[1..].iter().collect();
        let out_path = java_home.join(relative_path);

        if file.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = File::create(&out_path)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }

    let _ = fs::remove_file(&zip_path);

    if java_exe.exists() {
        Ok(java_exe)
    } else {
        Err("╨¥╨╡ ╨▓╨┤╨░╨╗╨╛╤ü╤Å ╨╖╨╜╨░╨╣╤é╨╕ ╨▓╨╕╨║╨╛╨╜╤â╨▓╨░╨╜╨╕╨╣ ╤ä╨░╨╣╨╗ Java ╨┐╤û╤ü╨╗╤Å ╤Ç╨╛╨╖╨┐╨░╨║╤â╨▓╨░╨╜╨╜╤Å".into())
    }
}

fn maven_name_to_path(name: &str) -> Option<PathBuf> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() >= 3 {
        let group = parts[0].replace('.', "/");
        let artifact = parts[1];
        let version = parts[2];
        Some(PathBuf::from(group).join(artifact).join(version).join(format!("{}-{}.jar", artifact, version)))
    } else {
        None
    }
}

fn should_download_library(rules: &Option<Vec<Rule>>) -> bool {
    let Some(rules) = rules else { return true; };
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
        if name.starts_with("META-INF") {
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

async fn download_and_collect_lib(
    lib: &Library,
    libraries_dir: &PathBuf,
    http_client: &reqwest::Client,
    classpath: &mut Vec<String>,
    natives_dir: &PathBuf,
) {
    if !should_download_library(&lib.rules) {
        return;
    }

    if let Some(downloads) = &lib.downloads {
        if let Some(classifiers) = &downloads.classifiers {
            if let Some(native) = classifiers.get("natives-windows") {
                let filename = native.url.split('/').last().unwrap_or("native.jar");
                let native_jar = libraries_dir.join("natives_downloads").join(filename);
                if download_file(http_client, &native.url, &native_jar).await.is_ok() {
                    let _ = extract_natives(&native_jar, natives_dir);
                }
            }
        }
    }

    let rel_path = match maven_name_to_path(&lib.name) {
        Some(p) => p,
        None => return,
    };

    let lib_path = if let Some(downloads) = &lib.downloads {
        if let Some(artifact) = &downloads.artifact {
            if let Some(path_str) = &artifact.path {
                libraries_dir.join(path_str)
            } else {
                libraries_dir.join(&rel_path)
            }
        } else {
            libraries_dir.join(&rel_path)
        }
    } else {
        libraries_dir.join(&rel_path)
    };

    let mut urls = Vec::new();
    if let Some(downloads) = &lib.downloads {
        if let Some(artifact) = &downloads.artifact {
            urls.push(artifact.url.clone());
        }
    }
    if let Some(base_url) = &lib.url {
        let rel_str = rel_path.to_string_lossy().replace('\\', "/");
        urls.push(format!("{}/{}", base_url.trim_end_matches('/'), rel_str));
    }
    let rel_str = rel_path.to_string_lossy().replace('\\', "/");
    urls.push(format!("https://maven.fabricmc.net/{}", rel_str));
    urls.push(format!("https://repo1.maven.org/maven2/{}", rel_str));
    urls.push(format!("https://libraries.minecraft.net/{}", rel_str));

    let mut success = false;
    if lib_path.exists() {
        if let Ok(meta) = fs::metadata(&lib_path) {
            if meta.len() > 100 {
                if let Ok(file) = File::open(&lib_path) {
                    if zip::ZipArchive::new(file).is_ok() {
                        success = true;
                    }
                }
            }
        }
    }

    if !success {
        for url in &urls {
            if download_file(http_client, url, &lib_path).await.is_ok() {
                if let Ok(meta) = fs::metadata(&lib_path) {
                    if meta.len() > 100 {
                        if let Ok(file) = File::open(&lib_path) {
                            if zip::ZipArchive::new(file).is_ok() {
                                success = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    if success {
        classpath.push(lib_path.to_string_lossy().to_string());
    }
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

        let is_fabric = version.ends_with("-fabric");
        let game_version = if is_fabric { version.replace("-fabric", "") } else { version.clone() };

        let version_dir = mc_dir.join("versions").join(&version);
        fs::create_dir_all(&version_dir).map_err(|e| e.to_string())?;

        let manifest_url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
        let manifest: VersionManifest = http_client.get(manifest_url).send().await.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;

        let game_version_info = manifest.versions.iter()
            .find(|v| v.id == game_version)
            .ok_or_else(|| format!("╨Æ╨╡╤Ç╤ü╤û╤Ä {} ╨╜╨╡ ╨╖╨╜╨░╨╣╨┤╨╡╨╜╨╛ ╤â ╨╝╨░╨╜╤û╤ä╨╡╤ü╤é╤û Mojang", game_version))?;

        let mut required_java_major = if game_version.starts_with("26.") { 25 } else { 21 };
        if let Some(j_info) = &game_version_info.java_version {
            required_java_major = j_info.major_version;
        }

        let vanilla_dir = mc_dir.join("versions").join(&game_version);
        fs::create_dir_all(&vanilla_dir).map_err(|e| e.to_string())?;
        let vanilla_json_path = vanilla_dir.join(format!("{}.json", game_version));
        let vanilla_jar_path = vanilla_dir.join(format!("{}.jar", game_version));

        download_file(&http_client, &game_version_info.url, &vanilla_json_path).await.map_err(|e| e.to_string())?;
        let vanilla_json_content = fs::read_to_string(&vanilla_json_path).map_err(|e| e.to_string())?;
        let vanilla_data: VersionData = serde_json::from_str(&vanilla_json_content).map_err(|e| e.to_string())?;

        if let Some(downloads) = &vanilla_data.downloads {
            download_file(&http_client, &downloads.client.url, &vanilla_jar_path).await.map_err(|e| e.to_string())?;
        }

        let asset_index_info = vanilla_data.asset_index.clone()
            .ok_or_else(|| "╨Æ╨╡╤Ç╤ü╤û╤Å ╨╜╨╡ ╨╝╨░╤ö assetIndex".to_string())?;

        let mut target_data = vanilla_data.clone();
        if is_fabric {
            let fabric_json_path = version_dir.join(format!("{}.json", version));
            if !fabric_json_path.exists() {
                let loaders_url = format!("https://meta.fabricmc.net/v2/versions/loader/{}", game_version);
                let loaders_resp: Vec<FabricLoaderResponse> = http_client.get(&loaders_url).send().await.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
                let loader_ver = loaders_resp.first()
                    .ok_or_else(|| format!("╨¥╨╡ ╨▓╨┤╨░╨╗╨╛╤ü╤Å ╨╖╨╜╨░╨╣╤é╨╕ Fabric loader ╨┤╨╗╤Å {}", game_version))?
                    .loader.version.clone();
                let profile_url = format!("https://meta.fabricmc.net/v2/versions/loader/{}/{}/profile/json", game_version, loader_ver);
                download_file(&http_client, &profile_url, &fabric_json_path).await.map_err(|e| e.to_string())?;
            }
            let fabric_content = fs::read_to_string(&fabric_json_path).map_err(|e| e.to_string())?;
            target_data = serde_json::from_str(&fabric_content).map_err(|e| e.to_string())?;
        }

        let java_path = ensure_java_runtime(&http_client, required_java_major).await
            .map_err(|e| format!("╨ƒ╨╛╨╝╨╕╨╗╨║╨░ ╨╜╨░╨╗╨░╤ê╤é╤â╨▓╨░╨╜╨╜╤Å Java: {}", e))?;

        let mut classpath = Vec::new();
        let mut seen_libs = HashSet::new();

        for lib in &vanilla_data.libraries {
            let parts: Vec<&str> = lib.name.split(':').collect();
            if parts.len() >= 2 {
                seen_libs.insert(format!("{}:{}", parts[0], parts[1]));
            }
            download_and_collect_lib(lib, &libraries_dir, &http_client, &mut classpath, &natives_dir).await;
        }

        if is_fabric {
            for lib in &target_data.libraries {
                let parts: Vec<&str> = lib.name.split(':').collect();
                if parts.len() >= 2 {
                    seen_libs.insert(format!("{}:{}", parts[0], parts[1]));
                }
                download_and_collect_lib(lib, &libraries_dir, &http_client, &mut classpath, &natives_dir).await;
            }
        }

        classpath.push(vanilla_jar_path.to_string_lossy().to_string());

        let asset_index_path = mc_dir.join("assets").join("indexes").join(format!("{}.json", asset_index_info.id));
        download_file(&http_client, &asset_index_info.url, &asset_index_path).await.map_err(|e| e.to_string())?;

        if let Ok(content) = fs::read_to_string(&asset_index_path) {
            if let Ok(index_json) = serde_json::from_str::<AssetIndexJson>(&content) {
                let objects_dir = mc_dir.join("assets").join("objects");
                for (_, obj) in index_json.objects {
                    let hash = obj.hash;
                    if hash.len() >= 2 {
                        let sub = &hash[0..2];
                        let asset_path = objects_dir.join(sub).join(&hash);
                        if !asset_path.exists() {
                            let url = format!("https://resources.download.minecraft.net/{}/{}", sub, hash);
                            let _ = download_file(&http_client, &url, &asset_path).await;
                        }
                    }
                }
            }
        }

        let cp_string = classpath.join(";");
        let offline_uuid = Uuid::new_v3(&Uuid::NAMESPACE_DNS, format!("OfflinePlayer:{}", username).as_bytes())
            .simple()
            .to_string();

        let main_class = target_data.main_class.unwrap_or_else(|| {
            if is_fabric {
                "net.fabricmc.loader.impl.launch.knot.KnotClient".to_string()
            } else {
                "net.minecraft.client.main.Main".to_string()
            }
        });

        let assets_dir = mc_dir.join("assets");

        let status = Command::new(&java_path)
            .arg(format!("-Xmx{}G", ram_gb))
            .arg("-XX:+UseG1GC")
            .arg("-XX:-UseAdaptiveSizePolicy")
            .arg("-XX:-OmitStackTraceInFastThrow")
            .arg("--enable-native-access=ALL-UNNAMED")
            .arg(format!("-Djava.library.path={}", natives_dir.to_string_lossy()))
            .arg("-Dfile.encoding=UTF-8")
            .arg("-cp")
            .arg(cp_string)
            .arg(main_class)
            .arg("--username").arg(&username)
            .arg("--version").arg(&game_version)
            .arg("--gameDir").arg(&mc_dir)
            .arg("--assetsDir").arg(&assets_dir)
            .arg("--assetIndex").arg(&asset_index_info.id)
            .arg("--uuid").arg(offline_uuid)
            .arg("--accessToken").arg("0")
            .arg("--userType").arg("legacy")
            .status()
            .map_err(|e| format!("╨¥╨╡ ╨▓╨┤╨░╨╗╨╛╤ü╤Å ╨╖╨░╨┐╤â╤ü╤é╨╕╤é╨╕ Java: {}", e))?;

        if status.success() {
            Ok(format!("╨ô╤Ç╨░ ╨╖╨░╨▓╨╡╤Ç╤ê╨╕╨╗╨░╤ü╤Å ╤â╤ü╨┐╤û╤ê╨╜╨╛"))
        } else {
            Err(format!("╨ô╤Ç╨░ ╨╖╨░╨▓╨╡╤Ç╤ê╨╕╨╗╨░╤ü╤Å ╨╖ ╨║╨╛╨┤╨╛╨╝: {:?}", status.code()))
        }
    }.await;

    result
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
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
