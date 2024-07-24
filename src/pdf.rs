use std::{collections::HashSet, fs::File, io::Read, path::Path};

use anyhow::Context;
use reqwest::{Client, Url};
use tracing::{info, instrument};

#[instrument]
pub async fn get_pdf_github(url: Url) -> anyhow::Result<String> {
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

pub fn pdf_contents(pdf_path: &str) -> anyhow::Result<Vec<u8>> {
    let path = Path::new(pdf_path);
    let mut buf = Vec::new();

    let mut file = File::open(path).context(format!("Failed to open PDF file: {pdf_path}"))?;

    let _ = file
        .read_to_end(&mut buf)
        .context(format!("Failed to read PDF file: {pdf_path}"))?;

    info!("PDF contents read successfully from: {}", pdf_path);
    Ok(buf)
}

pub fn get_unique_links(pdf: &[u8]) -> HashSet<Url> {
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

pub async fn get_urls(
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
