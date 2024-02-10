use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use clap::Parser;
use uuid::Uuid;
use walkdir::WalkDir;
use yaml_rust::{Yaml, YamlLoader};

const UUID_STR_LEN: usize = 32;

#[derive(Parser)]
struct Options {
    #[arg(long, short)]
    force: bool,
    #[arg(long, short)]
    ignore: Option<String>,
    scan_dir: Option<PathBuf>,
}

fn main() {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();

    let Options {
        ignore,
        scan_dir,
        force,
    } = Options::parse();

    let working_dir = std::env::current_dir().unwrap();
    let scan_dir = scan_dir.map_or(Cow::Borrowed(&working_dir), Cow::Owned);
    let ignore = ignore
        .map_or(Cow::Borrowed("png,git,fbx,exe"), Cow::Owned)
        .split(",")
        .map(|s| format!(".{}", s.trim()))
        .collect::<Vec<_>>();

    let mapping = make_mapping(&scan_dir);
    apply_mapping(&working_dir, &ignore, &mapping, force);

    if !force {
        log::warn!("Dry-run: no changes made. Use --force or -f to apply changes.");
    }
}

fn make_mapping(dir: &Path) -> Vec<(String, String)> {
    let mut mapping = Vec::new();
    let guid_key = Yaml::String("guid".to_owned());

    for entry in WalkDir::new(dir) {
        let entry = entry.unwrap();

        if !entry.file_type().is_file() {
            continue;
        }

        let file_name = entry.file_name().to_string_lossy();
        if !file_name.ends_with(".meta") {
            continue;
        }

        let yaml = match std::fs::read_to_string(entry.path()) {
            Ok(yaml) => yaml,
            Err(e) => {
                log::error!("reading {}: {}", entry.path().display(), e);
                continue;
            }
        };

        let yaml = match YamlLoader::load_from_str(&yaml) {
            Ok(mut xs) if xs.len() == 1 => xs.pop().unwrap(),
            Ok(xs) => {
                log::error!(
                    "unexpected {} documents in .meta: {}",
                    xs.len(),
                    entry.path().display()
                );
                continue;
            }
            Err(e) => {
                log::error!("parsing {}: {}", entry.path().display(), e);
                continue;
            }
        };

        let Yaml::Hash(hash) = yaml else {
            log::error!("unexpected non-hash in .meta: {}", entry.path().display());
            continue;
        };

        let Some(Yaml::String(guid)) = hash.get(&guid_key) else {
            log::error!(
                "expecting guid field with string value in .meta: {}",
                entry.path().display()
            );
            continue;
        };

        let guid = match uuid::Uuid::parse_str(guid) {
            Ok(guid) => guid,
            Err(e) => {
                log::error!(
                    "{} parsing uuid {} in .meta: {}",
                    e,
                    guid,
                    entry.path().display()
                );
                continue;
            }
        };

        let new_guid = Uuid::new_v4();
        log::info!("will map {} -> {}", guid, new_guid);
        mapping.push((guid.simple().to_string(), new_guid.simple().to_string()));
    }

    mapping
}

fn apply_mapping(dir: &Path, ignore: &[String], mapping: &[(String, String)], force: bool) {
    for entry in WalkDir::new(dir) {
        let entry = entry.unwrap();

        if !entry.file_type().is_file() {
            continue;
        }

        let file_name = entry.file_name().to_string_lossy();
        if ignore.iter().any(|ext| file_name.ends_with(ext)) {
            continue;
        }

        let mut contents = match std::fs::read_to_string(entry.path()) {
            Ok(contents) => contents,
            Err(e) => {
                log::error!("reading {}: {}", entry.path().display(), e);
                continue;
            }
        };

        let mut indices = Vec::new();
        for (src, dst) in mapping {
            indices.clear();
            indices.extend(contents.match_indices(src).map(|(n, _)| n));
            if indices.is_empty() {
                continue;
            }

            log::info!(
                "will rewrite {} instances of {} -> {} in {}",
                indices.len(),
                src,
                dst,
                entry.path().display()
            );

            if force {
                for n in &indices {
                    let n = *n;
                    unsafe {
                        contents[n..(n + UUID_STR_LEN)]
                            .as_bytes_mut()
                            .copy_from_slice(dst.as_bytes())
                    }
                }
            }
        }

        if force {
            if let Err(e) = std::fs::write(entry.path(), contents) {
                log::error!("writing {}: {}", entry.path().display(), e);
            };
        }
    }
}
