//! Watching source dependencies while one writer owns the project.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::config::{self, Inputs, Source};
use crate::{compiler, publication};

/// Coalesces atomic-save rename/write bursts without delaying interactive updates.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(150);

enum Event {
    Files(notify::Result<notify::Event>),
    Compiled(Box<Compilation>),
    Stop,
}

struct Compilation {
    inputs: Inputs,
    result: Result<compiler::Plan>,
}

pub(crate) fn run(source: &Source) -> Result<()> {
    let project = publication::Project::acquire(source.root())?;
    let (sender, receiver) = mpsc::channel();
    let stop_sender = sender.clone();
    ctrlc::set_handler(move || {
        let _ = stop_sender.send(Event::Stop);
    })
    .context("cannot install the shutdown handler")?;
    let watch_sender = sender.clone();
    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |event| {
        let _ = watch_sender.send(Event::Files(event));
    })
    .context("cannot start the filesystem watcher")?;
    let mut inputs = Inputs::default();
    let mut watched = BTreeMap::new();
    update_watches(&mut watcher, &mut watched, source, &inputs)?;
    let mut outputs = Vec::new();
    let mut compiling = false;
    let mut deadline = Some(Instant::now());
    eprintln!("confset: watching {}", source.path().display());
    loop {
        if !compiling && deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            start_compilation(source.clone(), sender.clone())?;
            compiling = true;
            deadline = None;
        }
        let event = match deadline.filter(|_| !compiling) {
            Some(deadline) => {
                match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(event) => event,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            None => receiver.recv().context("filesystem watcher stopped")?,
        };
        match event {
            Event::Stop => break,
            Event::Files(Err(error)) => eprintln!("confset: watcher error: {error}"),
            Event::Files(Ok(event)) => {
                if relevant(&event, source, &inputs, &outputs) {
                    deadline = Some(Instant::now() + SAVE_DEBOUNCE);
                }
            }
            Event::Compiled(compilation) => {
                compiling = false;
                inputs = compilation.inputs;
                // Attach newly discovered dependencies before announcing publication.
                // A fingerprint check closes the evaluation-to-watch registration gap.
                update_watches(&mut watcher, &mut watched, source, &inputs)?;
                if deadline.is_some() || inputs.changed() {
                    deadline.get_or_insert(Instant::now() + SAVE_DEBOUNCE);
                    continue;
                }
                let result = compilation.result.and_then(|plan| {
                    outputs = plan
                        .outputs()
                        .iter()
                        .map(|output| output.path().to_path_buf())
                        .collect();
                    project.generate(&plan)
                });
                if let Err(error) = result {
                    if inputs.changed() {
                        deadline = Some(Instant::now() + SAVE_DEBOUNCE);
                    } else {
                        eprintln!(
                            "confset: {error:#}\nconfset: keeping the last successful outputs; waiting for a change"
                        );
                    }
                }
            }
        }
    }
    eprintln!("confset: stopped watching; generated files remain in place");
    Ok(())
}

fn start_compilation(source: Source, sender: mpsc::Sender<Event>) -> Result<()> {
    // Only one compiler runs at a time. Filesystem and stop events stay responsive;
    // a pending save supersedes this result and starts another compilation afterward.
    std::thread::Builder::new()
        .name("confset-compiler".into())
        .spawn(move || {
            let attempt = config::evaluate_attempt(&source);
            let mut inputs = attempt.inputs().clone();
            let result = attempt
                .finish()
                .and_then(|evaluation| compiler::compile(&source, &evaluation));
            if let Ok(plan) = &result {
                inputs = plan.inputs().clone();
            }
            let _ = sender.send(Event::Compiled(Box::new(Compilation { inputs, result })));
        })
        .context("cannot start the configuration compiler")?;
    Ok(())
}

fn update_watches(
    watcher: &mut RecommendedWatcher,
    watched: &mut BTreeMap<PathBuf, RecursiveMode>,
    source: &Source,
    inputs: &Inputs,
) -> Result<()> {
    let mut desired = BTreeMap::new();
    for path in std::iter::once(source.path()).chain(inputs.paths().map(PathBuf::as_path)) {
        let mut directory = path.parent().unwrap_or(source.root());
        while !directory.is_dir() {
            let Some(parent) = directory.parent() else {
                break;
            };
            directory = parent;
        }
        desired
            .entry(directory.to_path_buf())
            .or_insert(RecursiveMode::NonRecursive);
    }
    for base in inputs.directories() {
        let directory = base
            .ancestors()
            .find(|path| path.is_dir())
            .unwrap_or(source.root());
        desired.insert(directory.to_path_buf(), RecursiveMode::Recursive);
    }
    let recursive = desired
        .iter()
        .filter(|(_, mode)| **mode == RecursiveMode::Recursive)
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    desired.retain(|path, _| {
        !recursive
            .iter()
            .any(|parent| path != parent && path.starts_with(parent))
    });
    for (directory, mode) in &desired {
        if watched.get(directory) == Some(mode) {
            continue;
        }
        if watched.contains_key(directory) {
            watcher.unwatch(directory)?;
        }
        watcher
            .watch(directory, *mode)
            .with_context(|| format!("cannot watch {}", directory.display()))?;
    }
    for directory in watched
        .keys()
        .filter(|directory| !desired.contains_key(*directory))
    {
        watcher
            .unwatch(directory)
            .with_context(|| format!("cannot stop watching {}", directory.display()))?;
    }
    *watched = desired;
    Ok(())
}

fn relevant(event: &notify::Event, source: &Source, inputs: &Inputs, outputs: &[PathBuf]) -> bool {
    if matches!(event.kind, notify::EventKind::Access(_)) {
        return false;
    }
    event.paths.iter().any(|path| {
        if inputs.ignores(path)
            || path.starts_with(source.root().join(".confset"))
            || outputs.contains(path)
            || path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".confset-stage-"))
        {
            return false;
        }
        path == source.path()
            || inputs
                .paths()
                .any(|input| input == path || input.starts_with(path))
            || inputs
                .directories()
                .any(|directory| path.starts_with(directory))
    })
}
