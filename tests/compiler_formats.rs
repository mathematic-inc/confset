#![warn(rust_2018_idioms)]

mod support {
    pub(crate) mod project;
}
use support::project::{Project, Result};

#[test]
fn data_formats_preserve_nested_values_and_large_integers() -> Result {
    let project = Project::new()?;
    project.config(
        r#"
local data = new Dynamic {
  text = "雪 and \"quotes\""
  enabled = true
  count = 9223372036854775807
  ratio = 1.25
  items = List("a", "b")
  nested { value = "x" }
}
files {
  ["json"] { path = "out.json"; format = "json"; config = data }
  ["yaml"] { path = "out.yaml"; format = "yaml"; config = data }
  ["toml"] { path = "out.toml"; format = "toml"; config = data }
}
"#
        .replace(';', "\n")
        .as_str(),
    )?;
    project.ok(&["generate"])?;
    let json: serde_json::Value = serde_json::from_str(&project.read("out.json")?)?;
    let yaml: serde_json::Value = serde_yaml_ng::from_str(&project.read("out.yaml")?)?;
    let toml: serde_json::Value = toml::from_str(&project.read("out.toml")?)?;
    assert_eq!(json, yaml);
    assert_eq!(json, toml);
    assert_eq!(json["count"].as_i64(), Some(i64::MAX));
    Ok(())
}

#[test]
fn ini_emits_editorconfig_sections_and_sqlfluff_values() -> Result {
    let project = Project::new()?;
    project.config(
        r#"
tools {
  ["shell"] = (Builtins.shfmt) {
    config {
      root = true
      ["**/generated/**"] { ignore = true }
    }
  }
  ["sql"] = (Builtins.sqlfluff) {
    config { sqlfluff { dialect = "sqlite"; max_line_length = 120 } }
  }
}
"#
        .replace(';', "\n")
        .as_str(),
    )?;
    project.ok(&["generate"])?;
    assert_eq!(
        project.read(".editorconfig")?,
        "root = true\n\n[**/generated/**]\nignore = true\n"
    );
    assert!(
        project
            .read(".sqlfluff")?
            .contains("[sqlfluff]\ndialect = sqlite\nmax_line_length = 120\n")
    );
    Ok(())
}

#[test]
fn functions_amendments_and_comprehensions_compute_custom_formats() -> Result {
    let project = Project::new()?;
    project.config(r#"
local names = List("alice", "bob")
local function render(items) = "<users>" + items.map((name) -> "<user>" + name + "</user>").join("") + "</users>\n"
local base = new File {
  path = "users.xml"
  format = "text"
  config = render(names)
}

files {
  ["xml"] = base
  ["ignore"] {
    path = ".customignore"
    format = "text"
    config = Renderers.lines(names.map((name) -> name + "/"))
  }
}
"#)?;
    project.ok(&["generate"])?;
    assert_eq!(
        project.read("users.xml")?,
        "<users><user>alice</user><user>bob</user></users>\n"
    );
    assert_eq!(project.read(".customignore")?, "alice/\nbob/\n");
    Ok(())
}

#[test]
fn pkl_functions_are_evaluated_before_knip_json_is_rendered() -> Result {
    let project = Project::new()?;
    project.config(
        r#"
local modules = List("main", "worker")
local function entry(name) = "src/\(name).ts"
tools {
  ["unused"] = (Builtins.knip) {
    config {
      entry = modules.map(entry)
      project = List("src/**/*.ts")
      ignoreDependencies = List("^@example/")
    }
  }
}
"#,
    )?;
    project.ok(&["generate"])?;
    let data: serde_json::Value = serde_json::from_str(&project.read("knip.json")?)?;
    assert_eq!(
        data["entry"],
        serde_json::json!(["src/main.ts", "src/worker.ts"])
    );
    assert_eq!(
        data["ignoreDependencies"],
        serde_json::json!(["^@example/"])
    );
    Ok(())
}

#[test]
fn functions_and_lossy_conversions_are_rejected_before_publication() -> Result {
    let project = Project::new()?;
    project.config(
        r#"
files {
  ["json"] {
    path = "out.json"
    config { value = (x) -> x }
  }
}
"#,
    )?;
    project.error(&["validate"], "Pkl function is not an output value")?;
    for (format, data, message) in [
        ("toml", "null", "TOML"),
        ("ini", "List(1, 2)", "INI entries"),
        ("ini", r#"" padded ""#, "surrounding whitespace"),
    ] {
        project.config(&format!(
            r#"
files {{
  ["invalid"] {{
    path = "out.{format}"
    format = "{format}"
    config {{ value = {data} }}
  }}
}}
"#
        ))?;
        project.error(&["generate"], message)?;
        assert!(!project.path(&format!("out.{format}")).exists());
    }
    Ok(())
}
