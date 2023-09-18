use anyhow::{bail, Result};
use std::env;
use std::fs::read_dir;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn find_lib_path() -> Result<PathBuf> {
    let directories = collect_possible_directories();

    if directories.is_empty() {
        bail!("Unable to find possible LLVM shared lib locations.");
    }

    for directory in &directories {
        if let Some(library) = find_library_in_directory(directory) {
            return Ok(library);
        }
    }

    bail!(
        "Unable to find LLVM shared lib in possible locations:\n- {}",
        directories
            .into_iter()
            .map(|item| item.to_str().unwrap().to_owned())
            .collect::<Vec<_>>()
            .join("\n- ")
    );
}

fn collect_possible_directories() -> Vec<PathBuf> {
    let mut paths = vec![];
    let separator = if cfg!(windows) { ';' } else { ':' };

    if let Ok(lib_paths) = env::var("LD_LIBRARY_PATH") {
        for item in lib_paths.split(separator) {
            paths.push(PathBuf::from(item));
        }
    }

    if let Ok(lib_paths) = env::var("DYLD_FALLBACK_LIBRARY_PATH") {
        for item in lib_paths.split(separator) {
            paths.push(PathBuf::from(item));
        }
    }

    if let Ok(bin_paths) = env::var("PATH") {
        for item in bin_paths.split(separator) {
            let mut possible_path = PathBuf::from(item);

            possible_path.pop();
            possible_path.push("lib");
            paths.push(possible_path);
        }
    }

    paths
}

fn find_library_in_directory(directory: &Path) -> Option<PathBuf> {
    let mut files = read_dir(directory).ok()?;
    files.find_map(|file| {
        let file = file.ok()?;
        let path = file.path();
        let stem = path.file_stem()?;
        let stem = stem.to_str()?;
        let extension = path.extension()?;
        (stem.starts_with("libLLVM") && extension != ".a").then_some(path)
    })
}
