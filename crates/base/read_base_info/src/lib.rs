use std::{collections::HashMap, env, fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

// =========================
// TYPED BASE CONFIG
// =========================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TimeInfo {
    #[serde(default)]
    pub creation_unix: String,
    #[serde(default)]
    pub edit_unix: String,
    #[serde(rename = "FLD_unix", default)]
    pub fld_unix: String,
    #[serde(rename = "LLD_unix", default)]
    pub lld_unix: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LatestInfo {
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub last_line: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PathEntry {
    pub path: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub ignored: Vec<String>,
    #[serde(default)]
    pub latest: Value,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PathEntryRaw {
    New(PathEntryFields),
    Legacy(HashMap<String, LegacyPathData>),
}

#[derive(Deserialize)]
struct PathEntryFields {
    path: String,
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    ignored: Vec<String>,
    #[serde(default)]
    latest: Value,
}

#[derive(Deserialize)]
struct LegacyPathData {
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    ignored: Vec<String>,
    #[serde(default)]
    latest: Value,
}

impl PathEntry {
    pub fn latest_flat(&self) -> LatestInfo {
        flatten_latest(&self.latest, &self.path)
    }

    pub fn set_latest_flat(&mut self, info: LatestInfo) {
        self.latest = serde_json::to_value(info).unwrap_or(Value::Null);
    }
}

fn flatten_latest(v: &Value, source_path: &str) -> LatestInfo {
    match v {
        Value::Null => LatestInfo::default(),
        Value::Object(map) => {
            if map.contains_key("hash") || map.contains_key("last_line") {
                serde_json::from_value(v.clone()).unwrap_or_default()
            } else if let Some(inner) = map.get(source_path) {
                serde_json::from_value(inner.clone()).unwrap_or_default()
            } else {
                map.values()
                    .next()
                    .and_then(|x| serde_json::from_value::<LatestInfo>(x.clone()).ok())
                    .unwrap_or_default()
            }
        }
        _ => LatestInfo::default(),
    }
}

/// Metadata for a single chunk of the base.
///
/// A base is stored as a sequence of zstd chunks, ordered by timestamp.
/// Chunks never overlap: `last_ts` of chunk N <= `first_ts` of chunk N+1.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChunkInfo {
    /// File name of the chunk inside the base directory (e.g. "mybase.0.zst").
    pub file: String,
    /// Number of log lines stored in the chunk.
    pub lines: u64,
    /// Timestamp of the first line in the chunk.
    #[serde(default)]
    pub first_ts: i64,
    /// Timestamp of the last line in the chunk.
    #[serde(default)]
    pub last_ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseConfig {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub lines: u64,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub tags: Vec<Value>,
    #[serde(default = "default_custom")]
    pub custom: String,
    #[serde(default)]
    pub time: TimeInfo,
    #[serde(default, deserialize_with = "de_path_entry_vec")]
    pub paths: Vec<PathEntry>,
    /// Chunked storage layout. Empty for legacy bases that use a single `<name>.zst`.
    #[serde(default)]
    pub chunks: Vec<ChunkInfo>,
    /// Number of log lines per chunk used when this base was created.
    #[serde(default = "default_chunk_lines")]
    pub chunk_lines: u64,
}

impl Default for BaseConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            lines: 0,
            hash: String::new(),
            tags: Vec::new(),
            custom: default_custom(),
            time: TimeInfo::default(),
            paths: Vec::new(),
            chunks: Vec::new(),
            chunk_lines: DEFAULT_CHUNK_LINES,
        }
    }
}

fn default_custom() -> String {
    "custom".to_string()
}

fn default_chunk_lines() -> u64 {
    DEFAULT_CHUNK_LINES
}

fn de_path_entry_vec<'de, D>(d: D) -> Result<Vec<PathEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Vec::<PathEntryRaw>::deserialize(d)?;
    Ok(raw
        .into_iter()
        .flat_map(|r| match r {
            PathEntryRaw::New(f) => vec![PathEntry {
                path: f.path,
                files: f.files,
                ignored: f.ignored,
                latest: f.latest,
            }],
            PathEntryRaw::Legacy(map) => map
                .into_iter()
                .map(|(path, data)| PathEntry {
                    path,
                    files: data.files,
                    ignored: data.ignored,
                    latest: data.latest,
                })
                .collect(),
        })
        .collect())
}

pub fn parse_base_config(content: &str) -> Result<BaseConfig, String> {
    let mut cfg: BaseConfig =
        serde_json::from_str(content).map_err(|e| format!("Broken JSON base file: {}", e))?;

    let entries = std::mem::take(&mut cfg.paths);
    cfg.paths = normalize_paths(entries);

    Ok(cfg)
}

pub fn normalize_paths(entries: Vec<PathEntry>) -> Vec<PathEntry> {
    let mut result: Vec<PathEntry> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();

    for e in entries {
        match index.get(&e.path) {
            Some(&i) => {
                let dst = &mut result[i];
                for f in e.files {
                    if !dst.files.contains(&f) {
                        dst.files.push(f);
                    }
                }
                for f in e.ignored {
                    if !dst.ignored.contains(&f) {
                        dst.ignored.push(f);
                    }
                }
                if dst.latest.is_null() && !e.latest.is_null() {
                    dst.latest = e.latest;
                }
            }
            None => {
                index.insert(e.path.clone(), result.len());
                result.push(e);
            }
        }
    }

    result
}

// =========================
// LOAD / SAVE
// =========================

fn bases_dir() -> Result<PathBuf, String> {
    let appdata = env::var("APPDATA").map_err(|_| "APPDATA not found.".to_string())?;
    Ok(PathBuf::from(appdata).join("LogTools").join("bases"))
}

pub fn load_log(name: &str) -> Result<BaseConfig, String> {
    let path = bases_dir()?.join(name).join(format!("{}.json", name));

    let content = fs::read_to_string(&path)
        .map_err(|_| format!("Base '{}' not found: {}", name, path.display()))?;

    parse_base_config(&content)
}

pub fn save_log(name: &str, config: &BaseConfig) -> Result<(), String> {
    let dir = bases_dir()?.join(name);

    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create base dir: {}", e))?;

    let path = dir.join(format!("{}.json", name));
    let content =
        serde_json::to_string_pretty(config).map_err(|e| format!("Serialize error: {}", e))?;

    fs::write(&path, content).map_err(|e| format!("Cannot write base config: {}", e))
}

pub fn get_zst_path(name: &str) -> Result<String, String> {
    let path = bases_dir()?.join(name).join(format!("{}.zst", name));
    Ok(path.to_string_lossy().to_string())
}

pub fn base_dir(name: &str) -> Result<PathBuf, String> {
    Ok(bases_dir()?.join(name))
}

pub fn chunk_file_name(name: &str, index: usize) -> String {
    format!("{}.{}.zst", name, index)
}

pub fn get_chunk_path(name: &str, index: usize) -> Result<String, String> {
    let path = base_dir(name)?.join(chunk_file_name(name, index));
    Ok(path.to_string_lossy().to_string())
}

/// Default number of log lines per chunk.
pub const DEFAULT_CHUNK_LINES: u64 = 500_000;

/// Default number of lines kept in memory before a sorted run is spilled to disk.
pub const DEFAULT_SORT_RUN_LINES: u64 = 250_000;

/// Legacy alias kept for callers that only need the default chunk size.
pub const CHUNK_LINES: u64 = DEFAULT_CHUNK_LINES;

pub fn chunk_files(name: &str, cfg: &BaseConfig) -> Result<Vec<(String, i64, i64)>, String> {
    let dir = base_dir(name)?;

    Ok(cfg
        .chunks
        .iter()
        .map(|c| {
            (
                dir.join(&c.file).to_string_lossy().to_string(),
                c.first_ts,
                c.last_ts,
            )
        })
        .collect())
}

pub fn delete_base(name: &str) -> Result<(), String> {
    let dir = bases_dir()?.join(name);

    if !dir.is_dir() {
        return Err(format!("Base '{}' not found", name));
    }

    fs::remove_dir_all(&dir).map_err(|e| format!("Cannot delete base '{}': {}", name, e))
}

// =========================
// LIST BASES
// =========================

#[derive(Debug, Clone)]
pub struct BaseInfo {
    pub name: String,
    pub lines: usize,
    pub paths_count: usize,
    pub creation_time: String,
    pub last_edit_time: String,
}

pub fn list_bases() -> Result<Vec<BaseInfo>, String> {
    let bases_path = bases_dir()?;

    if !bases_path.exists() {
        return Ok(Vec::new());
    }

    let mut bases = Vec::new();

    let entries =
        fs::read_dir(&bases_path).map_err(|e| format!("Failed to read bases dir: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();

        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            let json_path = path.join(format!("{}.json", name));

            if json_path.exists() {
                if let Ok(cfg) = load_log(&name) {
                    bases.push(BaseInfo {
                        name,
                        lines: cfg.lines as usize,
                        paths_count: cfg.paths.len(),
                        creation_time: cfg.time.creation_unix,
                        last_edit_time: cfg.time.edit_unix,
                    });
                }
            }
        }
    }

    bases.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(bases)
}

fn format_unix_time(ts: &str) -> String {
    if ts.is_empty() {
        return "—".to_string();
    }
    match ts.parse::<i64>() {
        Ok(unix_ts) => {
            use chrono::{DateTime, Local, TimeZone};
            let dt: DateTime<Local> = Local.timestamp_opt(unix_ts, 0).single().unwrap_or_default();
            dt.format("%Y-%m-%d %H:%M:%S").to_string()
        }
        Err(_) => "—".to_string(),
    }
}

pub fn print_bases_table(bases: &[BaseInfo]) {
    if bases.is_empty() {
        println!("No bases found.");
        return;
    }

    const NAME_W: usize = 25;
    const LINES_W: usize = 12;
    const PATHS_W: usize = 8;
    const TIME_W: usize = 20;

    let header = format!(
        "{:<NAME_W$} {:>LINES_W$} {:>PATHS_W$} {:<TIME_W$} {:<TIME_W$}",
        "NAME",
        "LINES",
        "PATHS",
        "CREATED",
        "EDITED",
        NAME_W = NAME_W,
        LINES_W = LINES_W,
        PATHS_W = PATHS_W,
        TIME_W = TIME_W
    );

    let separator = "─".repeat(header.len());

    println!("{}", header);
    println!("{}", separator);

    for base in bases {
        let name = if base.name.len() > NAME_W {
            format!("{}…", &base.name[..NAME_W - 1])
        } else {
            base.name.clone()
        };

        let created = format_unix_time(&base.creation_time);
        let edited = format_unix_time(&base.last_edit_time);

        println!(
            "{:<NAME_W$} {:>LINES_W$} {:>PATHS_W$} {:<TIME_W$} {:<TIME_W$}",
            name,
            base.lines,
            base.paths_count,
            created,
            edited,
            NAME_W = NAME_W,
            LINES_W = LINES_W,
            PATHS_W = PATHS_W,
            TIME_W = TIME_W
        );
    }

    println!("{}", separator);
    println!("Total: {} base(s)", bases.len());
}
