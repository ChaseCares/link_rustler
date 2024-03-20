use std::{io::Write, time::Duration};

use flate2::write::ZlibEncoder;
use flate2::Compression;
use image_hasher::ImageHash;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;
use url::Url;

use crate::common::{hash_img, hash_string};

#[derive(Debug)]
pub(crate) struct Mode<T> {
    pub value: Option<T>,
    pub confidence: Option<usize>,
}

#[derive(Debug)]
pub(crate) struct DiffReport {
    pub page_hash: Mode<String>,
    pub compression: Mode<usize>,
    pub title: Mode<String>,
    pub screenshot_hash: Mode<String>,
}

#[derive(Debug)]
pub(crate) struct ReportTableDataRow {
    pub url: Url,
    pub marker: String,
    pub errors: Option<CustomError>,
    pub invalid_reason: Option<Vec<InvalidReason>>,
    pub valid_reason: Option<Vec<ValidReason>>,
}

#[derive(Debug)]
pub(crate) struct Tables {
    pub valid: Vec<ReportTableDataRow>,
    pub unknown: Vec<ReportTableDataRow>,
    pub hash_only: Vec<ReportTableDataRow>,
    pub error: Vec<ReportTableDataRow>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct Storage {
    pub base_dir: String,
    pub project_subdir: String,
    pub pages_subdir: String,
    pub temp_subdir: String,
    pub extensions_subdir: String,
    pub data_store: String,
    pub report: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct Extensions {
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
pub(crate) struct GeckoConfig {
    pub binary: String,
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
            binary: "geckodriver".to_string(),
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct Config {
    pub github_username: Option<String>,
    pub url: Option<Url>,
    pub num_of_local_pages: usize,
    pub keep_local_records: bool,
    pub check_links: bool,
    pub gen_report: bool,
    pub screenshot_diff_confidence: usize,
    pub screenshot_diff_tolerance: u32,
    pub compression_length_tolerance: usize,
    #[serde(with = "humantime_serde")]
    pub page_dwell_time: Duration,
    pub pdf_path: Option<String>,
    pub gecko: GeckoConfig,
    pub extensions: Option<Vec<Extensions>>,
    pub dirs: Storage,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            github_username: Some(()).map(|()| "Awesome-Octocat-App".to_string()),
            url: Some(()).map(|()| Url::parse("https://github.com/").unwrap()),
            pdf_path: None,
            screenshot_diff_confidence: 60,
            screenshot_diff_tolerance: 3,
            compression_length_tolerance: 300,
            dirs: Storage {
                base_dir: "data".to_string(),
                project_subdir: "default".to_string(),
                pages_subdir: "pages".to_string(),
                temp_subdir: "temp".to_string(),
                extensions_subdir: "extensions".to_string(),
                data_store: "data_store.json".to_string(),
                report: "report.html".to_string(),
            },
            keep_local_records: true,
            check_links: true,
            gen_report: true,
            page_dwell_time: Duration::from_secs(45),
            num_of_local_pages: 2,
            gecko: GeckoConfig::default(),
            extensions: Some(vec![Extensions::default()]),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ActivePages {
    pub url: Url,
    pub time_added: Instant,
    pub linktype: LinkType,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Copy)]
pub(crate) enum LinkType {
    Generic,
    Content,
    Unknown,
    Local,
    Mailto,
    InternalError,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone)]
pub(crate) struct Validity {
    pub valid: Option<Vec<ValidReason>>,
    pub invalid: Option<Vec<InvalidReason>>,
    pub error: Option<CustomError>,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Copy)]
pub(crate) enum ValidReason {
    CompressionExact,
    CompressionWithinTolerance,
    ScreenshotHashExact,
    ScreenshotHashWithinTolerance,
    PageHash,
    Title,
    Marker,
    Type,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Copy)]
pub(crate) enum InvalidReason {
    Compression,
    PageHash,
    ScreenshotHash,
    Title,
    Type,
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq, Clone, Copy)]
pub(crate) enum CustomError {
    InsecureCertificate,
    Redirected,
    BadTitle,
    MarkerNotFound,
    UnknownLinkType,
    LinkTypeLocal,
    LinkTypeMailto,
    BadScreenshot,
    PageNotFound,
    PageError,
    Marker,
    Warning,
    WebDriverError,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct State {
    pub hash: String,
    pub compress_length: usize,
    pub screenshot_hash: Option<String>,
    pub title: Option<String>,
    pub link_type: LinkType,
    pub check_time: chrono::DateTime<chrono::Utc>,
    pub error: Option<CustomError>,
}

impl State {
    pub(crate) fn new(
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

    pub(crate) fn cal_screenshot_similarity(&self, screenshot_hash: Option<String>) -> Option<u32> {
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
pub(crate) struct PageData {
    pub marker: Option<String>,
    pub reference_state: Option<State>,
    pub last_checked: chrono::DateTime<chrono::Utc>,
    pub url_hash: String,
    pub history: Vec<State>,
}

impl PageData {
    pub(crate) fn new(state: State, url_hash: String, marker: Option<String>) -> Self {
        PageData {
            marker,
            reference_state: None,
            last_checked: chrono::Utc::now(),
            url_hash,
            history: vec![state],
        }
    }

    pub(crate) fn update(&mut self, new_state: State) {
        loop {
            if self.history.len() <= 5 {
                break;
            }
            let _ = self.history.remove(0);
        }

        self.last_checked = chrono::Utc::now();
        self.history.push(new_state);
    }

    pub(crate) fn current_state(&self) -> Vec<State> {
        self.history.clone()
    }

    pub(crate) fn marker(&self) -> Option<&String> {
        self.marker.as_ref()
    }
}
