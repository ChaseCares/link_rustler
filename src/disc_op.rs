use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};

use anyhow::Context;
use chrono::Utc;
use reqwest::Url;
use tracing::{error, info, instrument, warn};

use crate::{
    common, get_loc,
    structs::{Config, PageData},
    Locations,
};

#[instrument]
pub fn init_storage(clean_start: bool) {
    let base_config_dir = get_loc(Locations::BaseConfig);
    let base_data_dir = get_loc(Locations::BaseData);

    let dirs = vec![base_config_dir, base_data_dir];

    for dir in &dirs {
        if dir.exists() && clean_start {
            if let Err(err) = fs::remove_dir_all(dir) {
                warn!("Failed to remove {dir:?}: {err:?}");
            } else {
                info!("Removed directory: {dir:?}");
            }
        }
        if let Err(err) = fs::create_dir_all(dir) {
            error!("Failed to create {dir:?}: {err:?}");
        } else {
            info!("Created directory: {dir:?}");
        }
    }
}

pub fn load_data_store(data_store_path: &PathBuf) -> anyhow::Result<BTreeMap<Url, PageData>> {
    let path_str = data_store_path.to_string_lossy();

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
        info!("Data store path does not exist: {path_str}");
        Ok(BTreeMap::new())
    }
}

#[instrument]
pub fn save_data_store(
    page_datas: &BTreeMap<Url, PageData>,
    data_store_path: &PathBuf,
) -> anyhow::Result<(), anyhow::Error> {
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

pub fn save_page_data(
    url: &Url,
    config: &Config,
    page_source: &str,
    img: &image::DynamicImage,
) -> anyhow::Result<()> {
    let now = Utc::now();
    let url_hash = common::hash_string(&url.to_string());

    let save_data_path = get_loc(Locations::PagesSubdir).join(url_hash);

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
