//! Runtime helpers for locating and validating the linked `libghostty-vt`.

use std::{
    env,
    path::{Path, PathBuf},
};

use crate::{RenderState, Terminal, TerminalOptions};

/// Convenient alias for runtime initialization results.
pub type Result<T> = std::result::Result<T, RuntimeError>;

/// Runtime initialization error.
#[derive(Debug, Clone)]
pub struct RuntimeError {
    message: String,
}

impl RuntimeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RuntimeError {}

/// Information about the linked `libghostty-vt` runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInfo {
    /// Ghostty commit used for the linked library.
    pub commit: &'static str,
    /// Directory where the dynamic library was found or built.
    pub library_dir: String,
    /// Link kind used for the library.
    pub link_kind: &'static str,
}

/// Validate that the linked library is discoverable and callable.
pub fn initialize_runtime() -> Result<RuntimeInfo> {
    let compiled_library_dir = crate::ffi::LIB_DIR
        .ok_or_else(|| RuntimeError::new("libghostty-vt build output path was not provided"))?;
    let link_kind = crate::ffi::LINK_KIND.unwrap_or("dynamic");

    let library_dir = if link_kind == "dynamic" {
        resolve_dynamic_library_dir(compiled_library_dir)?
    } else {
        compiled_library_dir.to_owned()
    };

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    runtime_sanity_check()?;

    Ok(RuntimeInfo {
        commit: crate::build_info::GHOSTTY_COMMIT,
        library_dir,
        link_kind,
    })
}

/// Linux-specific readiness probe.
#[cfg(target_os = "linux")]
pub fn linux_readiness_probe() -> Result<RuntimeInfo> {
    initialize_runtime()
}

/// Linux-specific readiness probe.
#[cfg(not(target_os = "linux"))]
pub fn linux_readiness_probe() -> Result<RuntimeInfo> {
    Err(RuntimeError::new(
        "linux ghostty readiness probe is only available on linux",
    ))
}

fn runtime_sanity_check() -> Result<()> {
    let mut terminal = Terminal::new(TerminalOptions {
        cols: 8,
        rows: 4,
        max_scrollback: 16,
    })
    .map_err(|error| RuntimeError::new(format!("failed to create terminal: {error}")))?;
    terminal.vt_write(b"ok\r\n");

    let mut render_state = RenderState::new()
        .map_err(|error| RuntimeError::new(format!("render init failed: {error}")))?;
    let snapshot = render_state
        .update(&terminal)
        .map_err(|error| RuntimeError::new(format!("render update failed: {error}")))?;
    let rows = snapshot
        .rows()
        .map_err(|error| RuntimeError::new(format!("failed to query rows: {error}")))?;
    if rows == 0 {
        return Err(RuntimeError::new(
            "ghostty render state returned zero rows during runtime sanity check",
        ));
    }

    Ok(())
}

fn resolve_dynamic_library_dir(compiled_library_dir: &str) -> Result<String> {
    let candidate_dirs = candidate_dynamic_library_dirs(compiled_library_dir);

    for candidate_dir in &candidate_dirs {
        if directory_contains_ghostty_library(candidate_dir) {
            return Ok(candidate_dir.display().to_string());
        }
    }

    let searched_paths = candidate_dirs
        .iter()
        .map(|path| format!("`{}`", path.display()))
        .collect::<Vec<_>>()
        .join(", ");
    Err(RuntimeError::new(format!(
        "libghostty-vt dynamic library not found in runtime search paths [{searched_paths}] (commit `{}`)",
        crate::build_info::GHOSTTY_COMMIT
    )))
}

fn candidate_dynamic_library_dirs(compiled_library_dir: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    push_unique_path(&mut candidates, PathBuf::from(compiled_library_dir));

    if let Ok(current_executable) = env::current_exe() {
        if let Some(executable_dir) = current_executable.parent() {
            push_unique_path(&mut candidates, executable_dir.to_path_buf());

            #[cfg(target_os = "macos")]
            if let Some(contents_dir) = executable_dir.parent() {
                push_unique_path(&mut candidates, contents_dir.join("Frameworks"));
            }

            #[cfg(target_os = "linux")]
            {
                push_unique_path(&mut candidates, executable_dir.join("../lib"));
                push_unique_path(&mut candidates, executable_dir.join("lib"));
            }
        }
    }

    candidates
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn directory_contains_ghostty_library(directory: &Path) -> bool {
    if !directory.is_dir() {
        return false;
    }

    let Ok(entries) = std::fs::read_dir(directory) else {
        return false;
    };

    entries
        .filter_map(|entry| entry.ok())
        .any(|entry| is_ghostty_dynamic_library_name(&entry.file_name().to_string_lossy()))
}

fn is_ghostty_dynamic_library_name(file_name: &str) -> bool {
    if cfg!(target_os = "macos") {
        return file_name == "libghostty-vt.dylib"
            || (file_name.starts_with("libghostty-vt.") && file_name.ends_with(".dylib"));
    }

    if cfg!(target_os = "linux") {
        return file_name == "libghostty-vt.so"
            || file_name == "libghostty-vt.so.0"
            || (file_name.starts_with("libghostty-vt.so.")
                && !file_name.ends_with(".a")
                && !file_name.ends_with(".la"));
    }

    false
}
