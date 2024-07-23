#![warn(
    clippy::all,
    unsafe_code,
    unused_extern_crates,
    // slint forces these to be disabled :(
    // unused_results
    // unused_import_braces,
    // unused_qualifications,
    // clippy::pedantic,
    // missing_debug_implementations,
    // trivial_casts,
    // trivial_numeric_casts,
)]

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    rc::Rc,
    sync::OnceLock,
    time::Duration,
};

use anyhow::Context;
use clap::Parser;
use common::init_tracing;
use directories::ProjectDirs;
use driver::new_tab;
use reqwest::{Client, Url};
use slint::ComponentHandle;
use thirtyfour::WebDriver;
use tokio::time::{sleep, Instant};
use tracing::{error, info, instrument, warn};

slint::include_modules!();

use enums::{CustomError, LinkType, Locations};
use structs::{ActivePages, AppState, Args, Config, PageData, State};

mod common;
mod config;
mod disc_op;
mod driver;
mod enums;
mod report;
mod structs;
mod update;

#[instrument]
async fn get_pdf_github(url: Url) -> anyhow::Result<String> {
    let client = Client::new();

    let split_path = url.path().split('/').collect::<Vec<&str>>();

    let repo_owner = split_path[1];
    let repo_name = split_path[2];
    let branch = split_path[4];
    let file_path = split_path[5..].join("/");

    let pdf_url = format!("https://github.com/{repo_owner}/{repo_name}/raw/{branch}/{file_path}");

    let pdf = client
        .get(&pdf_url)
        .send()
        .await
        .context("Failed to download PDF")?
        .text()
        .await
        .context("Failed to read PDF content")?;

    info!("PDF fetched successfully from: {}", pdf_url);

    Ok(pdf)
}

fn pdf_contents(pdf_path: &str) -> anyhow::Result<Vec<u8>> {
    let path = Path::new(pdf_path);
    let mut buf = Vec::new();

    let mut file = File::open(path).context(format!("Failed to open PDF file: {pdf_path}"))?;

    let _ = file
        .read_to_end(&mut buf)
        .context(format!("Failed to read PDF file: {pdf_path}"))?;

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
            .context("Failed to fetch PDF from GitHub")?
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

fn get_unique_links(pdf: &[u8]) -> HashSet<Url> {
    let re_bytes = regex::bytes::Regex::new(r"/Type/Action/S/URI/URI\((.*?)\)").unwrap();
    let raw_links: HashSet<Url> = re_bytes
        .captures_iter(pdf)
        .map(|capture| {
            std::str::from_utf8(capture.get(1).unwrap().as_bytes()).expect("Invalid UTF-8")
        })
        .map(Url::parse)
        .filter_map(Result::ok)
        .collect();
    raw_links
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
            }

            let title = driver.title().await.unwrap_or_default();
            let raw_page_source = driver.source().await.unwrap_or_default();

            // Remove unique value from cookies extension
            let rx_filter = regex::Regex::new(r" [a-z]* idc0_343").unwrap();
            let page_source = rx_filter.replace_all(&raw_page_source, "");
            let new_ss = driver.screenshot_as_png().await.unwrap_or_default();
            let img = image::load_from_memory(&new_ss).unwrap_or_default();

            if config.keep_local_records {
                if let Err(err) = disc_op::save_page_data(url, config, &page_source, &img) {
                    panic!("Failed to save page data: {err:?}"); // TODO: Replace with proper error handling
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
                    .unwrap_or_default();

                let state = check_link(&driver, &url, marker, config, linktype).await;
                driver::safely_close_window(&driver, &url).await?;
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
            .unwrap_or_default();

        let state = check_link(&driver, &url, marker, config, linktype).await;
        driver::safely_close_window(&driver, &url).await?;
        results.push((url, state));
    }

    driver.quit().await?;
    Ok(results)
}

#[instrument(skip(config))]
async fn link_checker(config: &Config, urls: Option<Vec<String>>) -> anyhow::Result<()> {
    driver::stop_geckos().await;

    let datastore_path = get_loc(Locations::DataStore);
    let mut page_datas =
        disc_op::load_data_store(&datastore_path).context("Failed to load data store")?;

    let urls_to_check = get_urls(config.pdf_path.clone(), config.pdf_url.clone(), urls)
        .await
        .context("Failed to get URLs to check")?;

    let driver = match driver::fire_up_and_setup_the_gecko(config).await {
        Ok(driver) => driver,
        Err(e) => return Err(anyhow::anyhow!(e)),
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

    let datastore_path = get_loc(Locations::DataStore);
    disc_op::save_data_store(&page_datas, &datastore_path).context("Failed to save data store")?;

    driver::stop_geckos().await;

    info!("Link checking completed successfully");

    Ok(())
}

static PROJECT_NS: OnceLock<Option<ProjectDirs>> = OnceLock::new();

fn get_loc(loc: Locations) -> PathBuf {
    if let Some(dirs) =
        PROJECT_NS.get_or_init(|| ProjectDirs::from("dev", "chasecares", "link_rustler"))
    {
        match loc {
            Locations::BaseConfig => dirs.config_dir().to_path_buf(),
            Locations::BaseData => dirs.data_dir().to_path_buf(),
            Locations::Config => dirs.config_dir().join("config.toml"),
            Locations::Report => dirs.data_dir().join("report.html"),
            Locations::DataStore => dirs.data_dir().join("data_store.json"),
            Locations::ExtensionsDir => dirs.data_dir().join("extensions"),
            Locations::PagesSubdir => dirs.data_dir().join("pages"),
            Locations::GeckodriverBinary => dirs.data_dir().join("geckodriver"),
            Locations::LogDir => dirs.data_dir().join("logs"),
            Locations::LogPrefix => PathBuf::from("log_file.txt"),
        }
    } else {
        panic!("Failed to get project directories");
    }
}

static ARCHITECTURE: OnceLock<&str> = OnceLock::new();
static OPERATING_SYSTEM: OnceLock<&str> = OnceLock::new();

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let _guard = init_tracing();

    let args = Args::parse();

    let ui = MainWindow::new()?;
    let app_state = Rc::new(RefCell::new(AppState::new()));

    let config = match config::load(&ui, &mut app_state.borrow_mut()) {
        Ok(config) => Rc::new(RefCell::new(config)),
        Err(e) => return Err(anyhow::anyhow!(e.to_string())),
    };

    disc_op::init_storage(args.clean_start);

    let ui_weak = ui.as_weak();
    ui.global::<UpdateCheck>().on_self_check_update({
        let app_state = app_state.clone();

        if args.check_for_update {
            update::helper(&ui, &mut app_state.borrow_mut());
        } else {
            warn!("Automatic update checking is disabled.");
            app_state
                .borrow_mut()
                .add_to_self_update_log("Automatic update checking is disabled.", &ui);
        }

        move || {
            if let Some(ui) = ui_weak.upgrade() {
                update::helper(&ui, &mut app_state.borrow_mut());
            }
        }
    });

    let ui_weak = ui.as_weak();
    ui.global::<UpdateCheck>().on_geckodriver_check_update({
        let app_state = app_state.clone();

        let rc_config = Rc::clone(&config);
        let config_gecko = rc_config.borrow().gecko.clone();
        match driver::download_gecko( &config_gecko).await {
            Ok(()) => {
                app_state
                    .borrow_mut()
                    .add_to_geckodriver_update_log("Geckodriver is up to date.", &ui);
                ui.global::<Globals>().set_link_check_can_run(true);
                info!("Geckodriver is up to date.")
            }
            Err(e) => {
                app_state
                    .borrow_mut()
                    .add_to_geckodriver_update_log(&e.to_string(),  &ui);
                ui.global::<Globals>().set_link_check_can_run(false);
                error!("{e:?}")
            }
        }

        move || {
            if let Some(ui) = ui_weak.upgrade() {
                app_state
                    .borrow_mut()
                    .add_to_geckodriver_update_log("Not yet implemented, go to https://github.com/mozilla/geckodriver/releases/latest to check :)", &ui);
            }
        }
    });

    let ui_weak = ui.as_weak();
    ui.global::<Settings>().on_update_config_value({
        let rc_config = Rc::clone(&config);

        move |key, value| {
            if let Some(ui) = ui_weak.upgrade() {
                ui.global::<Settings>().set_config_saved(false);
                info!("value: {:?}", value);
                match rc_config.borrow_mut().update(&key, &value) {
                    Ok(()) => "".to_string().into(),
                    Err(e) => {
                        error!("{e:?}");
                        e.to_string().to_uppercase().into()
                    }
                }
            } else {
                "Unreachable?".to_string().into()
            }
        }
    });

    let ui_weak = ui.as_weak();
    ui.global::<Settings>().on_write_config({
        let rc_config = Rc::clone(&config);
        let app_state = app_state.clone();

        move || {
            if let Some(ui) = ui_weak.upgrade() {
                let config = rc_config.borrow();

                match config::write_config_file(&config, &get_loc(Locations::Config)) {
                    Ok(()) => {
                        ui.global::<Settings>().set_config_saved(true);
                        app_state
                            .borrow_mut()
                            .add_to_config_log("Config saved successfully.", &ui);
                    }
                    Err(e) => {
                        error!("{e:?}");
                        app_state
                            .borrow_mut()
                            .add_to_config_log("Failed to save config.", &ui);
                    }
                }

                ui.global::<Settings>().set_config_saved(true);
                app_state
                    .borrow_mut()
                    .add_to_config_log("Saved loaded successfully.", &ui);
            }
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_close_window({
        move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.hide().unwrap();
            }
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_run_link_checker({
        move || {
            info!("Running link checker");
            if let Some(ui) = ui_weak.upgrade() {
                let start = Instant::now();
                slint::spawn_local(async move {
                    ui.set_link_checker_running(true);
                    sleep(Duration::from_secs(10)).await;
                    let tokio_runtime = tokio::runtime::Runtime::new().unwrap();
                    let result = tokio_runtime
                        .spawn(async move {
                            // TODO: Use config without having to reload it
                            let config = config::no_ui_load().unwrap();

                            match link_checker(&config, None).await {
                                Ok(()) => {
                                    info!("Link checking completed successfully");
                                }
                                Err(e) => {
                                    anyhow::bail!("{e:?}")
                                }
                            }

                            Ok(())
                        })
                        .await
                        .unwrap();
                    result.unwrap();

                    std::mem::forget(tokio_runtime);
                    let duration = start.elapsed();
                    info!(
                        "Finished in {} minutes {} seconds.",
                        duration.as_secs() / 60,
                        duration.as_secs() % 60
                    );
                    ui.set_link_checker_running(false);
                })
                .unwrap();
            }
        }
    });

    ui.on_gen_report({
        let rc_config = Rc::clone(&config);
        move || {
            let config = rc_config.borrow();
            report::gen_post_run_report(&config);
            let report_path = get_loc(Locations::Report);
            match open::that(&report_path) {
                Ok(()) => {
                    info!("Report opened successfully");
                }
                Err(e) => {
                    info!("Failed to auto open report, error: {e:?}. Report path: {report_path:?}");
                }
            }
        }
    });

    ui.run().unwrap();
    Ok(())
}
