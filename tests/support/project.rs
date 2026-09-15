#![allow(dead_code)] // Each integration-test binary uses a different subset of this fixture.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub(crate) type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) struct Project {
    directory: tempfile::TempDir,
    root: PathBuf,
    cache: tempfile::TempDir,
}

impl Project {
    pub(crate) fn new() -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let root = directory.path().canonicalize()?;
        Ok(Self {
            directory,
            root,
            cache: tempfile::tempdir()?,
        })
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.root().join(name)
    }
    pub(crate) fn write(&self, name: &str, bytes: impl AsRef<[u8]>) -> Result {
        let path = self.path(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, bytes)?;
        Ok(())
    }
    pub(crate) fn read(&self, name: &str) -> Result<String> {
        Ok(std::fs::read_to_string(self.path(name))?)
    }
    pub(crate) fn config(&self, body: &str) -> Result {
        self.write("confset.pkl", format!("{}{}", header(), body))
    }
    pub(crate) fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_confset"));
        command
            .current_dir(self.root())
            .env_remove("CONFSET_CONFIG")
            .env("CONFSET_PKL_CACHE_DIR", self.cache.path())
            .env("CONFSET_PKL_OFFLINE", "true")
            .env("HOME", self.root().join("home"))
            .env("USERPROFILE", self.root().join("home"))
            .env("APPDATA", self.root().join("user-config"))
            .env("XDG_CONFIG_HOME", self.root().join("user-config"));
        command
    }
    pub(crate) fn run(&self, args: &[&str]) -> Result<Output> {
        Ok(self.command().args(args).output()?)
    }
    pub(crate) fn ok(&self, args: &[&str]) -> Result<String> {
        let output = self.run(args)?;
        assert!(
            output.status.success(),
            "args={args:?}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?)
    }
    pub(crate) fn error(&self, args: &[&str], expected: &str) -> Result {
        let output = self.run(args)?;
        assert!(
            !output.status.success(),
            "args={args:?} unexpectedly succeeded"
        );
        let message = String::from_utf8_lossy(&output.stderr);
        assert!(
            message.contains(expected),
            "expected {expected:?}, got {message}"
        );
        Ok(())
    }
}

pub(crate) fn header() -> String {
    let base = format!(
        "package://github.com/mathematic-inc/confset/releases/download/v{0}/confset@{0}",
        env!("CARGO_PKG_VERSION")
    );
    format!(
        "amends \"{base}#/Config.pkl\"\nimport \"{base}#/Builtins.pkl\"\nimport \"{base}#/Renderers.pkl\"\n"
    )
}
