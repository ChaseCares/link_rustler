use std::{io::Write, time::Duration};

use clap::Parser;
use flate2::{write::ZlibEncoder, Compression};
use image_hasher::ImageHash;
use serde::{Deserialize, Serialize};
use slint::ComponentHandle;
use tokio::time::Instant;
use url::Url;

use crate::common::{hash_img, hash_string};
use crate::enums::{CustomError, InvalidReason, LinkType, ValidReason};
use crate::MainWindow;

use crate::{Settings, UpdateCheck};

#[derive(Parser, Debug)]
#[clap(name = "Link Rustler", version = env!("CARGO_PKG_VERSION"), author = "ChaseCares")]
pub struct Args {
    #[arg(long)]
    pub clean_start: bool,

    #[arg(long, default_value = "true")]
    pub check_for_update: bool,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub self_update_log: String,
    pub geckodriver_update_log: String,
    pub config_log: String,
    pub self_update_complete: bool,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            self_update_log: String::new(),
            geckodriver_update_log: String::new(),
            config_log: String::new(),
            self_update_complete: false,
        }
    }

    pub fn add_to_self_update_log(&mut self, message: &str, ui: &MainWindow) {
        self.self_update_log
            .push_str(format!("{}\n", message).as_str());
        ui.global::<UpdateCheck>()
            .set_self_update_log(self.self_update_log.clone().into());
    }
    pub fn add_to_geckodriver_update_log(&mut self, message: &str, ui: &MainWindow) {
        self.geckodriver_update_log
            .push_str(format!("{}\n", message).as_str());
        ui.global::<UpdateCheck>()
            .set_geckodriver_update_log(self.geckodriver_update_log.clone().into());
    }
    pub fn add_to_config_log(&mut self, message: &str, ui: &MainWindow) {
        self.config_log.push_str(format!("{}\n", message).as_str());
        ui.global::<Settings>()
            .set_config_log(self.config_log.clone().into());
    }
}

#[derive(Debug)]
pub struct Mode<T> {
    pub value: Option<T>,
    pub confidence: Option<usize>,
}

#[derive(Debug)]
pub struct DiffReport {
    pub page_hash: Mode<String>,
    pub compression: Mode<usize>,
    pub title: Mode<String>,
    pub screenshot_hash: Mode<String>,
}

#[derive(Debug)]
pub struct ReportTableDataRow {
    pub url: Url,
    pub marker: String,
    pub errors: Option<CustomError>,
    pub invalid_reason: Option<Vec<InvalidReason>>,
    pub valid_reason: Option<Vec<ValidReason>>,
}

#[derive(Debug)]
pub struct Tables {
    pub valid: Vec<ReportTableDataRow>,
    pub unknown: Vec<ReportTableDataRow>,
    pub hash_only: Vec<ReportTableDataRow>,
    pub error: Vec<ReportTableDataRow>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Extensions {
    pub repo: String,
    pub name: String,
}

impl Default for Extensions {
    fn default() -> Self {
        Extensions {
            repo: "OhMyGuus".to_string(),
            name: "I-Still-Dont-Care-About-Cookies".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct GeckoConfig {
    pub version: String,
    pub headless: bool,
    pub width: u32,
    pub height: u32,
    pub ip: String,
    pub port: u16,
    #[serde(with = "humantime_serde")]
    pub page_load_timeout: Duration,
    #[serde(with = "humantime_serde")]
    pub script_timeout: Duration,
}

impl Default for GeckoConfig {
    fn default() -> Self {
        GeckoConfig {
            version: "0.34.0".to_string(),
            headless: true,
            width: 1080,
            height: 2000,
            ip: "127.0.0.1".to_string(),
            port: 4444,
            page_load_timeout: Duration::from_secs(15),
            script_timeout: Duration::from_secs(15),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub github_username: Option<String>,
    pub pdf_url: Option<Url>,
    pub num_of_local_pages: usize,
    pub keep_local_records: bool,
    pub screenshot_diff_confidence: usize,
    pub screenshot_diff_tolerance: u32,
    pub compression_length_tolerance: usize,
    #[serde(with = "humantime_serde")]
    pub page_dwell_time: Duration,
    pub pdf_path: Option<String>,
    pub gecko: GeckoConfig,
    pub extensions: Option<Vec<Extensions>>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            github_username: Some(()).map(|()| "Awesome-Octocat-App".to_string()),
            pdf_url: Some(()).map(|()| Url::parse("https://github.com/").unwrap()),
            pdf_path: None,
            screenshot_diff_confidence: 60,
            screenshot_diff_tolerance: 3,
            compression_length_tolerance: 300,
            keep_local_records: true,
            page_dwell_time: Duration::from_secs(45),
            num_of_local_pages: 2,
            gecko: GeckoConfig::default(),
            extensions: Some(vec![Extensions::default()]),
        }
    }
}

impl Config {
    pub fn update(&mut self, key: &str, value: &str) -> anyhow::Result<()> {
        match key {
            "github_username" => self.github_username = Some(value.to_string()),
            "pdf_url" => self.pdf_url = Some(Url::parse(value)?),
            "num_of_local_pages" => self.num_of_local_pages = value.parse()?,
            "keep_local_records" => self.keep_local_records = value.parse()?,
            "screenshot_diff_confidence" => self.screenshot_diff_confidence = value.parse()?,
            "screenshot_diff_tolerance" => self.screenshot_diff_tolerance = value.parse()?,
            "compression_length_tolerance" => self.compression_length_tolerance = value.parse()?,
            "page_dwell_time" => self.page_dwell_time = Duration::from_secs(value.parse()?),
            "pdf_path" => self.pdf_path = Some(value.to_string()),
            "gecko_version" => self.gecko.version = value.to_string(),
            "gecko_headless" => self.gecko.headless = value.parse()?,
            "gecko_width" => self.gecko.width = value.parse()?,
            "gecko_height" => self.gecko.height = value.parse()?,
            "gecko_page_load_timeout" => {
                self.gecko.page_load_timeout = Duration::from_secs(value.parse()?)
            }
            "gecko_script_timeout" => {
                self.gecko.script_timeout = Duration::from_secs(value.parse()?)
            }
            _ => (),
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct ActivePages {
    pub url: Url,
    pub time_added: Instant,
    pub linktype: LinkType,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct State {
    pub hash: String,
    pub compress_length: usize,
    pub screenshot_hash: Option<String>,
    pub title: Option<String>,
    pub link_type: LinkType,
    pub check_time: chrono::DateTime<chrono::Utc>,
    pub error: Option<CustomError>,
}

impl State {
    pub fn new(
        content: &str,
        screenshot: Option<image::DynamicImage>,
        title: Option<String>,
        link_type: LinkType,
        error: Option<CustomError>,
    ) -> Self {
        let screenshot_hash = screenshot.map(|screenshot| hash_img(&screenshot));

        let mut e = ZlibEncoder::new(Vec::new(), Compression::best());
        e.write_all(content.as_bytes()).unwrap();
        let compressed_bytes = e.finish();
        let compress_length = compressed_bytes.as_ref().unwrap().len();

        State {
            hash: hash_string(&content.to_string()),
            compress_length,
            screenshot_hash,
            title,
            check_time: chrono::Utc::now(),
            link_type,
            error,
        }
    }

    pub fn cal_screenshot_similarity(&self, screenshot_hash: Option<String>) -> Option<u32> {
        if self.screenshot_hash.is_some() {
            let original_screenshot: ImageHash<Box<[u8]>> =
                ImageHash::from_base64(self.screenshot_hash.as_ref().unwrap().as_str()).unwrap();
            let new_screenshot: ImageHash<Box<[u8]>> =
                ImageHash::from_base64(screenshot_hash.unwrap().as_str()).unwrap();
            Some(original_screenshot.dist(&new_screenshot))
        } else {
            None
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PageData {
    pub marker: Option<String>,
    pub reference_state: Option<State>,
    pub last_checked: chrono::DateTime<chrono::Utc>,
    pub url_hash: String,
    pub history: Vec<State>,
}

impl PageData {
    pub fn new(state: State, url_hash: String, marker: Option<String>) -> Self {
        PageData {
            marker,
            reference_state: None,
            last_checked: chrono::Utc::now(),
            url_hash,
            history: vec![state],
        }
    }

    pub fn update(&mut self, new_state: State) {
        loop {
            if self.history.len() <= 5 {
                break;
            }
            let _ = self.history.remove(0);
        }

        self.last_checked = chrono::Utc::now();
        self.history.push(new_state);
    }

    pub fn current_state(&self) -> Vec<State> {
        self.history.clone()
    }

    pub fn marker(&self) -> Option<&String> {
        self.marker.as_ref()
    }
}
