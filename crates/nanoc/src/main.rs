//! `nanoc`, the nanobuf schema compiler.

mod driver;
mod layout;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, Subcommand};
use nanobuf_diagnostics::{Diagnostics, SourceMap};
use nanobuf_schema::Schema;

#[derive(Parser)]
#[command(name = "nanoc", version, about = "The nanobuf schema compiler")]
struct Cli {
    /// Disable colored output.
    #[arg(long, global = true)]
    no_color: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Type-check a schema and report all diagnostics.
    Check {
        /// Root .nb schema file.
        schema: PathBuf,
    },
    /// Generate code for one or more target languages.
    Build {
        /// Root .nb schema file.
        schema: PathBuf,
        /// Target languages, comma separated: rust, python, csharp, cpp.
        #[arg(long, value_delimiter = ',', required = true)]
        lang: Vec<String>,
        /// Output directory.
        #[arg(short, long, default_value = "gen")]
        out: PathBuf,
    },
    /// Print the wire layout of every message.
    Layout {
        /// Root .nb schema file.
        schema: PathBuf,
    },
    /// Convert a proto3 file to nanobuf IDL.
    Migrate {
        /// Input .proto file.
        proto: PathBuf,
        /// Output file; defaults to the input with a .nb extension.
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let color =
        !cli.no_color && std::env::var_os("NO_COLOR").is_none() && std::io::stderr().is_terminal();
    match cli.command {
        Command::Check { schema } => check(&schema, color),
        Command::Build { schema, lang, out } => build(&schema, &lang, &out, color),
        Command::Layout { schema } => layout_cmd(&schema, color),
        Command::Migrate { proto, out } => migrate(&proto, out.as_deref(), color),
    }
}

/// Runs the front half of the pipeline and prints diagnostics.
/// Returns the schema and source count on success.
fn compile(path: &Path, color: bool) -> Result<(Schema, usize), ExitCode> {
    let mut sources = SourceMap::new();
    let mut diags = Diagnostics::new();
    let units = driver::load(path, &mut sources, &mut diags);
    let schema = if diags.has_errors() {
        None
    } else {
        nanobuf_schema::analyze(&units, &mut diags)
    };
    eprint!("{}", diags.render(&sources, color));
    match schema {
        Some(schema) => Ok((schema, units.len())),
        None => Err(ExitCode::FAILURE),
    }
}

fn check(path: &Path, color: bool) -> ExitCode {
    let start = Instant::now();
    let Ok((schema, files)) = compile(path, color) else {
        return ExitCode::FAILURE;
    };
    println!(
        "ok: {files} file(s), {} message(s), {} enum(s) in {:.1?}",
        schema.messages.len(),
        schema.enums.len(),
        start.elapsed()
    );
    ExitCode::SUCCESS
}

fn build(path: &Path, langs: &[String], out: &Path, color: bool) -> ExitCode {
    let start = Instant::now();
    let mut targets = Vec::new();
    for name in langs {
        match nanobuf_codegen::Lang::parse(name) {
            Some(lang) => targets.push(lang),
            None => {
                eprintln!("error: unknown language `{name}` (expected rust, python, csharp, cpp)");
                return ExitCode::FAILURE;
            }
        }
    }
    let Ok((schema, _)) = compile(path, color) else {
        return ExitCode::FAILURE;
    };
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("schema");
    if let Err(err) = std::fs::create_dir_all(out) {
        eprintln!("error: cannot create `{}`: {err}", out.display());
        return ExitCode::FAILURE;
    }
    let mut written = 0;
    for lang in targets {
        let files = match nanobuf_codegen::generate(&schema, lang, stem) {
            Ok(files) => files,
            Err(err) => {
                eprintln!("error: {err}");
                return ExitCode::FAILURE;
            }
        };
        for file in files {
            let target = out.join(&file.name);
            if let Err(err) = std::fs::write(&target, file.content) {
                eprintln!("error: cannot write `{}`: {err}", target.display());
                return ExitCode::FAILURE;
            }
            written += 1;
        }
    }
    println!(
        "ok: wrote {written} file(s) to {} in {:.1?}",
        out.display(),
        start.elapsed()
    );
    ExitCode::SUCCESS
}

fn layout_cmd(path: &Path, color: bool) -> ExitCode {
    let Ok((schema, _)) = compile(path, color) else {
        return ExitCode::FAILURE;
    };
    print!("{}", layout::render(&schema));
    ExitCode::SUCCESS
}

fn migrate(path: &Path, out: Option<&Path>, color: bool) -> ExitCode {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("error: cannot read `{}`: {err}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let mut sources = SourceMap::new();
    let mut diags = Diagnostics::new();
    let file = sources.add(path.display().to_string(), text.clone());
    let result = nanobuf_proto::migrate(file, &text, &mut diags);
    eprint!("{}", diags.render(&sources, color));
    let Some(nb) = result else {
        return ExitCode::FAILURE;
    };
    let target = out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| path.with_extension("nb"));
    if let Err(err) = std::fs::write(&target, nb) {
        eprintln!("error: cannot write `{}`: {err}", target.display());
        return ExitCode::FAILURE;
    }
    println!("ok: wrote {}", target.display());
    ExitCode::SUCCESS
}
