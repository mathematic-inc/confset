//! Configuration discovery and Pkl evaluation with recorded input dependencies.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use pklr::capabilities::BoxFuture;
use pklr::{BlockingCapabilities, EvalCapabilities, Evaluator};
use sha2::{Digest, Sha256};

use crate::compiler::Value;

const EMBEDDED_PACKAGE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/confset-package.zip"));
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, Debug)]
pub(crate) enum Discovery {
    Existing,
    Initialize,
    Cleanup,
}

#[derive(Clone, Debug)]
pub(crate) struct Source {
    path: PathBuf,
    root: PathBuf,
}

impl Source {
    pub(crate) fn discover(requested: Option<&Path>, mode: Discovery) -> Result<Self> {
        let cwd = std::env::current_dir()
            .context("cannot read the current directory")?
            .canonicalize()?;
        let selected = requested
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("CONFSET_CONFIG").map(PathBuf::from));
        if let Some(path) = selected {
            if path.as_os_str().is_empty() {
                bail!("the explicit configuration path is empty");
            }
            let path = resolve_source_path(&absolute(&cwd, &path)?)?;
            if matches!(mode, Discovery::Existing) && !path.is_file() {
                bail!("configuration does not exist: {}", path.display());
            }
            let root = path
                .parent()
                .context("configuration needs a parent directory")?;
            return Ok(Self {
                root: root.to_path_buf(),
                path,
            });
        }
        for ancestor in cwd.ancestors() {
            let path = ancestor.join("confset.pkl");
            if path
                .try_exists()
                .with_context(|| format!("cannot inspect {}", path.display()))?
            {
                return Ok(Self {
                    path,
                    root: ancestor.to_path_buf(),
                });
            }
            if matches!(mode, Discovery::Cleanup) && ancestor.join(".confset/state.json").is_file()
            {
                return Ok(Self {
                    path,
                    root: ancestor.to_path_buf(),
                });
            }
        }
        let user_directory = if cfg!(windows) {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .or_else(dirs::config_dir)
        } else {
            dirs::config_dir()
        };
        if let Some(user) = user_directory {
            let path = resolve_source_path(&user.join("confset/config.pkl"))?;
            if path
                .try_exists()
                .context("cannot inspect the user configuration")?
            {
                return Ok(Self { path, root: cwd });
            }
        }
        match mode {
            Discovery::Existing => bail!("no confset.pkl found; run confset init"),
            Discovery::Initialize | Discovery::Cleanup => Ok(Self {
                path: cwd.join("confset.pkl"),
                root: cwd,
            }),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn initialize(&self) -> Result<bool> {
        if self.path.try_exists()? {
            if !self.path.is_file() {
                bail!("configuration is not a file: {}", self.path.display());
            }
            return Ok(false);
        }
        std::fs::create_dir_all(&self.root)
            .with_context(|| format!("cannot create {}", self.root.display()))?;
        let text = format!(
            "amends \"{base}#/Config.pkl\"\n\nimport \"{base}#/Builtins.pkl\"\n\n// Enable and amend the configurations your project uses.\ntools {{}}\n",
            base = package_uri(),
        );
        use std::io::Write;
        let mut file = tempfile::Builder::new()
            .prefix(".confset-stage-")
            .tempfile_in(&self.root)?;
        file.write_all(text.as_bytes())?;
        file.as_file().sync_all()?;
        match file.persist_noclobber(&self.path) {
            Ok(_) => Ok(true),
            Err(error)
                if error.error.kind() == std::io::ErrorKind::AlreadyExists
                    && self.path.is_file() =>
            {
                Ok(false)
            }
            Err(error) => {
                Err(error.error).with_context(|| format!("cannot create {}", self.path.display()))
            }
        }
    }
}

// Resolve existing parents without requiring an explicitly requested new source to exist.
fn resolve_source_path(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .context("configuration needs a parent directory")?;
    for ancestor in parent.ancestors() {
        match ancestor.canonicalize() {
            Ok(real) => return Ok(real.join(path.strip_prefix(ancestor)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("cannot resolve {}", path.display()));
            }
        }
    }
    bail!("cannot resolve configuration path {}", path.display())
}

pub(crate) fn package_uri() -> String {
    format!(
        "package://github.com/mathematic-inc/confset/releases/download/v{VERSION}/confset@{VERSION}"
    )
}

pub(crate) fn absolute(base: &Path, path: &Path) -> Result<PathBuf> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    let mut result = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    bail!("path escapes its filesystem root: {}", joined.display());
                }
            }
            part => result.push(part.as_os_str()),
        }
    }
    if !result.is_absolute() {
        bail!("path is not absolute: {}", result.display());
    }
    Ok(result)
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Inputs {
    files: BTreeMap<PathBuf, Option<String>>,
    directories: BTreeSet<PathBuf>,
    globs: BTreeMap<(PathBuf, String), Vec<PathBuf>>,
    excluded: BTreeSet<PathBuf>,
}

impl Inputs {
    pub(crate) fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.files.keys()
    }
    pub(crate) fn directories(&self) -> impl Iterator<Item = &PathBuf> {
        self.directories.iter()
    }
    pub(crate) fn changed(&self) -> bool {
        self.files
            .iter()
            .any(|(path, old)| match (old, fingerprint(path)) {
                (Some(old), Ok(current)) => old != &current,
                (None, Err(_)) => path.try_exists().unwrap_or(true),
                _ => true,
            })
            || self.globs.iter().any(|((base, pattern), expected)| {
                let mut capabilities = BlockingCapabilities::new();
                pollster::block_on(capabilities.glob(base, pattern)).map_or(true, |paths| {
                    let paths = paths
                        .into_iter()
                        .filter(|path| !self.ignores(path))
                        .collect::<Vec<_>>();
                    &paths != expected
                })
            })
    }
    pub(crate) fn contains(&self, path: &Path) -> bool {
        self.files.contains_key(path)
            || path
                .canonicalize()
                .is_ok_and(|real| self.files.contains_key(&real))
    }

    pub(crate) fn ignores(&self, path: &Path) -> bool {
        self.excluded
            .iter()
            .any(|directory| path.starts_with(directory))
    }
    pub(crate) fn inspect(&mut self, path: &Path) -> Result<Option<Vec<u8>>> {
        match std::fs::read(path) {
            Ok(bytes) => {
                self.files.insert(path.to_path_buf(), Some(digest(&bytes)));
                Ok(Some(bytes))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.files.insert(path.to_path_buf(), None);
                Ok(None)
            }
            Err(error) => Err(error).with_context(|| format!("cannot inspect {}", path.display())),
        }
    }
}

pub(crate) fn fingerprint(path: &Path) -> Result<String> {
    Ok(digest(&std::fs::read(path).with_context(|| {
        format!("cannot read {}", path.display())
    })?))
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) struct Evaluation {
    value: Value,
    inputs: Inputs,
}

impl Evaluation {
    pub(crate) fn value(&self) -> &Value {
        &self.value
    }
    pub(crate) fn inputs(&self) -> &Inputs {
        &self.inputs
    }
}

pub(crate) struct Attempt {
    value: Result<Value>,
    inputs: Inputs,
}
impl Attempt {
    pub(crate) fn inputs(&self) -> &Inputs {
        &self.inputs
    }
    pub(crate) fn finish(self) -> Result<Evaluation> {
        Ok(Evaluation {
            value: self.value?,
            inputs: self.inputs,
        })
    }
}
pub(crate) fn evaluate(source: &Source) -> Result<Evaluation> {
    evaluate_attempt(source).finish()
}
pub(crate) fn evaluate_attempt(source: &Source) -> Attempt {
    let recorded = Arc::new(Mutex::new(Inputs::default()));
    let value = (|| -> Result<Value> {
        let cache = std::env::var_os("CONFSET_PKL_CACHE_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs::cache_dir().map(|path| path.join("confset/pkl")))
            .context("cannot locate a package cache; set CONFSET_PKL_CACHE_DIR")?;
        // A development rebuild must not reuse different package bytes under the same version.
        let cache = absolute(source.root(), &cache)?.join(digest(EMBEDDED_PACKAGE));
        std::fs::create_dir_all(&cache).context("cannot create the Pkl package cache")?;
        let cache = cache
            .canonicalize()
            .context("cannot resolve the package cache")?;
        recorded
            .lock()
            .map_err(|_| anyhow::anyhow!("input tracking failed"))?
            .excluded
            .insert(cache.clone());
        let capabilities = Tracking {
            inner: BlockingCapabilities::new(),
            recorded: Arc::clone(&recorded),
            root: source.root.clone(),
            cache: cache.clone(),
            temporary: Vec::new(),
            config_dir: source
                .path
                .parent()
                .context("configuration has no parent")?
                .to_path_buf(),
        };
        let mut evaluator = Evaluator::with_capabilities(capabilities);
        evaluator.set_package_cache_dir(cache);
        evaluator.set_offline(env_flag("CONFSET_PKL_OFFLINE")?);
        let archive = format!(
            "https://github.com/mathematic-inc/confset/releases/download/v{VERSION}/confset@{VERSION}.zip"
        );
        evaluator
            .preload_package(&archive, "zip", EMBEDDED_PACKAGE)
            .context("cannot load the embedded Pkl package")?;
        evaluator.set_base_path(
            source
                .path
                .parent()
                .context("configuration has no parent")?,
        );
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pollster::block_on(evaluator.eval_file_pub(&source.path))
        }));
        let raw = result
            .map_err(|_| {
                anyhow::anyhow!(
                    "the Pkl evaluator panicked while evaluating {}; no outputs were written",
                    source.path.display()
                )
            })?
            .map_err(pkl_error)
            .with_context(|| format!("cannot evaluate {}", source.path.display()))?;
        Value::from_pkl(&raw, "configuration")
    })();
    match recorded.lock() {
        Ok(inputs) => Attempt {
            value,
            inputs: inputs.clone(),
        },
        Err(_) => Attempt {
            value: Err(anyhow::anyhow!("input tracking failed")),
            inputs: Inputs::default(),
        },
    }
}

fn env_flag(name: &str) -> Result<bool> {
    match std::env::var(name).as_deref() {
        Ok("1" | "true") => Ok(true),
        Ok("0" | "false" | "") | Err(std::env::VarError::NotPresent) => Ok(false),
        _ => bail!("{name} must be true, false, 1, or 0"),
    }
}

struct Tracking {
    inner: BlockingCapabilities,
    recorded: Arc<Mutex<Inputs>>,
    root: PathBuf,
    config_dir: PathBuf,
    cache: PathBuf,
    temporary: Vec<tempfile::TempDir>,
}

impl Tracking {
    fn record(&self, path: &Path, bytes: Option<&[u8]>) -> pklr::Result<()> {
        if path.starts_with(&self.cache) {
            return Ok(());
        }
        let mut inputs = self
            .recorded
            .lock()
            .map_err(|_| pklr::Error::Unsupported("input tracking failed".into()))?;
        let hash = bytes.map(digest);
        inputs.files.insert(path.to_path_buf(), hash.clone());
        if let Ok(real) = path.canonicalize() {
            inputs.files.insert(real, hash);
        }
        Ok(())
    }
}

impl EvalCapabilities for Tracking {
    fn read_to_string<'a>(&'a mut self, path: &'a Path) -> BoxFuture<'a, pklr::Result<String>> {
        Box::pin(async move {
            let result = self.inner.read_to_string(path).await;
            self.record(path, result.as_ref().ok().map(String::as_bytes))?;
            result
        })
    }
    fn read_bytes<'a>(&'a mut self, path: &'a Path) -> BoxFuture<'a, pklr::Result<Vec<u8>>> {
        Box::pin(async move {
            let result = self.inner.read_bytes(path).await;
            self.record(path, result.as_ref().ok().map(Vec::as_slice))?;
            result
        })
    }
    fn path_exists<'a>(&'a mut self, path: &'a Path) -> BoxFuture<'a, pklr::Result<bool>> {
        Box::pin(async move {
            let exists = self.inner.path_exists(path).await?;
            if !exists {
                self.record(path, None)?;
            }
            Ok(exists)
        })
    }
    fn canonicalize<'a>(&'a mut self, path: &'a Path) -> BoxFuture<'a, pklr::Result<PathBuf>> {
        self.inner.canonicalize(path)
    }
    fn read_env<'a>(&'a mut self, name: &'a str) -> BoxFuture<'a, pklr::Result<Option<String>>> {
        Box::pin(async move {
            match name {
                "CONFSET_PROJECT_DIR" => Ok(Some(self.root.to_string_lossy().into_owned())),
                "CONFSET_CONFIG_DIR" => Ok(Some(self.config_dir.to_string_lossy().into_owned())),
                _ => self.inner.read_env(name).await,
            }
        })
    }
    fn fetch_text<'a>(&'a mut self, url: &'a str) -> BoxFuture<'a, pklr::Result<String>> {
        self.inner.fetch_text(url)
    }
    fn fetch_bytes<'a>(&'a mut self, url: &'a str) -> BoxFuture<'a, pklr::Result<Vec<u8>>> {
        self.inner.fetch_bytes(url)
    }
    fn temp_dir<'a>(&'a mut self, prefix: &'a str) -> BoxFuture<'a, pklr::Result<PathBuf>> {
        Box::pin(async move {
            let directory = tempfile::Builder::new()
                .prefix(prefix)
                .tempdir_in(&self.cache)
                .map_err(|error| pklr::Error::Io(self.cache.clone(), error))?;
            let path = directory.path().to_path_buf();
            self.temporary.push(directory);
            Ok(path)
        })
    }
    fn glob<'a>(
        &'a mut self,
        base: &'a Path,
        pattern: &'a str,
    ) -> BoxFuture<'a, pklr::Result<Vec<PathBuf>>> {
        Box::pin(async move {
            let paths = self
                .inner
                .glob(base, pattern)
                .await?
                .into_iter()
                .filter(|path| !path.starts_with(&self.cache))
                .collect::<Vec<_>>();
            let mut recorded = self
                .recorded
                .lock()
                .map_err(|_| pklr::Error::Unsupported("input tracking failed".into()))?;
            recorded.directories.insert(base.to_path_buf());
            recorded
                .globs
                .insert((base.to_path_buf(), pattern.to_owned()), paths.clone());
            Ok(paths)
        })
    }
}

fn pkl_error(error: pklr::Error) -> anyhow::Error {
    match &error {
        pklr::Error::Parse { src, span, .. } | pklr::Error::Lex { src, span, .. } => {
            let prefix = src.inner().get(..span.offset()).unwrap_or("");
            let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
            let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
            anyhow::anyhow!("{}:{line}:{column}: {error}", src.name())
        }
        _ => anyhow::Error::new(error),
    }
}
