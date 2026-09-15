#![cfg(windows)]
#![warn(rust_2018_idioms)]

mod support {
    pub(crate) mod project;
}
use support::project::{Project, Result};

#[test]
fn junction_parents_cannot_redirect_publication() -> Result {
    let project = Project::new()?;
    let outside = tempfile::tempdir()?;
    let junction = project.path("redirected");
    let result = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(outside.path())
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
    project.config(r#"files { ["output"] { path = "redirected/config.json" } }"#)?;
    project.error(&["generate"], "reparse point")?;
    assert!(!outside.path().join("config.json").exists());
    std::fs::remove_dir(junction)?;
    Ok(())
}
