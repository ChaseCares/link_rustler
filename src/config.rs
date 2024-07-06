use std::{fs, path::PathBuf, rc::Rc};

use anyhow::Context;
use slint::{ComponentHandle, ModelRc, VecModel};
use tracing::info;
use url::Url;

use crate::structs::{AppState, Config};
use crate::MainWindow;
use crate::{ConfigProperty, Settings};

pub fn load(ui: &MainWindow, app_state: &mut AppState) -> anyhow::Result<Config> {
    app_state.add_to_config_log("Checking configuration.", ui);

    let default_config_path = PathBuf::from("./data/config.toml");
    let default_config = Config::default();

    let config_path = if default_config_path.exists() {
        Some(default_config_path.clone())
    } else {
        None
    };

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

        write_config_file(
            &default_config,
            &"config.toml".into(),
            &default_config.dirs.base_dir,
        )?;

        app_state.add_to_config_log(
            &format!(
                "No config file found, default config file created here: {default_config_path:?}."
            ),
            ui,
        );
    }

    let config = match config {
        Some(c) => c,
        None => {
            app_state.add_to_config_log(
                &format!(
                    "Failed to load config file, using default config: {default_config_path:?}."
                ),
                ui,
            );
            default_config.clone()
        }
    };

    fill_gui_config_panel(ui, &config);

    info!("Configuration loaded successfully");
    app_state.add_to_config_log("Configuration loaded successfully.", ui);
    ui.set_config_ready(true);

    Ok(config)
}

pub fn write_config_file(
    config: &Config,
    file_name: &String,
    base_dir: &String,
) -> anyhow::Result<()> {
    let base_path = PathBuf::from(base_dir);

    if !base_path.exists() {
        fs::create_dir(&base_path)
            .with_context(|| format!("Failed to create data directory {base_path:?}"))?;
    }

    let full_config_path = base_path.join(file_name);
    fs::write(
        &full_config_path,
        toml::to_string_pretty(&config).with_context(|| "Failed to serialize default config")?,
    )
    .with_context(|| format!("Failed to write to config file {full_config_path:?}"))?;

    Ok(())
}

pub fn fill_gui_config_panel(ui: &MainWindow, config: &Config) {
    let propertys = vec![
        ConfigProperty {
            FriendlyName: "Github username".into(),
            Key: "github_username".into(),
            Value: config.github_username.clone().unwrap_or("".into()).into(),
            DisplaType: "string".into(),
            Advanced: false,
        },
        ConfigProperty {
            FriendlyName: "PDF URL".into(),
            Key: "pdf_url".into(),
            Value: config
                .pdf_url
                .clone()
                .unwrap_or(Url::parse("https://github.com/").unwrap())
                .to_string()
                .into(),
            DisplaType: "string".into(),
            Advanced: false,
        },
        ConfigProperty {
            FriendlyName: "Number of local pages".into(),
            Key: "num_of_local_pages".into(),
            Value: config.num_of_local_pages.to_string().into(),
            DisplaType: "num".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Keep local records".into(),
            Key: "keep_local_records".into(),
            Value: config.keep_local_records.to_string().into(),
            DisplaType: "bool".into(),
            Advanced: false,
        },
        ConfigProperty {
            FriendlyName: "Check links".into(),
            Key: "check_links".into(),
            Value: config.check_links.to_string().into(),
            DisplaType: "bool".into(),
            Advanced: false,
        },
        ConfigProperty {
            FriendlyName: "Generate report".into(),
            Key: "gen_report".into(),
            Value: config.gen_report.to_string().into(),
            DisplaType: "bool".into(),
            Advanced: false,
        },
        ConfigProperty {
            FriendlyName: "Screenshot diff confidence".into(),
            Key: "screenshot_diff_confidence".into(),
            Value: config.screenshot_diff_confidence.to_string().into(),
            DisplaType: "num".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Screenshot diff tolerance".into(),
            Key: "screenshot_diff_tolerance".into(),
            Value: config.screenshot_diff_tolerance.to_string().into(),
            DisplaType: "num".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Compression length tolerance".into(),
            Key: "compression_length_tolerance".into(),
            Value: config.compression_length_tolerance.to_string().into(),
            DisplaType: "num".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Page dwell time".into(),
            Key: "page_dwell_time".into(),
            Value: config.page_dwell_time.as_secs().to_string().into(),
            DisplaType: "num".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "PDF path".into(),
            Key: "pdf_path".into(),
            Value: config.pdf_path.clone().unwrap_or("".into()).into(),
            DisplaType: "string".into(),
            Advanced: false,
        },
        ConfigProperty {
            FriendlyName: "Gecko version".into(),
            Key: "gecko_version".into(),
            Value: config.gecko.version.clone().into(),
            DisplaType: "string".into(),
            Advanced: false,
        },
        ConfigProperty {
            FriendlyName: "Gecko arch".into(),
            Key: "gecko_arch".into(),
            Value: config.gecko.arch.clone().into(),
            DisplaType: "string".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Gecko location".into(),
            Key: "gecko_location".into(),
            Value: config.gecko.location.clone().into(),
            DisplaType: "string".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Gecko binary".into(),
            Key: "gecko_binary".into(),
            Value: config.gecko.binary.clone().into(),
            DisplaType: "string".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Gecko headless".into(),
            Key: "gecko_headless".into(),
            Value: config.gecko.headless.to_string().into(),
            DisplaType: "bool".into(),
            Advanced: false,
        },
        ConfigProperty {
            FriendlyName: "Gecko width".into(),
            Key: "gecko_width".into(),
            Value: config.gecko.width.to_string().into(),
            DisplaType: "num".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Gecko height".into(),
            Key: "gecko_height".into(),
            Value: config.gecko.height.to_string().into(),
            DisplaType: "num".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Gecko IP".into(),
            Key: "gecko_ip".into(),
            Value: config.gecko.ip.clone().into(),
            DisplaType: "string".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Gecko port".into(),
            Key: "gecko_port".into(),
            Value: config.gecko.port.to_string().into(),
            DisplaType: "num".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Gecko page load timeout".into(),
            Key: "gecko_page_load_timeout".into(),
            Value: config.gecko.page_load_timeout.as_secs().to_string().into(),
            DisplaType: "num".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Gecko script timeout".into(),
            Key: "gecko_script_timeout".into(),
            Value: config.gecko.script_timeout.as_secs().to_string().into(),
            DisplaType: "num".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Base directory".into(),
            Key: "base_dir".into(),
            Value: config.dirs.base_dir.clone().into(),
            DisplaType: "string".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Project subdirectory".into(),
            Key: "project_subdir".into(),
            Value: config.dirs.project_subdir.clone().into(),
            DisplaType: "string".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Pages subdirectory".into(),
            Key: "pages_subdir".into(),
            Value: config.dirs.pages_subdir.clone().into(),
            DisplaType: "string".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Temporary subdirectory".into(),
            Key: "temp_subdir".into(),
            Value: config.dirs.temp_subdir.clone().into(),
            DisplaType: "string".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Extensions subdirectory".into(),
            Key: "extensions_subdir".into(),
            Value: config.dirs.extensions_subdir.clone().into(),
            DisplaType: "string".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Data store".into(),
            Key: "data_store".into(),
            Value: config.dirs.data_store.clone().into(),
            DisplaType: "string".into(),
            Advanced: true,
        },
        ConfigProperty {
            FriendlyName: "Report".into(),
            Key: "report".into(),
            Value: config.dirs.report.clone().into(),
            DisplaType: "string".into(),
            Advanced: true,
        },
    ];

    ui.global::<Settings>()
        .set_config_propertys(ModelRc::from(Rc::new(VecModel::from(propertys))));
}
