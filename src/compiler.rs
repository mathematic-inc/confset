//! Evaluated values, native file selection, and format compilation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Map, Number};

use crate::config::{Evaluation, Inputs, Source, absolute};

#[derive(Clone, Debug)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub(crate) fn from_pkl(value: &pklr::Value, path: &str) -> Result<Self> {
        Ok(match value {
            pklr::Value::Null => Self::Null,
            pklr::Value::Bool(value) => Self::Bool(*value),
            pklr::Value::Int(value) => Self::Int(*value),
            pklr::Value::Float(value) if value.is_finite() => Self::Float(*value),
            pklr::Value::Float(_) => bail!("{path}: non-finite numbers cannot be rendered"),
            pklr::Value::String(value) => Self::String(value.clone()),
            pklr::Value::Lambda(..) => bail!(
                "{path}: a Pkl function is not an output value; call it to compute a data value"
            ),
            pklr::Value::List(values) => Self::Array(
                values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| Self::from_pkl(value, &format!("{path}[{index}]")))
                    .collect::<Result<_>>()?,
            ),
            pklr::Value::Object(values, _) => {
                let fields = values
                    .iter()
                    .map(|(key, value)| {
                        Self::from_pkl(value, &format!("{path}.{key}"))
                            .map(|value| (key.clone(), value))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Self::Object(fields)
            }
        })
    }

    fn object(&self) -> Result<&[(String, Value)]> {
        match self {
            Self::Object(fields) => Ok(fields),
            _ => bail!("expected an object"),
        }
    }
    fn get(&self, key: &str) -> Option<&Value> {
        self.object()
            .ok()?
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }
    fn string(&self) -> Result<&str> {
        match self {
            Self::String(value) => Ok(value),
            _ => bail!("expected a string"),
        }
    }
    fn text(&self, key: &str) -> Result<Option<&str>> {
        match self.get(key) {
            None | Some(Self::Null) => Ok(None),
            Some(value) => value.string().map(Some),
        }
    }
    fn check_fields(&self, allowed: &[&str]) -> Result<()> {
        for (name, _) in self.object()? {
            if !allowed.contains(&name.as_str()) {
                bail!("unknown field {name:?}");
            }
        }
        Ok(())
    }
    fn json(&self) -> Result<serde_json::Value> {
        Ok(match self {
            Self::Null => serde_json::Value::Null,
            Self::Bool(value) => (*value).into(),
            Self::Int(value) => (*value).into(),
            Self::Float(value) => {
                serde_json::Value::Number(Number::from_f64(*value).context("non-finite number")?)
            }
            Self::String(value) => value.clone().into(),
            Self::Array(values) => {
                serde_json::Value::Array(values.iter().map(Self::json).collect::<Result<_>>()?)
            }
            Self::Object(fields) => {
                let mut result = Map::new();
                for (key, value) in fields {
                    result.insert(key.clone(), value.json()?);
                }
                serde_json::Value::Object(result)
            }
        })
    }
}

#[derive(Deserialize, Debug)]
struct Builtin {
    id: String,
    default: String,
    formats: BTreeMap<String, String>,
    alternatives: Vec<String>,
    manifest_key: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Output {
    path: PathBuf,
    label: String,
    format: String,
    bytes: Vec<u8>,
}

impl Output {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn label(&self) -> &str {
        &self.label
    }
    pub(crate) fn format(&self) -> &str {
        &self.format
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Debug)]
pub(crate) struct Plan {
    outputs: Vec<Output>,
    alternatives: Vec<PathBuf>,
    inputs: Inputs,
}

impl Plan {
    pub(crate) fn outputs(&self) -> &[Output] {
        &self.outputs
    }
    pub(crate) fn alternatives(&self) -> &[PathBuf] {
        &self.alternatives
    }
    pub(crate) fn inputs(&self) -> &Inputs {
        &self.inputs
    }
}

pub(crate) fn compile(source: &Source, evaluation: &Evaluation) -> Result<Plan> {
    let catalog: Vec<Builtin> = serde_json::from_str(include_str!("../pkl/catalog.json"))?;
    let document = evaluation.value();
    document.check_fields(&["tools", "files"])?;
    let mut plan = Plan {
        outputs: Vec::new(),
        alternatives: Vec::new(),
        inputs: evaluation.inputs().clone(),
    };
    if let Some(tools) = document.get("tools") {
        for (name, tool) in tools.object()? {
            compile_tool(source.root(), name, tool, &catalog, &mut plan)
                .with_context(|| format!("tool {name:?}"))?;
        }
    }
    if let Some(files) = document.get("files") {
        compile_files(source.root(), "files", files, &mut plan)?;
    }
    let mut seen = BTreeMap::new();
    for output in &plan.outputs {
        let normalized = path_key(&output.path);
        if let Some(previous) = seen.insert(normalized, &output.label) {
            bail!(
                "{} and {previous} both write {}",
                output.label,
                output.path.display()
            );
        }
        if plan.inputs.contains(&output.path) || output.path == source.path() {
            bail!(
                "{} collides with a configuration input",
                output.path.display()
            );
        }
        let state = path_key(&source.root().join(".confset"));
        if path_key(&output.path) == state
            || output
                .path
                .ancestors()
                .skip(1)
                .any(|ancestor| path_key(ancestor) == state)
        {
            bail!(
                "{} collides with Confset's internal state",
                output.path.display()
            );
        }
        if output
            .path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".confset-stage-"))
        {
            bail!(
                "{} uses Confset's reserved staging prefix",
                output.path.display()
            );
        }
        for ancestor in output.path.ancestors().skip(1) {
            if plan
                .outputs
                .iter()
                .any(|other| path_key(&other.path) == path_key(ancestor))
            {
                bail!(
                    "{} is both an output file and an output directory",
                    ancestor.display()
                );
            }
        }
    }
    // Two configured producers cannot evade native exclusivity by owning both alternatives.
    for output in &plan.outputs {
        if plan.alternatives.contains(&output.path) {
            bail!(
                "{} conflicts with another configured native configuration",
                output.path.display()
            );
        }
    }
    plan.outputs.sort_by(|a, b| a.path.cmp(&b.path));
    plan.alternatives.sort();
    plan.alternatives.dedup();
    Ok(plan)
}

fn compile_tool(
    root: &Path,
    name: &str,
    tool: &Value,
    catalog: &[Builtin],
    plan: &mut Plan,
) -> Result<()> {
    tool.check_fields(&["builtin", "format", "directory", "path", "config", "files"])?;
    let directory = absolute(root, Path::new(tool.text("directory")?.unwrap_or(".")))?;
    let id = tool.text("builtin")?.unwrap_or("");
    let requested = tool.text("format")?.unwrap_or("");
    let explicit = tool.text("path")?;
    if !id.is_empty() {
        let builtin = catalog
            .iter()
            .find(|entry| entry.id == id)
            .with_context(|| format!("unknown builtin {id:?}"))?;
        let format = if requested.is_empty() {
            &builtin.default
        } else {
            requested
        };
        let filename = builtin.formats.get(format).with_context(|| {
            format!(
                "{id} does not support generating {format} configurations; supported formats: {}",
                builtin
                    .formats
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        if explicit.is_some_and(|path| path != filename) {
            bail!(
                "builtin {id} requires native filename {filename}; set directory for nested projects"
            );
        }
        let path = absolute(&directory, Path::new(filename))?;
        for alternative in &builtin.alternatives {
            let candidate = absolute(&directory, Path::new(alternative))?;
            if candidate != path {
                plan.alternatives.push(candidate);
            }
        }
        if let Some(key) = &builtin.manifest_key {
            let manifest = directory.join("package.json");
            if let Some(bytes) = plan.inputs.inspect(&manifest)? {
                let value: serde_json::Value = serde_json::from_slice(&bytes)
                    .with_context(|| format!("cannot inspect {}", manifest.display()))?;
                if value.get(key).is_some() {
                    bail!(
                        "{} contains {key:?}, which competes with the generated configuration",
                        manifest.display()
                    );
                }
            }
        }
        push_output(
            plan,
            path,
            format!("{name}/{filename}"),
            format,
            tool.get("config").context("missing config")?,
        )?;
    } else if let Some(path) = explicit {
        if requested.is_empty() {
            bail!("a custom output needs a format");
        }
        push_output(
            plan,
            absolute(&directory, Path::new(path))?,
            format!("{name}/{path}"),
            requested,
            tool.get("config").context("missing config")?,
        )?;
    } else if tool
        .get("files")
        .is_none_or(|files| files.object().is_ok_and(|values| values.is_empty()))
    {
        bail!("a tool needs a builtin, a path, or additional files");
    }
    if let Some(files) = tool.get("files") {
        compile_files(&directory, name, files, plan)?;
    }
    Ok(())
}

fn compile_files(root: &Path, owner: &str, files: &Value, plan: &mut Plan) -> Result<()> {
    for (name, file) in files.object()? {
        file.check_fields(&["path", "format", "config"])?;
        let path = file
            .text("path")?
            .with_context(|| format!("{owner}/{name}: missing path"))?;
        push_output(
            plan,
            absolute(root, Path::new(path))?,
            format!("{owner}/{name}"),
            file.text("format")?.unwrap_or("json"),
            file.get("config").context("missing config")?,
        )?;
    }
    Ok(())
}

fn push_output(
    plan: &mut Plan,
    path: PathBuf,
    label: String,
    format: &str,
    value: &Value,
) -> Result<()> {
    let text = render(format, value).with_context(|| format!("rendering {label} as {format}"))?;
    plan.outputs.push(Output {
        path,
        label,
        format: format.into(),
        bytes: text.into_bytes(),
    });
    Ok(())
}

fn render(format: &str, value: &Value) -> Result<String> {
    match format {
        "json" => Ok(serde_json::to_string_pretty(&value.json()?)? + "\n"),
        "yaml" => Ok(serde_yaml_ng::to_string(&value.json()?)?),
        "toml" => {
            let value = toml::Value::try_from(value.json()?)
                .context("value is not representable in TOML")?;
            Ok(toml::to_string_pretty(&value)?)
        }
        "ini" => render_ini(&value.json()?),
        "text" => Ok(value.string()?.to_owned()),
        _ => bail!(
            "unsupported format {format:?}; use a Pkl renderer and format = \"text\" for custom formats"
        ),
    }
}

fn render_ini(value: &serde_json::Value) -> Result<String> {
    fn scalar(value: &serde_json::Value) -> Result<String> {
        match value {
            serde_json::Value::String(value) => {
                if value.trim() != value || value.contains(['\r', '\n', '\0']) {
                    bail!("INI strings cannot contain line breaks, NUL, or surrounding whitespace");
                }
                Ok(value.clone())
            }
            serde_json::Value::Number(_) | serde_json::Value::Bool(_) => Ok(value.to_string()),
            _ => bail!("INI entries must be strings, numbers, or booleans"),
        }
    }
    fn entry(result: &mut String, key: &str, value: &serde_json::Value) -> Result<()> {
        if key.is_empty()
            || key.trim() != key
            || key.contains(['=', ':', '\r', '\n', '\0'])
            || key.starts_with(['#', ';', '['])
        {
            bail!("invalid INI key {key:?}");
        }
        result.push_str(&format!("{key} = {}\n", scalar(value)?));
        Ok(())
    }
    let object = value
        .as_object()
        .context("INI requires an object of entries and sections")?;
    let mut result = String::new();
    for (key, value) in object.iter().filter(|(_, value)| !value.is_object()) {
        entry(&mut result, key, value)?;
    }
    for (section, value) in object.iter().filter(|(_, value)| value.is_object()) {
        if section.is_empty() || section.contains(['[', ']', '\r', '\n', '\0']) {
            bail!("invalid INI section {section:?}");
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&format!("[{section}]\n"));
        for (key, value) in value.as_object().context("INI section must be an object")? {
            entry(&mut result, key, value)?;
        }
    }
    Ok(result)
}

fn path_key(path: &Path) -> String {
    let key = path.to_string_lossy();
    if cfg!(any(windows, target_os = "macos")) {
        key.to_lowercase()
    } else {
        key.into_owned()
    }
}
