//! CDK project discovery and synthesis.
//!
//! Locates the `cdk.json` for a project and runs `cdk synth` to produce
//! `cdk.out/`, which [`cdk_graph`] parses.
//!
//! See `docs/PRD.md` for the product context.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SynthError {
    #[error("no cdk.json found at or above {path}")]
    NoCdkJson { path: String },
    #[error("invalid cdk.json at {path}: {reason}")]
    InvalidCdkJson { path: String, reason: String },
    #[error("cdk synth failed: {stderr}")]
    SynthFailed { stderr: String },
    #[error("cdk synth timed out after {secs}s")]
    TimedOut { secs: u64 },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result of a successful `cdk synth`.
#[derive(Debug, Clone)]
pub struct SynthResult {
    pub outdir: PathBuf,
    pub stdout: String,
}

/// A CDK project rooted at the directory containing `cdk.json`.
#[derive(Debug, Clone)]
pub struct CdkProject {
    /// Directory containing `cdk.json`.
    pub root: PathBuf,
    /// The `app` command from `cdk.json` (e.g. `npx ts-node bin/app.ts`).
    pub app_command: String,
    /// Resolved output directory (defaults to `<root>/cdk.out`).
    pub outdir: PathBuf,
}

impl CdkProject {
    /// Walk up from `start` looking for `cdk.json`.
    pub fn locate(start: &Path) -> Result<Self, SynthError> {
        let mut current = start.to_path_buf();
        loop {
            let candidate = current.join("cdk.json");
            if candidate.is_file() {
                return Self::from_cdk_json(&candidate);
            }
            match current.parent() {
                Some(parent) => current = parent.to_path_buf(),
                None => {
                    return Err(SynthError::NoCdkJson {
                        path: start.display().to_string(),
                    });
                }
            }
        }
    }

    fn from_cdk_json(path: &Path) -> Result<Self, SynthError> {
        let invalid = |reason: &str| SynthError::InvalidCdkJson {
            path: path.display().to_string(),
            reason: reason.to_string(),
        };
        let root = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let raw = std::fs::read_to_string(path).map_err(SynthError::Io)?;
        let json: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| invalid(&e.to_string()))?;
        let app_command = json
            .get("app")
            .and_then(|v| v.as_str())
            .ok_or_else(|| invalid("missing required string field \"app\""))?
            .to_string();
        let outdir_name = json
            .get("output")
            .and_then(|v| v.as_str())
            .unwrap_or("cdk.out");
        Ok(Self {
            outdir: root.join(outdir_name),
            app_command,
            root,
        })
    }

    pub fn outdir(&self) -> &Path {
        &self.outdir
    }

    /// `tree.json` inside the synthesis output directory.
    pub fn tree_json(&self) -> PathBuf {
        self.outdir.join("tree.json")
    }

    /// `manifest.json` inside the synthesis output directory.
    pub fn manifest_json(&self) -> PathBuf {
        self.outdir.join("manifest.json")
    }

    /// Run `npx cdk synth` into this project's outdir (FR-3).
    ///
    /// Uses the CDK CLI so `cdk.json` `app` is honored. Falls through to a
    /// clear error when Node/`cdk` aren't installed.
    pub fn synthesize(&self, timeout: Duration) -> Result<SynthResult, SynthError> {
        let secs = timeout.as_secs().max(1);
        let outdir = self.outdir.display().to_string();
        let root = self.root.clone();
        let tree_json = self.tree_json();
        let outdir_path = self.outdir.clone();

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = Command::new("npx")
                .args(["--yes", "cdk", "synth", "--quiet", "-o", &outdir])
                .current_dir(&root)
                .env("CDK_DISABLE_NOTICES", "1")
                .env("CI", "1")
                .output();
            let _ = tx.send(result);
        });

        let output = match rx.recv_timeout(timeout) {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                return if e.kind() == std::io::ErrorKind::NotFound {
                    Err(SynthError::SynthFailed {
                        stderr: "npx not found — install Node.js and try again, or use an existing cdk.out".to_string(),
                    })
                } else {
                    Err(SynthError::Io(e))
                };
            }
            Err(_) => return Err(SynthError::TimedOut { secs }),
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !output.status.success() {
            let msg = if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                stderr
            };
            return Err(SynthError::SynthFailed { stderr: msg });
        }
        if !tree_json.is_file() {
            return Err(SynthError::SynthFailed {
                stderr: format!("synth finished but {} is missing", tree_json.display()),
            });
        }
        Ok(SynthResult {
            outdir: outdir_path,
            stdout,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn locates_cdk_json_and_reads_app_command() {
        let project = CdkProject::locate(&fixture("minimal-cdk")).unwrap();
        assert_eq!(project.app_command, "npx cdk --version");
        assert_eq!(project.outdir.file_name().unwrap(), "cdk.out");
        assert_eq!(project.tree_json().file_name().unwrap(), "tree.json");
        assert_eq!(
            project.manifest_json().file_name().unwrap(),
            "manifest.json"
        );
    }

    #[test]
    fn errors_when_no_cdk_json_found() {
        let err = CdkProject::locate(&fixture("does-not-exist")).unwrap_err();
        assert!(matches!(err, SynthError::NoCdkJson { .. }));
    }
}
