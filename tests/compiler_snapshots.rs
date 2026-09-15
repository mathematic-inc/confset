#![warn(rust_2018_idioms)]

mod support {
    pub(crate) mod project;
}
use support::project::{Project, Result};

#[test]
fn native_formats_have_reviewable_snapshots() -> Result {
    let project = Project::new()?;
    project.config(
        r#"
local data = new Dynamic {
  name = "Confset"
  enabled = true
  columns = List(80, 100)
  rules { strict = "error" }
}
files {
  ["json"] { path = "config.json"; format = "json"; config = data }
  ["yaml"] { path = "config.yaml"; format = "yaml"; config = data }
  ["toml"] { path = "config.toml"; format = "toml"; config = data }
  ["ini"] {
    path = "config.ini"
    format = "ini"
    config { defaults { name = "Confset"; enabled = true } }
  }
}
"#
        .replace(';', "\n")
        .as_str(),
    )?;
    project.ok(&["generate"])?;
    for (snapshot, path) in [
        ("json", "config.json"),
        ("yaml", "config.yaml"),
        ("toml", "config.toml"),
        ("ini", "config.ini"),
    ] {
        insta::assert_snapshot!(snapshot, project.read(path)?);
    }
    Ok(())
}

#[test]
fn invalid_input_has_a_source_located_diagnostic() -> Result {
    let project = Project::new()?;
    project.write("confset.pkl", "tools {\n  [\"broken\"] =\n}\n")?;
    let output = project.run(&["validate"])?;
    assert!(!output.status.success());
    let root = project
        .root()
        .to_string_lossy()
        .replace(r"\\?\", "")
        .replace('\\', "/");
    let message = String::from_utf8(output.stderr)?
        .replace(r"\\?\", "")
        .replace('\\', "/")
        .replace(&root, "$PROJECT");
    insta::assert_snapshot!("invalid_pkl", message);
    Ok(())
}

#[test]
fn command_surface_has_a_snapshot() -> Result {
    let project = Project::new()?;
    insta::assert_snapshot!(
        "help",
        project.ok(&["--help"])?.replace("confset.exe", "confset")
    );
    Ok(())
}

#[test]
fn builtin_definitions_preserve_pkl_property_amendments() {
    insta::assert_snapshot!(
        "builtins",
        include_str!(concat!(env!("OUT_DIR"), "/Builtins.pkl"))
    );
}
