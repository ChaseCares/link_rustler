use std::{fs, path::PathBuf};

use anyhow::Context;
use tracing::{info, instrument};

use crate::{structs::Config, Args};

#[instrument]
pub fn config(args: &Args) -> anyhow::Result<Config> {
    let default_config_path = PathBuf::from("./data/config.toml");
    let default_config = Config::default();

    let config_path = args.config_path.as_ref().map(PathBuf::from).or_else(|| {
        if default_config_path.exists() {
            Some(default_config_path.clone())
        } else {
            None
        }
    });

    let config = if let Some(path) = config_path {
        let config_str = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file {path:?}"))?;
        Some(
            toml::from_str(&config_str)
                .with_context(|| format!("Failed to parse config file {path:?}"))?,
        )
    } else {
        None
    };

    if config.is_none() {
        let default_base_path = PathBuf::from("./data");
        if !default_base_path.exists() {
            fs::create_dir(&default_base_path).with_context(|| {
                format!("Failed to create data directory {default_base_path:?}")
            })?;
        }

        fs::write(
            &default_config_path,
            toml::to_string_pretty(&default_config)
                .with_context(|| "Failed to serialize default config")?,
        )
        .with_context(|| format!("Failed to write to config file {default_config_path:?}"))?;
        panic!("No config file found, default config file created at ./data/config.toml");
    }

    let mut config: Config = config.unwrap();

    assert!(
        !(config == default_config),
        "Default config file found at ./data/config.toml, please update it with your settings"
    );

    if let Some(pdf_path) = &args.pdf_path {
        config.pdf_path = Some(pdf_path.clone());
    }

    info!("Configuration loaded successfully");
    Ok(config)
}
