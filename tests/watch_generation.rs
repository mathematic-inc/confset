#![warn(rust_2018_idioms)]

mod support {
    pub(crate) mod project;
}
use support::project::{Project, Result};

use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// Allows native filesystem notifications and debug Pkl evaluation to complete on CI.
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);

struct Watching {
    child: Child,
    messages: Receiver<String>,
}

impl Watching {
    fn start(mut command: std::process::Command) -> Result<Self> {
        let mut child = command
            .args(["generate", "--watch"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        let stderr = child.stderr.take().ok_or("missing watch stderr")?;
        let (sender, messages) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                match line {
                    Ok(line) => {
                        if sender.send(line).is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
        });
        Ok(Self { child, messages })
    }
    fn wait_for(&self, text: &str) -> Result {
        let deadline = std::time::Instant::now() + EVENT_TIMEOUT;
        let mut seen = Vec::new();
        loop {
            match self
                .messages
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            {
                Ok(message) if message.contains(text) => return Ok(()),
                Ok(message) => seen.push(message),
                Err(error) => {
                    return Err(format!("waiting for {text:?}: {error}; messages: {seen:?}").into());
                }
            }
        }
    }
    fn shutdown(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[test]
fn atomic_saves_and_import_changes_keep_the_last_valid_generation() -> Result {
    let project = Project::new()?;
    project.write("settings.pkl", "width = 80\n")?;
    project.config(
        r#"
import "settings.pkl"
tools {
  ["format"] = (Builtins.oxfmt) {
    config { printWidth = settings.width }
  }
}
"#,
    )?;
    let mut watching = Watching::start(project.command())?;
    watching.wait_for("generated 1 configuration files")?;
    assert!(project.read(".oxfmtrc.json")?.contains("80"));
    project.error(&["generate"], "another Confset writer")?;
    project.write("replacement.pkl", "width = 120\n")?;
    std::fs::rename(
        project.path("replacement.pkl"),
        project.path("settings.pkl"),
    )?;
    watching.wait_for("generated 1 configuration files")?;
    assert!(project.read(".oxfmtrc.json")?.contains("120"));
    project.write("settings.pkl", "width = ???")?;
    watching.wait_for("keeping the last successful outputs")?;
    assert!(project.read(".oxfmtrc.json")?.contains("120"));
    project.write("settings.pkl", "width = 90\n")?;
    watching.wait_for("generated 1 configuration files")?;
    assert!(project.read(".oxfmtrc.json")?.contains("90"));
    watching.shutdown();
    assert!(project.path(".oxfmtrc.json").exists());
    project.ok(&["generate"])?;
    Ok(())
}

#[test]
fn initially_invalid_configuration_can_be_fixed_without_restarting() -> Result {
    let project = Project::new()?;
    project.write("confset.pkl", "tools {")?;
    let mut watching = Watching::start(project.command())?;
    watching.wait_for("keeping the last successful outputs")?;
    project.config(r#"tools { ["lint"] = Builtins.oxlint }"#)?;
    watching.wait_for("generated 1 configuration files")?;
    assert!(project.path(".oxlintrc.json").exists());
    watching.shutdown();
    assert!(project.path(".oxlintrc.json").exists());
    Ok(())
}

#[test]
fn glob_imports_detect_additions_and_bursts_publish_the_latest_value() -> Result {
    let project = Project::new()?;
    project.write("fragments/one.pkl", "width = 80\n")?;
    project.config(
        r#"
import* "fragments/*.pkl" as fragments
files { ["combined"] { path = "out.json"; config = fragments } }
"#
        .replace(';', "\n")
        .as_str(),
    )?;
    let mut watching = Watching::start(project.command())?;
    watching.wait_for("generated 1 configuration files")?;
    assert!(project.read("out.json")?.contains("80"));
    project.write("fragments/two.pkl", "width = 100\n")?;
    watching.wait_for("generated 1 configuration files")?;
    assert!(project.read("out.json")?.contains("100"));
    for width in 120..140 {
        project.write("fragments/two.pkl", format!("width = {width}\n"))?;
    }
    watching.wait_for("generated 1 configuration files")?;
    assert!(project.read("out.json")?.contains("139"));
    watching.shutdown();
    Ok(())
}

#[test]
fn a_project_local_package_cache_does_not_trigger_regeneration() -> Result {
    let project = Project::new()?;
    project.write("fragments/one.pkl", "width = 80\n")?;
    project.config(
        r#"
import* "fragments/*.pkl" as fragments
files { ["combined"] { path = "out.json"; config = fragments } }
"#
        .replace(';', "\n")
        .as_str(),
    )?;
    let mut command = project.command();
    command.env("CONFSET_PKL_CACHE_DIR", project.path(".cache"));
    let watching = Watching::start(command)?;
    watching.wait_for("generated 1 configuration files")?;
    let cache = std::fs::read_dir(project.path(".cache"))?
        .next()
        .ok_or("missing package cache")??
        .path();
    std::fs::write(cache.join("unrelated"), "cache activity")?;
    assert!(matches!(
        watching.messages.recv_timeout(Duration::from_millis(500)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    Ok(())
}

#[test]
fn nested_amendments_are_watched_through_multiple_levels() -> Result {
    let project = Project::new()?;
    let base = format!(
        "{}{}",
        support::project::header(),
        r#"tools { ["format"] = (Builtins.oxfmt) { config { printWidth = 80 } } }"#
    );
    project.write("config/shared/base.pkl", &base)?;
    project.write(
        "config/project.pkl",
        r#"amends "shared/base.pkl"
    tools { ["format"] { config { semi = false } } }"#,
    )?;
    project.write("confset.pkl", "amends \"config/project.pkl\"\n")?;
    let watching = Watching::start(project.command())?;
    watching.wait_for("generated 1 configuration files")?;
    let initial: serde_json::Value = serde_json::from_str(&project.read(".oxfmtrc.json")?)?;
    assert_eq!(initial["semi"], false);
    assert_eq!(initial["printWidth"], 80);
    project.write("config/shared/base.pkl", base.replace("80", "110"))?;
    watching.wait_for("generated 1 configuration files")?;
    let updated: serde_json::Value = serde_json::from_str(&project.read(".oxfmtrc.json")?)?;
    assert_eq!(updated["semi"], false);
    assert_eq!(updated["printWidth"], 110);
    Ok(())
}
