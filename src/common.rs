use blake2::{Blake2s256, Digest};
use image_hasher::HasherConfig;
use tracing::{error, info};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt};

use crate::{
    enums::{Arch, Locations, OS},
    get_loc,
};

use crate::{ARCHITECTURE, OPERATING_SYSTEM};

pub fn init_tracing() -> WorkerGuard {
    let file_appender: tracing_appender::rolling::RollingFileAppender =
        tracing_appender::rolling::daily(get_loc(Locations::LogDir), get_loc(Locations::LogPrefix));
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    tracing::subscriber::set_global_default(
        fmt::Subscriber::builder()
            .with_file(true)
            .with_line_number(true)
            .with_max_level(tracing::Level::INFO)
            .finish()
            .with(fmt::Layer::default().with_writer(file_writer)),
    )
    .expect("Unable to set global tracing subscriber");

    guard
}

pub fn hash_img(image: &image::DynamicImage) -> String {
    let hasher = HasherConfig::new().to_hasher();
    let hash = hasher.hash_image(image);
    hash.to_base64()
}

pub fn hash_string(source: &String) -> String {
    let mut hasher = Blake2s256::new();
    hasher.update(source.as_bytes());
    let result = hasher.finalize();
    let mut hash = String::new();
    for byte in result {
        hash.push_str(&format!("{byte:02x}"));
    }
    info!("String hashed successfully. Hash: {hash}");
    hash
}

fn get_arch() -> Arch {
    match *ARCHITECTURE.get_or_init(|| std::env::consts::ARCH) {
        "x86_64" => Arch::X64,
        "x86" => Arch::X86,
        "aarch64" => Arch::Arm64,
        _ => panic!("Unsupported architecture"),
    }
}

fn get_os() -> OS {
    match *OPERATING_SYSTEM.get_or_init(|| std::env::consts::OS) {
        "windows" => OS::Windows,
        "linux" => OS::Linux,
        "macos" => OS::Mac,
        _ => panic!("Unsupported OS"),
    }
}

pub fn get_os_arch_for_geckodriver() -> String {
    let arch = get_arch();

    match get_os() {
        OS::Windows => match arch {
            Arch::X64 => "win32",
            Arch::X86 => "win32",
            Arch::Arm64 => "win64-aarch64",
        },
        OS::Linux => match arch {
            Arch::X64 => "linux64",
            Arch::X86 => "linux32",
            Arch::Arm64 => "linux64-aarch64",
        },
        OS::Mac => match arch {
            Arch::X64 => "macos",
            Arch::X86 => "macos",
            Arch::Arm64 => "macos-aarch64",
        },
    }
    .to_string()
}
