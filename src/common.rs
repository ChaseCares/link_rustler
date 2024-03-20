use blake2::{Blake2s256, Digest};
use image_hasher::HasherConfig;
use tracing::info;

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
