#![deny(
    clippy::all,
    clippy::pedantic,
    missing_debug_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    unused_results
)]

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{anyhow, Context};
use chrono::Utc;
use clap::Parser;
use reqwest::{Client, Url};
use serde_json::Value;
use thirtyfour::extensions::addons::firefox::FirefoxTools;
use thirtyfour::{FirefoxCapabilities, WebDriver};
use tokio::time::{sleep, Instant};
use tracing::{error, info, instrument, warn};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use structs::{ActivePages, Config, CustomError, LinkType, PageData, State};

mod common;
mod config;
mod report;
mod structs;

fn storage(clean_start: bool, config: &Config) {
    let project_dir = &format!(
        "./{}/{}",
        &config.dirs.base_dir, &config.dirs.project_subdir
    );

    if clean_start {
        if let Err(err) = fs::remove_dir_all(project_dir) {
            error!("Failed to remove {:?}: {:?}", project_dir, err);
        } else {
            info!("Removed project directory: {:?}", project_dir);
        }
    }

    let dirs = [
        &format!("{}/{}", project_dir, &config.dirs.pages_subdir),
        &format!("{}/{}", project_dir, &config.dirs.temp_subdir),
    ];

    for dir in &dirs {
        if let Err(err) = fs::create_dir_all(dir) {
            error!("Failed to create {dir:?}: {err:?}");
        } else {
            info!("Created directory: {dir:?}");
        }
    }
}

#[instrument]
async fn get_pdf_github(url: Url) -> anyhow::Result<String> {
    let client = Client::new();

    let res = client
        .get(url.clone())
        .send()
        .await
        .context("Failed to send request to GitHub API")?
        .text()
        .await
        .context("Failed to get response body")?;

    let json: Value = serde_json::from_str(&res).context("Failed to parse JSON response")?;

    let repo_owner = json["payload"]["repo"]["ownerLogin"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Failed to get repository owner"))?;

    let repo_name = json["payload"]["repo"]["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Failed to get repository name"))?;

    let items = json["payload"]["tree"]["items"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Failed to get PDF items"))?;

    let pdfs: Vec<&Value> = items
        .iter()
        .filter(|item| {
            item["path"].as_str().map_or(false, |path| {
                Path::new(path)
                    .extension()
                    .map_or(false, |ext| ext.eq_ignore_ascii_case("pdf"))
            })
        })
        .collect();

    if pdfs.len() != 1 {
        return Err(anyhow::anyhow!("Expected 1 PDF, found {}", pdfs.len()));
    }

    let pdf_path = pdfs[0]["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Failed to get PDF path"))?;

    let pdf_url = format!("https://github.com/{repo_owner}/{repo_name}/raw/main/{pdf_path}");

    let pdf = client
        .get(&pdf_url)
        .send()
        .await
        .context("Failed to download PDF")?
        .text()
        .await
        .context("Failed to read PDF content")?;

    info!("Successfully retrieved PDF from GitHub");

    Ok(pdf)
}

#[instrument]
async fn get_extension_github(
    github_username: &String,
    repo: &String,
    extension_name: &String,
    extensions_dir: &String,
) -> anyhow::Result<String> {
    let url = format!("https://api.github.com/repos/{repo}/{extension_name}/releases/latest");

    let output_dir = Path::new(extensions_dir).join(extension_name);
    if output_dir.exists() && output_dir.metadata()?.created()?.elapsed()?.as_secs() < 86400 {
        return Ok("Extension was checked within the last 24 hours".to_string());
    }

    let client = Client::builder()
        .user_agent(github_username)
        .build()
        .context("Failed to create HTTP client")?;
    let res = client
        .get(&url)
        .send()
        .await
        .context("Failed to send request to GitHub API")?
        .text()
        .await
        .context("Failed to get response body")?;
    let json: Value = serde_json::from_str(&res).context("Failed to parse JSON response")?;

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

fn pdf_contents(pdf_path: &str) -> anyhow::Result<Vec<u8>> {
    let path = Path::new(pdf_path);
    let mut buf = Vec::new();

    let mut file =
        File::open(path).with_context(|| format!("Failed to open PDF file: {pdf_path}"))?;

    let _ = file
        .read_to_end(&mut buf)
        .with_context(|| format!("Failed to read PDF file: {pdf_path}"))?;

    info!("PDF contents read successfully from: {}", pdf_path);
    Ok(buf)
}

async fn download_file(url: String) -> Option<String> {
    let client = Client::new();
    let res = client.get(url).send().await.unwrap().text().await;
    sleep(Duration::from_secs(1)).await;

    match res {
        Ok(_) => Some(res.unwrap()),
        Err(_) => None,
    }
}

fn load_data_store(path_str: &str) -> anyhow::Result<BTreeMap<Url, PageData>> {
    let data_store_path = Path::new(path_str);

    if data_store_path.exists() {
        let mut file = File::open(data_store_path)
            .with_context(|| format!("Failed to open hash file: {path_str}"))?;
        let mut contents = String::new();
        let _ = file
            .read_to_string(&mut contents)
            .with_context(|| format!("Failed to read hash file: {path_str}"))?;
        let data_store = serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse hash file: {path_str}"))?;
        Ok(data_store)
    } else {
        info!("Data store path does not exist: {}", path_str);
        Ok(BTreeMap::new())
    }
}

#[instrument]
fn save_data_store(
    page_datas: &BTreeMap<Url, PageData>,
    path_str: &str,
) -> anyhow::Result<(), anyhow::Error> {
    let data_store_path = Path::new(path_str);
    let mut data_store_file = if data_store_path.exists() {
        OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(data_store_path)
            .with_context(|| format!("Failed to open file at {data_store_path:?}"))?
    } else {
        File::create(data_store_path)
            .with_context(|| format!("Failed to create file at {data_store_path:?}"))?
    };

    let serialized = serde_json::to_string_pretty(&page_datas)
        .map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to serialize HashMap: {e}"),
            )
        })
        .context("Failed to serialize page data")?;

    data_store_file
        .write_all(serialized.as_bytes())
        .with_context(|| "Failed to write serialized data to file")?;

    Ok(())
}

#[instrument]
fn check_link_type(url: &Url) -> anyhow::Result<LinkType> {
    let url_string = url.to_string();

    let link_type = if Path::new(&url_string)
        .extension()
        .map_or(false, |ext| ext.eq_ignore_ascii_case("docx"))
    {
        LinkType::Content
    } else if url_string.starts_with("http") {
        LinkType::Generic
    } else if url_string.contains("/User") || url_string.starts_with("file://") {
        LinkType::Local
    } else if url_string.starts_with("mailto:") {
        LinkType::Mailto
    } else {
        LinkType::Unknown
    };

    Ok(link_type)
}

async fn get_urls(
    pdf_path: Option<String>,
    external_source_url: Option<Url>,
    given_urls: Option<Vec<String>>,
) -> anyhow::Result<HashSet<Url>> {
    let urls_to_check: HashSet<Url> = if let Some(given_urls) = given_urls {
        given_urls
            .iter()
            .map(|url| Url::parse(url))
            .filter_map(Result::ok)
            .collect()
    } else if let Some(pdf_path) = pdf_path {
        let pdf = pdf_contents(&pdf_path)?;
        get_unique_links(&pdf)
    } else {
        let pdf = get_pdf_github(external_source_url.unwrap())
            .await
            .with_context(|| "Failed to fetch PDF from GitHub")?
            .as_bytes()
            .to_vec();
        get_unique_links(&pdf)
    };

    if urls_to_check.is_empty() {
        anyhow::bail!("No links found in PDF");
    }

    info!("Total number of links: {:?}", urls_to_check.len());

    Ok(urls_to_check)
}

fn get_unique_links(raw_pdf: &[u8]) -> HashSet<Url> {
    let re_bytes = regex::bytes::Regex::new(r"/Type/Action/S/URI/URI\((.*?)\)").unwrap();
    let raw_links: HashSet<Url> = re_bytes
        .captures_iter(raw_pdf)
        .map(|capture| {
            std::str::from_utf8(capture.get(1).unwrap().as_bytes()).expect("Invalid UTF-8")
        })
        .map(Url::parse)
        .filter_map(Result::ok)
        .collect();
    raw_links
}

#[instrument]
async fn fire_up_and_setup_the_gecko(config: &Config) -> anyhow::Result<WebDriver> {
    let ip = &config.gecko.ip;
    let port = config.gecko.port;

    let process = Command::new("geckodriver")
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
                "./{}/{}/{}",
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

async fn stop_geckos() {
    let _ = Command::new("killall").arg("geckodriver").spawn();
    sleep(Duration::from_secs(1)).await;
}

async fn new_tab(driver: WebDriver, url: &str) -> anyhow::Result<WebDriver> {
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

    // Setting the name must come after the goto
    driver
        .set_window_name(url)
        .await
        .context("Failed to set window name")?;

    info!("New tab successfully created and navigated to {}", url);
    Ok(driver)
}

async fn safely_close_window(driver: &WebDriver, url: &Url) -> anyhow::Result<()> {
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

fn title_check(title: &str) -> Result<(), CustomError> {
    if title.contains("404") || title.contains("Not Found") {
        return Err(CustomError::PageNotFound);
    }
    if title.contains("Warning") {
        return Err(CustomError::Warning);
    }
    if title.contains("Error") || title.contains("Unable to") || title.contains("Problem") {
        return Err(CustomError::PageError);
    }

    Ok(())
}

fn save_page_data(
    url: &Url,
    config: &Config,
    page_source: &str,
    img: &image::DynamicImage,
) -> anyhow::Result<()> {
    let now = Utc::now();
    let url_hash = common::hash_string(&url.to_string());
    let save_data_path = Path::new(&config.dirs.base_dir)
        .join(&config.dirs.project_subdir)
        .join(&config.dirs.pages_subdir)
        .join(url_hash.clone());

    if !save_data_path.exists() {
        fs::create_dir_all(&save_data_path)
            .with_context(|| format!("Failed to create directory: {:?}", &save_data_path))?;
    }

    let mut remove_files = Vec::new();

    if let Ok(old_files) = fs::read_dir(&save_data_path) {
        let files: Vec<_> = old_files
            .filter_map(Result::ok)
            .filter(|entry| {
                let path = entry.path();
                path.extension()
                    .map_or(false, |ext| ext == "html" || ext == "png")
            })
            .collect();

        remove_files = files
            .into_iter()
            .skip(config.num_of_local_pages)
            .map(|e| e.path())
            .collect();
    }

    for file in &remove_files {
        if let Err(err) = fs::remove_file(file) {
            error!("Failed to remove file: {:?}. Error: {:?}", file, err);
        }
    }

    let page_file_name = format!("page_{now:?}.html");
    let screenshot_file_name = format!("screenshot_{now:?}.png");

    let page_file_path = save_data_path.join(page_file_name);
    let screenshot_file_path = save_data_path.join(screenshot_file_name);

    File::create(&page_file_path)
        .with_context(|| format!("Failed to create file: {:?}", &page_file_path))?
        .write_all(page_source.as_bytes())
        .with_context(|| format!("Failed to write to file: {:?}", &page_file_path))?;

    img.save(&screenshot_file_path)
        .with_context(|| format!("Failed to save screenshot: {:?}", &screenshot_file_path))?;

    info!("Page data saved successfully for URL: {}", url);

    Ok(())
}

async fn check_link(
    driver: &WebDriver,
    url: &Url,
    marker: Option<&String>,
    config: &Config,
    linktype: LinkType,
) -> State {
    match linktype {
        LinkType::Generic => {
            if let Err(err) = driver.switch_to_named_window(url.as_str()).await {
                warn!("Failed to switch to window: {err:?}");
                sleep(Duration::from_secs(1000)).await;
            }

            let title = driver.title().await.unwrap_or_default();
            let raw_page_source = driver.source().await.unwrap_or_default();

            // Remove unique value from cookies extension
            let rx_filter = regex::Regex::new(r" [a-z]* idc0_343").unwrap();
            let page_source = rx_filter.replace_all(&raw_page_source, "");
            let new_ss = driver.screenshot_as_png().await.unwrap_or_default();
            let img = image::load_from_memory(&new_ss).unwrap_or_default();

            if config.keep_local_records {
                if let Err(err) = save_page_data(url, config, &page_source, &img) {
                    panic!("Failed to save page data: {err:?}");
                }
            }

            let mut error = None;
            if common::hash_img(&img) == *"AAAAAAAAAAA" {
                error = Some(CustomError::BadScreenshot);
            }

            if let Some(marker) = marker {
                if !page_source.contains(marker) {
                    error = Some(CustomError::MarkerNotFound);
                }
            }

            if let Err(e) = title_check(&title) {
                error = Some(e);
            }

            if url.as_str() != driver.current_url().await.unwrap().as_str() {
                error = Some(CustomError::Redirected);
            }

            State::new(
                &page_source,
                Some(img),
                Some(title),
                LinkType::Generic,
                error,
            )
        }

        LinkType::Content => {
            let content = download_file(url.to_string()).await.unwrap_or_default();
            State::new(&content, None, None, LinkType::Content, None)
        }

        LinkType::Local => State::new(
            "",
            None,
            None,
            LinkType::Local,
            Some(CustomError::LinkTypeLocal),
        ),
        LinkType::Mailto => State::new(
            "",
            None,
            None,
            LinkType::Mailto,
            Some(CustomError::LinkTypeMailto),
        ),
        LinkType::Unknown => State::new(
            "",
            None,
            None,
            LinkType::Unknown,
            Some(CustomError::UnknownLinkType),
        ),
        LinkType::InternalError => State::new(
            "",
            None,
            None,
            LinkType::InternalError,
            Some(CustomError::WebDriverError),
        ),
    }
}

async fn check_links(
    mut driver: WebDriver,
    urls: HashSet<Url>,
    page_datas: BTreeMap<Url, PageData>,
    config: &Config,
) -> anyhow::Result<Vec<(Url, State)>> {
    let mut url_in_waiting: Vec<ActivePages> = Vec::new();
    let mut results = Vec::new();

    for url in &urls {
        let linktype = match check_link_type(url) {
            Ok(linktype) => linktype,
            Err(e) => {
                error!("Failed to check link type: {e:?}");
                LinkType::InternalError
            }
        };

        if linktype == LinkType::Generic {
            info!("Loading link: {}", url.as_str());
            driver = new_tab(driver, url.as_str()).await?;
            url_in_waiting.push(ActivePages {
                url: url.clone(),
                time_added: Instant::now(),
                linktype,
            });

            // Removing links significantly decreases ram usage
            if !url_in_waiting.is_empty()
                && url_in_waiting[0].time_added + config.page_dwell_time < Instant::now()
            {
                let url = url_in_waiting.remove(0).url;
                info!("Removing {} from waiting list", url.as_str());

                let marker = page_datas
                    .get(&url)
                    .map(|page_data| page_data.marker())
                    .unwrap();

                let state = check_link(&driver, &url, marker, config, linktype).await;
                safely_close_window(&driver, &url).await?;
                results.push((url, state));
            }
        } else {
            results.push((
                url.clone(),
                check_link(&driver, url, None, config, linktype).await,
            ));
        }
    }

    for ActivePages {
        url,
        time_added,
        linktype,
    } in url_in_waiting.drain(..)
    {
        sleep(config.page_dwell_time.saturating_sub(time_added.elapsed())).await;
        info!("Removing {} from waiting list", url.as_str());

        let marker = page_datas
            .get(&url)
            .map(|page_data| page_data.marker())
            .unwrap();

        let state = check_link(&driver, &url, marker, config, linktype).await;
        safely_close_window(&driver, &url).await?;
        results.push((url, state));
    }

    driver.quit().await?;
    Ok(results)
}

#[instrument(skip(config))]
async fn link_checker(config: &Config, urls: Option<Vec<String>>) -> anyhow::Result<()> {
    stop_geckos().await;

    let datastore_path = &format!(
        "./{}/{}/{}",
        config.dirs.base_dir, config.dirs.project_subdir, config.dirs.data_store
    );
    let mut page_datas = load_data_store(datastore_path).context("Failed to load data store")?;

    let urls_to_check = get_urls(config.pdf_path.clone(), config.url.clone(), urls)
        .await
        .context("Failed to get URLs to check")?;

    let driver = match fire_up_and_setup_the_gecko(config).await {
        Ok(driver) => driver,
        Err(e) => return Err(anyhow::anyhow!("Error: {:?}", e)),
    };

    let results = check_links(driver, urls_to_check, page_datas.clone(), config)
        .await
        .context("Failed to check links")?;

    for (url, state) in results {
        if let std::collections::btree_map::Entry::Vacant(e) = page_datas.entry(url.clone()) {
            let _ = e.insert(PageData::new(
                state,
                common::hash_string(&url.to_string()),
                None,
            ));
        } else if let Some(page_data) = page_datas.get_mut(&url) {
            page_data.update(state);
        }
    }

    let datastore_path = &format!(
        "./{}/{}/{}",
        config.dirs.base_dir, config.dirs.project_subdir, config.dirs.data_store
    );
    save_data_store(&page_datas, datastore_path).context("Failed to save data store")?;

    stop_geckos().await;

    info!("Link checking completed successfully");

    Ok(())
}

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    pdf_path: Option<String>,

    #[arg(short, long)]
    config_path: Option<String>,

    #[arg(long)]
    clean_start: bool,

    #[arg(long)]
    check_this_url: Option<Vec<String>>,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let start = Instant::now();
    let args = Args::parse();
    let config = match config::config(&args) {
        Ok(config) => config,
        Err(e) => return Err(anyhow::anyhow!("Error: {e:?}")),
    };

    if config.check_links {
        storage(args.clean_start, &config);

        match link_checker(&config, args.check_this_url).await {
            Ok(()) => {}
            Err(e) => println!("Error: {e:?}"),
        }
    }

    if config.gen_report {
        report::gen_post_run_report(&config);
        let report_path = format!(
            "{}/{}/{}",
            config.dirs.base_dir, config.dirs.project_subdir, config.dirs.report
        );
        match open::that(&report_path) {
            Ok(()) => {
                info!("Report opened successfully");
            }
            Err(e) => {
                info!("Failed to auto open report, error: {e:?}. Report path: {report_path:?}");
            }
        }
    }

    let duration = start.elapsed();
    info!(
        "Finished in {} minutes {} seconds.",
        duration.as_secs() / 60,
        duration.as_secs() % 60
    );

    Ok(())
}
