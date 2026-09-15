//! The five command workflows and their shared compilation path.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::config::{self, Discovery, Source};
use crate::{compiler, publication, watch};

#[derive(Debug, Parser)]
#[command(
    name = "confset",
    version,
    about = "Generate native tool configuration from Pkl",
    disable_help_subcommand = true
)]
struct Arguments {
    /// Select a Pkl file instead of normal configuration discovery.
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a starter configuration, preserving an existing file.
    Init {
        /// Establish a managed block for generated files in .gitignore.
        #[arg(long)]
        gitignore: bool,
    },
    /// Create or update native configuration files.
    Generate {
        /// Regenerate when the configuration or local dependencies change.
        #[arg(long)]
        watch: bool,
    },
    /// Evaluate and compile without writing generated configuration files.
    Validate,
    /// List configured outputs, formats, and ownership status.
    List,
    /// Remove unmodified generated files using ownership records.
    Clean,
}

pub(crate) fn run() -> Result<()> {
    let arguments = Arguments::parse();
    let mode = match arguments.command {
        Command::Init { .. } => Discovery::Initialize,
        Command::Clean => Discovery::Cleanup,
        _ => Discovery::Existing,
    };
    let source = Source::discover(arguments.config.as_deref(), mode)?;
    match arguments.command {
        Command::Init { gitignore } => {
            let created = source.initialize()?;
            if gitignore {
                let evaluation = config::evaluate(&source)?;
                let plan = compiler::compile(&source, &evaluation)?;
                publication::Project::acquire(source.root())?.establish_ignore(&plan)?;
            }
            println!(
                "{} {}",
                if created { "Created" } else { "Using" },
                source.path().display()
            );
        }
        Command::Generate { watch: true } => watch::run(&source)?,
        Command::Generate { watch: false } => {
            let project = publication::Project::acquire(source.root())?;
            let evaluation = config::evaluate(&source)?;
            let plan = compiler::compile(&source, &evaluation)?;
            project.generate(&plan)?;
        }
        Command::Validate => {
            let evaluation = config::evaluate(&source)?;
            let plan = compiler::compile(&source, &evaluation)?;
            publication::validate(source.root(), &plan)?;
            println!("Valid: {} generated files", plan.outputs().len());
        }
        Command::List => {
            let evaluation = config::evaluate(&source)?;
            let plan = compiler::compile(&source, &evaluation)?;
            println!("STATUS\tFORMAT\tOUTPUT\tDEFINITION");
            for output in plan.outputs() {
                println!(
                    "{}\t{}\t{}\t{}",
                    publication::status(source.root(), output.path())?,
                    output.format(),
                    output.path().display(),
                    output.label()
                );
            }
        }
        Command::Clean => publication::Project::acquire(source.root())?.clean(source.path())?,
    }
    Ok(())
}
