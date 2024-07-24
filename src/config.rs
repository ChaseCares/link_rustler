use std::{fs, path::PathBuf, rc::Rc};

use anyhow::Context;
use slint::{ComponentHandle, ModelRc, VecModel};
use tracing::info;
use url::Url;

use crate::{
    get_loc,
    structs::{AppState, Config},
    ConfigProperty, Locations, MainWindow, Settings,
};

pub fn load(ui: &MainWindow, app_state: &mut AppState) -> anyhow::Result<Config> {
    app_state.add_to_config_log("Checking configuration.", ui);
    let config_path = get_loc(Locations::Config);
    let default_config = Config::default();

    let config = if config_path.exists() {
        let config_str = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file {config_path:?}"))?;

        toml::from_str(&config_str)
            .with_context(|| format!("Failed to parse config file {config_path:?}"))?
    } else {
        write_config_file(&default_config, &config_path)?;

        app_state.add_to_config_log(
            &format!("No config file found, default config file created here: {config_path:?}."),
            ui,
        );
        default_config
    };

    fill_gui_config_panel(ui, &config);

    info!("Configuration loaded successfully");
    app_state.add_to_config_log("Configuration loaded successfully.", ui);
    ui.global::<Settings>().set_config_ready(true);

    Ok(config)
}

pub fn no_ui_load() -> anyhow::Result<Config> {
    let config_path = get_loc(Locations::Config);
    let default_base_path = config_path.parent().unwrap();

    let default_config = Config::default();

    let config = if config_path.exists() {
        let config_str = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file {config_path:?}"))?;

        toml::from_str(&config_str)
            .with_context(|| format!("Failed to parse config file {config_path:?}"))?
    } else {
        fs::create_dir(default_base_path)
            .with_context(|| format!("Failed to create data directory {default_base_path:?}"))?;
        write_config_file(&default_config, &config_path)?;

        default_config
    };

    info!("Configuration loaded successfully");

    Ok(config)
}

pub fn write_config_file(config: &Config, config_path: &PathBuf) -> anyhow::Result<()> {
    fs::write(
        config_path,
        toml::to_string_pretty(&config).with_context(|| "Failed to serialize default config")?,
    )
    .with_context(|| format!("Failed to write to config file {config_path:?}"))?;

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
    ];

    ui.global::<Settings>()
        .set_config_propertys(ModelRc::from(Rc::new(VecModel::from(propertys))));
}
