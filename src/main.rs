mod settings;
use settings::{load_settings, save_settings, LauncherSettings};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct VersionManifest {
    latest: LatestVersion,
    versions: Vec<VersionInfo>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LatestVersion {
    release: String,
    snapshot: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct VersionInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
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
    size: u64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Library {
    name: String,
    downloads: Option<LibraryDownloads>,
    rules: Option<Vec<Rule>>,
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
    let mut path = dirs::data_dir().expect("Ne vdaloś vyznachyty systemnu papku AppData/Home");
    path.push(".minecraft");
    path
}

async fn download_file(client: &reqwest::Client, url: &str, target_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mc_dir = get_minecraft_dir();
    let libraries_dir = mc_dir.join("libraries");
    let natives_dir = mc_dir.join("versions").join("natives_temp");
    let http_client = reqwest::Client::new();

    let manifest_url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
    let manifest: VersionManifest = http_client.get(manifest_url).send().await?.json().await?;

    let target_version_id = &manifest.latest.release;
    let version_info = manifest
        .versions
        .iter()
        .find(|v| &v.id == target_version_id)
        .expect("Versiiu ne znajdeno u manifesti");

    let version_dir = mc_dir.join("versions").join(target_version_id);
    let version_json_path = version_dir.join(format!("{}.json", target_version_id));
    let version_jar_path = version_dir.join(format!("{}.jar", target_version_id));

    download_file(&http_client, &version_info.url, &version_json_path).await?;

    let version_json_content = fs::read_to_string(&version_json_path)?;
    let version_data: VersionData = serde_json::from_str(&version_json_content)?;

    download_file(&http_client, &version_data.downloads.client.url, &version_jar_path).await?;

    let mut classpath_entries = vec![version_jar_path.to_string_lossy().to_string()];

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

    let asset_index_path = mc_dir.join("assets").join("indexes").join(format!("{}.json", version_data.asset_index.id));
    download_file(&http_client, &version_data.asset_index.url, &asset_index_path).await?;

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

    let username = "Player123";
    let offline_uuid = Uuid::new_v3(&Uuid::NAMESPACE_DNS, format!("OfflinePlayer:{}", username).as_bytes())
        .simple()
        .to_string();

    let assets_dir = mc_dir.join("assets");
    let main_class = version_data.main_class.unwrap_or_else(|| "net.minecraft.client.main.Main".to_string());

    let mut cmd = Command::new("java");
    cmd.arg(format!("-Djava.library.path={}", natives_dir.to_string_lossy()))
        .arg("-Dfile.encoding=UTF-8")
        .arg("-cp")
        .arg(classpath)
        .arg(main_class)
        .arg("--username").arg(username)
        .arg("--version").arg(target_version_id)
        .arg("--gameDir").arg(&mc_dir)
        .arg("--assetsDir").arg(&assets_dir)
        .arg("--assetIndex").arg(version_data.asset_index.id)
        .arg("--uuid").arg(offline_uuid)
        .arg("--accessToken").arg("0")
        .arg("--userType").arg("legacy");

    let mut child = cmd.spawn()?;
    child.wait()?;

    Ok(())
}
