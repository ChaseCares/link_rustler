// Over your eyes! Don't look in here! :)

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;
use std::hash::Hash;
use std::vec;
use std::{fs::OpenOptions, path::Path};

use html_builder::{Buffer, Html5, Node};

use crate::common::hash_string;
use crate::structs::{
    DiffReport, InvalidReason, Mode, PageData, ReportTableDataRow, State, Tables, ValidReason,
};

const NUM_VALID: usize = 8;
const NUM_INVALID: usize = 5;

const CSS: &str = r"* {
	background-color: #272727;
	color: white;
}
table,
th,
td {
	border: 1px solid white;
	border-collapse: collapse;
	padding: 5px;
}
td:nth-child(9) {
	border-right: 1px solid white;
}
.empty {
	border-left: none;
	border-right: none;
	margin: 0;
	padding: 0;
}
.valid {
	color: #00ff00;
	border-left: none;
	border-right: none;
}
.invalid {
	color: red;
	border-left: none;
	border-right: none;
}
";

fn get_data_store(config: &crate::structs::Config) -> BTreeMap<url::Url, PageData> {
    let data_store_path = &format!(
        "./{}/{}/{}",
        config.dirs.base_dir, config.dirs.project_subdir, config.dirs.data_store
    );

    let data_store_file = OpenOptions::new()
        .read(true)
        .open(Path::new(data_store_path))
        .expect("Could not open data store");

    let page_datas: BTreeMap<url::Url, PageData> =
        serde_json::from_reader(data_store_file).expect("Could not parse data store");
    page_datas
}

fn mode<T: Eq + Hash + Clone>(values: &[T]) -> Mode<T> {
    let mut counts = HashMap::new();
    let total = values.len();

    for value in values {
        *counts.entry(value.clone()).or_insert(0) += 1;
    }

    if let Some((value, &count)) = counts.iter().max_by_key(|(_, &count)| count) {
        Mode {
            value: Some(value.clone()),
            confidence: Some(count * 100 / total),
        }
    } else {
        Mode {
            value: None,
            confidence: None,
        }
    }
}

fn diff_report(history: &[State]) -> DiffReport {
    let screenshot_hashes: Vec<String> = history
        .iter()
        .filter_map(|state| state.screenshot_hash.clone())
        .collect();

    DiffReport {
        page_hash: mode(
            &history
                .iter()
                .map(|state| state.hash.clone())
                .collect::<Vec<String>>(),
        ),
        compression: mode(
            &history
                .iter()
                .map(|state| state.compress_length)
                .collect::<Vec<usize>>(),
        ),
        title: mode(
            &history
                .iter()
                .filter_map(|state| state.title.clone())
                .collect::<Vec<String>>(),
        ),
        screenshot_hash: mode(&screenshot_hashes),
    }
}

fn within(value: usize, target: usize, tolerance: usize) -> bool {
    value >= target - tolerance && value <= target + tolerance
}

fn mk_table(
    body: &mut Node<'_>,
    pages_title: &str,
    table_data: Vec<ReportTableDataRow>,
    local_dir: Option<&String>,
) -> anyhow::Result<()> {
    let mut div = body.div();
    let mut h2 = div.h2();
    writeln!(h2, "{pages_title}")?;

    let mut table = body.table();
    let mut thead = table.thead();
    let mut tr = thead.tr();
    writeln!(tr.th(), "URL")?;
    writeln!(tr.th(), "Local data")?;
    writeln!(tr.th(), "Errors")?;
    writeln!(tr.th(), "Marker")?;
    writeln!(tr.th().attr(&format!("colspan='{NUM_INVALID}'")), "Invalid")?;
    writeln!(tr.th().attr(&format!("colspan='{NUM_VALID}'")), "Valid")?;

    let mut table_body = table.tbody();

    for row in table_data {
        let mut tr = table_body.tr();

        let url = row.url;
        let domain = url
            .domain()
            .unwrap_or_else(|| url.host_str().unwrap_or(url.as_str()));
        let url_display = &format!("{domain:.40}");
        let url_hash = hash_string(&url.to_string());

        let mut url_td = tr.td();
        writeln!(
            url_td
                .a()
                .attr(&format!("href='{url}'"))
                .attr("target='_blank'"),
            "{url_display}"
        )?;

        let mut data_td = tr.td();
        if let Some(local_dir) = local_dir {
            writeln!(
                data_td.a().attr(&format!("href='{local_dir}/{url_hash}'")),
                "Data"
            )?;
        } else {
            writeln!(data_td, "None")?;
        }

        if let Some(errors) = row.errors {
            writeln!(tr.td(), "{errors:?}")?;
        } else {
            writeln!(tr.td(), "None")?;
        }

        writeln!(tr.td(), "{}", row.marker)?;

        if let Some(invalid_reason) = &row.invalid_reason {
            for reason in invalid_reason {
                writeln!(tr.td().attr("class='invalid'"), "{reason:?}")?;
            }
        }
        for _ in row.invalid_reason.iter().len()..NUM_INVALID {
            writeln!(tr.td().attr("class='empty'"))?;
        }

        if let Some(valid_reason) = &row.valid_reason {
            for reason in valid_reason {
                writeln!(tr.td().attr("class='valid'"), "{reason:?}")?;
            }
        }
        for _ in row.valid_reason.iter().len()..NUM_VALID {
            writeln!(tr.td().attr("class='empty'"))?;
        }
    }

    Ok(())
}

fn save_report(config: &crate::structs::Config, root_buf: Buffer) {
    let report_file_path = &format!(
        "./{}/{}/{}",
        config.dirs.base_dir, config.dirs.project_subdir, config.dirs.report,
    );

    if Path::new(report_file_path).exists() {
        std::fs::remove_file(report_file_path).unwrap();
    }

    let mut report_file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(Path::new(report_file_path))
        .expect("Could not open report file");

    let page = root_buf.finish();
    std::io::Write::write_all(&mut report_file, page.as_bytes())
        .expect("Could not write report file");
}

#[allow(clippy::too_many_lines)]
pub(crate) fn gen_post_run_report(config: &crate::Config) {
    let page_datas = get_data_store(config);

    let mut root_buf = Buffer::new();
    root_buf.doctype();
    let mut html = root_buf.html().attr("lang='en'");
    let mut head = html.head();
    writeln!(head.title(), "Results!").unwrap();
    let _ = head.meta().attr("charset='UTF-8'");
    let _ = head
        .meta()
        .attr("name='viewport'")
        .attr("content='width=device-width, initial-scale=1.0'");

    writeln!(head.style(), "{CSS}").unwrap();
    let mut body = html.body();

    writeln!(body.h1(), "Results").unwrap();
    let mut tables = Tables {
        valid: vec![],
        unknown: vec![],
        hash_only: vec![],
        error: vec![],
    };

    for (url, page_data) in page_datas {
        let mut history: Vec<State> = page_data.current_state();
        let last_state = history
            .pop()
            .unwrap_or_else(|| panic!("No state for url: {url}"));

        if history.is_empty() {
            continue;
        }

        let mut invalid_reason = vec![];
        let mut valid_reason = vec![];

        let dr = diff_report(&history);
        if last_state.hash.eq(&dr.page_hash.value.unwrap()) {
            valid_reason.push(ValidReason::PageHash);
        } else {
            invalid_reason.push(InvalidReason::PageHash);
        }

        if let Mode {
            value: Some(value),
            confidence: Some(_),
        } = dr.compression
        {
            if last_state.compress_length.eq(&value) {
                valid_reason.push(ValidReason::CompressionExact);
            } else if within(
                last_state.compress_length,
                value,
                config.compression_length_tolerance,
            ) {
                valid_reason.push(ValidReason::CompressionWithinTolerance);
            } else {
                invalid_reason.push(InvalidReason::Compression);
            }
        }

        let screenshot_diff =
            last_state.cal_screenshot_similarity(dr.screenshot_hash.value.clone());

        if last_state.screenshot_hash.eq(&dr.screenshot_hash.value) {
            valid_reason.push(ValidReason::ScreenshotHashExact);
        } else if dr.screenshot_hash.confidence.unwrap_or(0) > config.screenshot_diff_confidence {
            if screenshot_diff.is_some()
                && screenshot_diff.unwrap() < config.screenshot_diff_tolerance
            {
                valid_reason.push(ValidReason::ScreenshotHashWithinTolerance);
            } else {
                invalid_reason.push(InvalidReason::ScreenshotHash);
            }
        } else {
            invalid_reason.push(InvalidReason::ScreenshotHash);
        }

        if let Mode {
            value: Some(value),
            confidence: Some(_),
        } = dr.title
        {
            if last_state.title.unwrap_or_default().eq(&value) {
                valid_reason.push(ValidReason::Title);
            } else {
                invalid_reason.push(InvalidReason::Title);
            }
        }

        let status = if last_state.error.is_some() {
            "error"
        } else if invalid_reason.is_empty() {
            "valid"
        } else if invalid_reason.contains(&InvalidReason::PageHash) && invalid_reason.len() == 1 {
            "hash_only"
        } else {
            "unknown"
        };

        let row = ReportTableDataRow {
            url: url.clone(),
            marker: if page_data.marker.is_some() {
                "Set".to_string()
            } else {
                "Not set".to_string()
            },
            invalid_reason: if invalid_reason.is_empty() {
                None
            } else {
                Some(invalid_reason)
            },
            valid_reason: if valid_reason.is_empty() {
                None
            } else {
                Some(valid_reason)
            },
            errors: last_state.error,
        };

        match status {
            "error" => tables.error.push(row),
            "hash_only" => tables.hash_only.push(row),
            "valid" => tables.valid.push(row),
            _ => tables.unknown.push(row),
        }
    }

    for (title, table) in [
        ("Error", tables.error),
        ("Unknown", tables.unknown),
        ("Hash Only", tables.hash_only),
        ("Valid", tables.valid),
    ] {
        mk_table(&mut body, title, table, Some(&config.dirs.pages_subdir)).unwrap();
    }

    save_report(config, root_buf);
}
