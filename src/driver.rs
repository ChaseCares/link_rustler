use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{anyhow, Context};
use flate2::read::GzDecoder;
use reqwest::Client;
use serde_json::Value;
use tar::Archive;
use thirtyfour::extensions::addons::firefox::FirefoxTools;
use thirtyfour::{FirefoxCapabilities, WebDriver};
use tokio::time::sleep;
use tracing::{error, info, instrument, warn};
use url::Url;

use crate::structs::{self, Config};

#[instrument]
async fn github_api_request(
    github_username: &String,
    repo_owner: &String,
    repo_name: &String,
    client: Option<Client>,
) -> anyhow::Result<Value> {
    let url = format!("https://api.github.com/repos/{repo_owner}/{repo_name}/releases/latest");
    let client = match client {
        Some(client) => client,
        None => Client::builder()
            .user_agent(github_username)
            .build()
            .context("Failed to create HTTP client")?,
    };

    let res = client
        .get(&url)
        .send()
        .await
        .context("Failed to send request to GitHub API")?
        .text()
        .await
        .context("Failed to get response body")?;
    let json: Value = serde_json::from_str(&res).context("Failed to parse JSON response")?;

    Ok(json)
}

#[instrument]
async fn get_extension_github(
    github_username: &String,
    repo_owner: &String,
    extension_name: &String,
    extensions_dir: &String,
) -> anyhow::Result<String> {
    let output_dir = Path::new(extensions_dir).join(extension_name);
    if output_dir.exists() && output_dir.metadata()?.created()?.elapsed()?.as_secs() < 86400 {
        return Ok("Extension was checked within the last 24 hours".to_string());
    }

    let client = Client::builder()
        .user_agent(github_username)
        .build()
        .context("Failed to create HTTP client")?;

    let json = github_api_request(
        github_username,
        repo_owner,
        extension_name,
        Some(client.clone()),
    )
    .await?;
    let asset = json["assets"]
        .as_array()
        .and_then(|assets| {
            assets.iter().find(|asset| {
                asset["name"].as_str().map_or(false, |name| {
                    Path::new(name)
                        .extension()
                        .map_or(false, |ext| ext.eq_ignore_ascii_case("xpi"))
                })
            })
        })
        .ok_or_else(|| anyhow::anyhow!("Failed to find the extension asset"))?;

    let official_name = asset["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Failed to get extension name"))?;

    let output_file = output_dir.join(official_name);
    if output_file.exists() {
        return Ok("Extension already exists".to_string());
    } else if output_dir.exists() {
        fs::remove_dir_all(&output_dir).context("Failed to remove existing output directory")?;
    }

    fs::create_dir_all(&output_dir).context("Failed to create output directory")?;
    let extension_url = asset["browser_download_url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Failed to get extension URL"))?;
    let mut response = client
        .get(extension_url)
        .send()
        .await
        .context("Failed to download extension")?;

    let mut file = File::create(output_file).context("Failed to create output file")?;
    while let Some(chunk) = response
        .chunk()
        .await
        .context("Failed to read response chunk")?
    {
        file.write_all(&chunk)
            .context("Failed to write to output file")?;
    }
    info!("Extension downloaded successfully");
    return Ok("Extension downloaded".to_string());
}

#[instrument]
pub async fn fire_up_and_setup_the_gecko(config: &Config) -> anyhow::Result<WebDriver> {
    let ip = &config.gecko.ip;
    let port = &config.gecko.port;

    let process = Command::new(config.gecko.binary.clone())
        .arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg(ip)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to spawn geckodriver process")?;

    info!("Gecko process started: {:?}", process.id());
    sleep(Duration::from_secs(1)).await;

    let mut caps = FirefoxCapabilities::new();
    if config.gecko.headless {
        caps.set_headless()?;
    }

    let driver_url = format!("http://{ip}:{port}");
    let driver = WebDriver::new(&driver_url, caps)
        .await
        .context("Failed to create WebDriver instance")?;
    driver
        .set_window_rect(0, 0, config.gecko.width, config.gecko.height)
        .await
        .context("Failed to set window rectangle")?;
    driver
        .set_page_load_timeout(config.gecko.page_load_timeout)
        .await
        .context("Failed to set page load timeout")?;
    driver
        .set_script_timeout(config.gecko.script_timeout)
        .await
        .context("Failed to set script timeout")?;

    if let Some(extensions) = &config.extensions {
        let pwd = std::env::current_dir()?;
        for extension in extensions {
            let extensions_dir = format!(
                "{}/{}/{}",
                config.dirs.base_dir, config.dirs.project_subdir, config.dirs.extensions_subdir
            );

            if let Some(username) = &config.github_username {
                match get_extension_github(
                    username,
                    &extension.repo,
                    &extension.name,
                    &extensions_dir,
                )
                .await
                {
                    Ok(msg) => info!("{msg}"),
                    Err(e) => error!("{e:?}"),
                }
            }

            let extension_path = Path::new(&extensions_dir).join(&extension.name);
            let file = fs::read_dir(&extension_path)?
                .filter_map(|entry| entry.ok().map(|e| e.path()))
                .find(|path| path.extension().map_or(false, |ext| ext == "xpi"))
                .ok_or_else(|| anyhow!("No .xpi file found in {extension_path:?}"))
                .context("Failed to find XPI file")?;

            let absolute_extension_path =
                format!("{}/{}", pwd.display(), file.strip_prefix("./")?.display());
            info!("Installing extension: {absolute_extension_path}");

            let tools = FirefoxTools::new(driver.handle.clone());
            tools
                .install_addon(&absolute_extension_path, Some(false))
                .await
                .context("Failed to install extension")?;
        }
    }

    Ok(driver)
}

#[instrument]
pub async fn download_gecko(
    config_dirs: &structs::Storage,
    config_gecko: &structs::GeckoConfig,
) -> anyhow::Result<()> {
    let gecko_tar_gz_path = format!(
        "{}/geckodriver.{}.tar.gz",
        config_dirs.base_dir, config_gecko.version
    );

    if !Path::new(&gecko_tar_gz_path).exists() && config_gecko.location == "default" {
        download_and_extract_gecko(&gecko_tar_gz_path, config_dirs, config_gecko).await?;
        verify_geckodriver_version(config_dirs, config_gecko)?;
    } else if config_gecko.location == "custom" && !Path::new(&config_gecko.binary).exists() {
        error!(
            "Custom geckodriver binary not found: {:?}",
            config_gecko.binary
        );
    }

    Ok(())
}

pub async fn download_and_extract_gecko(
    gecko_tar_gz_path: &str,
    config_dirs: &structs::Storage,
    config_gecko: &structs::GeckoConfig,
) -> anyhow::Result<()> {
    let gecko_binary_url = format!(
        "https://github.com/mozilla/geckodriver/releases/download/v{}/geckodriver-v{}-{}.tar.gz",
        config_gecko.version, config_gecko.version, config_gecko.arch
    );

    let client = Client::new();
    let binary_res = client.get(&gecko_binary_url).send().await?;

    let mut file = File::create(gecko_tar_gz_path).context("Failed to create geckodriver file")?;
    file.write_all(&binary_res.bytes().await?)?;

    let tar_gz = File::open(gecko_tar_gz_path).context("Failed to open geckodriver file")?;
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);
    archive.unpack(config_dirs.base_dir.clone())?;

    Ok(())
}

pub fn verify_geckodriver_version(
    config_dirs: &structs::Storage,
    config_gecko: &structs::GeckoConfig,
) -> anyhow::Result<()> {
    let out = Command::new(format!("{}/{}", config_dirs.base_dir, config_gecko.binary))
        .arg("--version")
        .stdout(Stdio::piped())
        .spawn()
        .context("Failed to spawn geckodriver process")?
        .wait_with_output()
        .context("Failed to get geckodriver version")?;

    let stdout = String::from_utf8(out.stdout).context("Failed to get stdout")?;

    if stdout.contains(&config_gecko.version) {
        info!("Geckodriver downloaded and run successfully, output: {stdout:?}");
    } else {
        error!("Geckodriver version mismatch: {:?}", stdout);
    }

    Ok(())
}

pub async fn stop_geckos() {
    let _ = Command::new("killall").arg("geckodriver").spawn();
    sleep(Duration::from_secs(1)).await;
}

pub async fn new_tab(driver: WebDriver, url: &str) -> anyhow::Result<WebDriver> {
    let handle = driver.new_tab().await.context("Failed to create new tab")?;
    driver
        .switch_to_window(handle.clone())
        .await
        .context("Failed to switch to new tab")?;

    match driver.goto(url).await {
        Ok(()) => {}
        Err(thirtyfour::error::WebDriverError::CmdError(
            thirtyfour::fantoccini::error::CmdError::Standard(e),
        )) => {
            if "insecure certificate" == e.error() {
                warn!("CmdError::Standard insecure certificate: {e}, URL: {url}");
            } else if "timeout" == e.error() {
                info!("CmdError::Standard Timeout: <common>");
            } else {
                warn!(
                    "CmdError::Standard error: {e}, e.error(): {}, URL: {url}",
                    e.error()
                );
            }
        }
        Err(e) => {
            warn!("WebDriverError error: {e}, URL: {url}");
        }
    }

    // pfizeroncologytogether changes the name of the window after the page has loaded
    if url.contains("pfizeroncologytogether") {
        sleep(Duration::from_secs(5)).await;
    }

    // Setting the name must come after the goto
    driver
        .set_window_name(url)
        .await
        .context("Failed to set window name")?;

    info!("New tab successfully created and navigated to {}", url);
    Ok(driver)
}

pub async fn safely_close_window(driver: &WebDriver, url: &Url) -> anyhow::Result<()> {
    driver
        .switch_to_named_window(url.as_str())
        .await
        .with_context(|| format!("Failed to switch to window named: {url}"))?;

    driver
        .close_window()
        .await
        .with_context(|| "Failed to close window")?;

    // Prevents NoSuchWindow error after a window has been closed
    let handles = driver
        .windows()
        .await
        .with_context(|| "Failed to get window handles")?;

    if let Some(handle) = handles.first() {
        driver
            .switch_to_window(handle.clone())
            .await
            .with_context(|| "Failed to switch to main window")?;
    } else {
        error!("No window handles found after closing the window");
    }

    info!("Window closed safely for URL: {}", url);

    Ok(())
}
