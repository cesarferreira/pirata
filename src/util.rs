use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use serde::de::{self, Deserializer};

pub fn encode_component(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}

pub fn parse_size_filter(input: &str) -> Result<u64> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("size filter cannot be empty");
    }

    let split_at = trimmed
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(trimmed.len());
    let (number, suffix) = trimmed.split_at(split_at);
    let value: f64 = number
        .parse()
        .map_err(|_| anyhow!("invalid size value: {trimmed}"))?;
    let multiplier = match suffix.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "k" | "kb" => 1_000.0,
        "m" | "mb" => 1_000_000.0,
        "g" | "gb" => 1_000_000_000.0,
        "t" | "tb" => 1_000_000_000_000.0,
        "ki" | "kib" => 1024.0,
        "mi" | "mib" => 1024.0 * 1024.0,
        "gi" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "ti" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        other => bail!("unsupported size suffix: {other}"),
    };

    Ok((value * multiplier).round() as u64)
}

pub fn format_size(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{value} {}", UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

pub fn ensure_transmission_cli_available() -> Result<()> {
    if command_exists("transmission-cli") {
        return Ok(());
    }

    bail!("{}", transmission_cli_missing_message())
}

pub fn ensure_aria2_available() -> Result<()> {
    if command_exists("aria2c") {
        return Ok(());
    }

    bail!("{}", aria2_missing_message())
}

pub fn command_exists(command: &str) -> bool {
    let candidate = PathBuf::from(command);
    if candidate.components().count() > 1 {
        return candidate.is_file();
    }

    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths).any(|dir| dir.join(command).is_file())
    })
}

pub fn transmission_cli_missing_message() -> String {
    if cfg!(target_os = "linux") {
        if fs::read_to_string("/etc/os-release")
            .ok()
            .as_deref()
            .is_some_and(is_debian_like_os_release)
        {
            return "`transmission-cli` was not found in PATH. On Debian/Ubuntu install it with `sudo apt install transmission-cli`.".to_string();
        }

        return "`transmission-cli` was not found in PATH. Install the `transmission-cli` package for your distribution and ensure it is on PATH.".to_string();
    }

    if cfg!(target_os = "macos") {
        return "`transmission-cli` was not found in PATH. Install it with `brew install transmission-cli`.".to_string();
    }

    "`transmission-cli` was not found in PATH. Install it and ensure it is on PATH.".to_string()
}

pub fn aria2_missing_message() -> String {
    if cfg!(target_os = "linux") {
        if fs::read_to_string("/etc/os-release")
            .ok()
            .as_deref()
            .is_some_and(is_debian_like_os_release)
        {
            return "`aria2c` was not found in PATH. On Debian/Ubuntu install it with `sudo apt install aria2`.".to_string();
        }

        return "`aria2c` was not found in PATH. Install the `aria2` package for your distribution and ensure `aria2c` is on PATH.".to_string();
    }

    if cfg!(target_os = "macos") {
        return "`aria2c` was not found in PATH. Install it with `brew install aria2`.".to_string();
    }

    "`aria2c` was not found in PATH. Install it and ensure it is on PATH.".to_string()
}

fn is_debian_like_os_release(contents: &str) -> bool {
    contents.lines().any(|line| {
        let value = line
            .strip_prefix("ID=")
            .or_else(|| line.strip_prefix("ID_LIKE="));

        value.is_some_and(|value| {
            let value = value.trim_matches('"').to_ascii_lowercase();
            value.split_whitespace().any(|part| matches!(part, "debian" | "ubuntu"))
        })
    })
}

pub fn deserialize_u32_from_any<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = StringOrInt::deserialize(deserializer)?;
    value.parse_u32().map_err(de::Error::custom)
}

pub fn deserialize_u64_from_any<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = StringOrInt::deserialize(deserializer)?;
    value.parse_u64().map_err(de::Error::custom)
}

pub fn deserialize_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<StringOrInt>::deserialize(deserializer)?;
    Ok(value
        .map(|item| item.into_string())
        .filter(|item| !item.trim().is_empty()))
}

pub fn deserialize_string_from_any<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = StringOrInt::deserialize(deserializer)?;
    Ok(value.into_string())
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StringOrInt {
    String(String),
    Integer(i64),
    Unsigned(u64),
}

impl StringOrInt {
    fn into_string(self) -> String {
        match self {
            Self::String(value) => value,
            Self::Integer(value) => value.to_string(),
            Self::Unsigned(value) => value.to_string(),
        }
    }

    fn parse_u32(self) -> Result<u32> {
        let raw = self.into_string();
        raw.trim()
            .parse()
            .map_err(|_| anyhow!("invalid u32 value: {raw}"))
    }

    fn parse_u64(self) -> Result<u64> {
        let raw = self.into_string();
        raw.trim()
            .parse()
            .map_err(|_| anyhow!("invalid u64 value: {raw}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{format_size, is_debian_like_os_release, parse_size_filter};

    #[test]
    fn parses_human_size_filters() {
        assert_eq!(parse_size_filter("42").unwrap(), 42);
        assert_eq!(parse_size_filter("1kb").unwrap(), 1_000);
        assert_eq!(parse_size_filter("1MiB").unwrap(), 1_048_576);
        assert_eq!(parse_size_filter("1.5GB").unwrap(), 1_500_000_000);
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn detects_debian_like_os_release() {
        assert!(is_debian_like_os_release("ID=ubuntu\nID_LIKE=debian"));
        assert!(is_debian_like_os_release("ID=debian"));
        assert!(!is_debian_like_os_release("ID=fedora\nID_LIKE=rhel"));
    }
}
