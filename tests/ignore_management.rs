#![warn(rust_2018_idioms)]

mod support {
    pub(crate) mod project;
}
use support::project::{Project, Result};

#[test]
fn ignore_updates_preserve_manual_content_and_remove_obsolete_entries() -> Result {
    let project = Project::new()?;
    project.config(r#"tools { ["lint"] = Builtins.oxlint }"#)?;
    project.write(".gitignore", "# My rules\r\nnode_modules/\r\n")?;
    project.ok(&["init", "--gitignore"])?;
    let before = project.read(".gitignore")?;
    assert!(before.starts_with("# My rules\r\nnode_modules/\r\n"));
    assert!(before.contains("/.oxlintrc.json\r\n"));
    project.ok(&["init", "--gitignore"])?;
    assert_eq!(before, project.read(".gitignore")?);
    project.ok(&["generate"])?;
    project.config(r#"tools { ["format"] = Builtins.prettier }"#)?;
    project.ok(&["generate"])?;
    let after = project.read(".gitignore")?;
    assert!(after.starts_with("# My rules\r\nnode_modules/\r\n"));
    assert!(!after.contains("/.oxlintrc.json"));
    assert!(after.contains("/.prettierrc.json\r\n"));
    Ok(())
}

#[test]
fn ignore_management_is_opt_in_and_rejects_ambiguous_markers() -> Result {
    let project = Project::new()?;
    project.config(r#"tools { ["lint"] = Builtins.oxlint }"#)?;
    project.write(".gitignore", "# mine\n")?;
    project.ok(&["generate"])?;
    assert_eq!(project.read(".gitignore")?, "# mine\n");
    project.write(
        ".gitignore",
        "# BEGIN CONFSET GENERATED FILES\nhandwritten\n",
    )?;
    project.error(&["init", "--gitignore"], "incomplete")?;
    assert_eq!(
        project.read(".gitignore")?,
        "# BEGIN CONFSET GENERATED FILES\nhandwritten\n"
    );
    Ok(())
}

#[test]
fn generated_glob_characters_are_literal_to_git() -> Result {
    let project = Project::new()?;
    project.config(r#"files { ["odd"] { path = "nested/[one] space!.json" } }"#)?;
    project.ok(&["init", "--gitignore"])?;
    project.ok(&["generate"])?;
    let init = std::process::Command::new("git")
        .current_dir(project.root())
        .args(["init", "-q"])
        .output()?;
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let ignored = std::process::Command::new("git")
        .current_dir(project.root())
        .args([
            "check-ignore",
            "--no-index",
            "--",
            "nested/[one] space!.json",
        ])
        .output()?;
    assert!(
        ignored.status.success(),
        "{}",
        String::from_utf8_lossy(&ignored.stderr)
    );
    let neighbor = std::process::Command::new("git")
        .current_dir(project.root())
        .args(["check-ignore", "--no-index", "--", "nested/o space!.json"])
        .output()?;
    assert_eq!(neighbor.status.code(), Some(1));
    let index = std::process::Command::new("git")
        .current_dir(project.root())
        .args(["ls-files"])
        .output()?;
    assert!(index.stdout.is_empty());
    Ok(())
}

#[test]
fn tracked_outputs_are_reported_without_changing_the_index() -> Result {
    let project = Project::new()?;
    project.config(r#"tools { ["lint"] = Builtins.oxlint }"#)?;
    project.ok(&["init", "--gitignore"])?;
    project.ok(&["generate"])?;
    let git = |args: &[&str]| -> Result<std::process::Output> {
        Ok(std::process::Command::new("git")
            .current_dir(project.root())
            .args(args)
            .output()?)
    };
    assert!(git(&["init", "-q"])?.status.success());
    assert!(git(&["add", "-f", ".oxlintrc.json"])?.status.success());
    let before = git(&["ls-files", "--stage"])?.stdout;
    let output = project.run(&["generate"])?;
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("already tracked"));
    assert_eq!(before, git(&["ls-files", "--stage"])?.stdout);
    Ok(())
}

#[test]
fn unmanaged_bytes_and_other_managed_blocks_are_preserved() -> Result {
    let project = Project::new()?;
    project.config(r#"tools { ["lint"] = Builtins.oxlint }"#)?;
    let non_utf8 = b"# native comment \xff\nbuild/\n";
    project.write(".gitignore", non_utf8)?;
    project.ok(&["generate"])?;
    assert_eq!(std::fs::read(project.path(".gitignore"))?, non_utf8);
    let other = "# BEGIN ANOTHER TOOL\nother/\n# END ANOTHER TOOL\n";
    project.write(".gitignore", other)?;
    project.ok(&["init", "--gitignore"])?;
    assert!(project.read(".gitignore")?.starts_with(other));
    Ok(())
}
