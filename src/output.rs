use anyhow::Result;
use comfy_table::{Cell, ContentArrangement, Table, presets::UTF8_FULL};
use serde::Serialize;

use crate::model::{Torrent, TrackedDownload};
use crate::util::format_size;

pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub fn print_search_table(results: &[Torrent]) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::DynamicFullWidth)
        .set_header(vec!["ID", "Name", "Seeders", "Leechers", "Size", "Status"]);

    for torrent in results {
        table.add_row(vec![
            Cell::new(&torrent.id),
            Cell::new(truncate(&torrent.name, 80)),
            Cell::new(torrent.seeders),
            Cell::new(torrent.leechers),
            Cell::new(format_size(torrent.size_bytes)),
            Cell::new(torrent.status.clone().unwrap_or_else(|| "-".to_string())),
        ]);
    }

    println!("{table}");
}

pub fn print_torrent_info(torrent: &Torrent) {
    println!("Name: {}", torrent.name);
    println!("ID: {}", torrent.id);
    println!("Info hash: {}", torrent.info_hash);
    println!("Seeders: {}", torrent.seeders);
    println!("Leechers: {}", torrent.leechers);
    println!("Size: {}", format_size(torrent.size_bytes));
    if let Some(status) = &torrent.status {
        println!("Status: {status}");
    }
    if let Some(user) = &torrent.uploaded_by {
        println!("Uploader: {user}");
    }
    if let Some(category) = &torrent.category {
        println!("Category: {category}");
    }
    if let Some(subcategory) = &torrent.subcategory {
        println!("Subcategory: {subcategory}");
    }
    if let Some(added) = torrent.added {
        println!("Added: {added}");
    }
    println!("Magnet: {}", torrent.resolved_magnet());
    if let Some(description) = &torrent.description {
        println!();
        println!("{description}");
    }
}

pub fn print_tracked_downloads(downloads: &[TrackedDownload]) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::DynamicFullWidth)
        .set_header(vec!["Status", "Name", "Path"]);

    for download in downloads {
        let status = if download.completed {
            "done".to_string()
        } else {
            format!("{}%", download.percent_done)
        };
        table.add_row(vec![
            Cell::new(status),
            Cell::new(truncate(&download.name, 60)),
            Cell::new(truncate(&download.target_path.display().to_string(), 80)),
        ]);
    }

    println!("{table}");
}

fn truncate(value: &str, max_len: usize) -> String {
    let count = value.chars().count();
    if count <= max_len {
        value.to_string()
    } else {
        let mut truncated = value
            .chars()
            .take(max_len.saturating_sub(3))
            .collect::<String>();
        truncated.push_str("...");
        truncated
    }
}
