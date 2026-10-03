//! Entrypoint for the alexandria compiler's executable.

use std::{collections::HashSet, error::Error, path::Path, process::ExitCode};

use clap::{Parser, ValueEnum};
use diagnostic::Diagnostics;
use lexer::Intern;
use nameres::resolver::Resolver;
use parser::{Parser as CParser, ast_table::AstTable, crate_table::CrateTable};
use source::{SourceFile, SourceIdx, SourceMap};

#[derive(Parser)]
#[command(version, about)]
#[expect(missing_docs)]
pub struct Cli {
    /// The entrypoint file to compile (e.g., main.aa).
    entrypoint: String,
    /// The name of the crate being compiled. Defaults to the name of the entrypoint file.
    #[arg(long)]
    crate_name: Option<String>,
    /// Which compiler stages to debug.
    #[arg(long, short, value_delimiter = ',', value_enum)]
    debug: Vec<CompilerStage>,
    /// Further crates to compile against, as `name=path/to/entrypoint.aa`.
    #[arg(long, short, value_parser = parse_key_val::<String, String>)]
    crates: Vec<(String, String)>,
}

fn parse_key_val<T, U>(s: &str) -> Result<(T, U), Box<dyn Error + Send + Sync + 'static>>
where
    T: std::str::FromStr,
    T::Err: Error + Send + Sync + 'static,
    U: std::str::FromStr,
    U::Err: Error + Send + Sync + 'static,
{
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=value: no `=` found in `{s}`"))?;

    let key = s[..pos].parse()?;
    let value = s[pos + 1..].parse()?;
    Ok((key, value))
}

/// The compiler stage.
#[derive(Clone, ValueEnum, PartialEq, Eq, Hash, Debug)]
pub enum CompilerStage {
    /// The parser (and lexer) stage. This will display solely the AST table.
    Parser,
    /// The name resolution stage. This will display all internal tables filled by the name resolver.
    NameRes,
}

fn main() -> ExitCode {
    setup_panic();
    let cli = Cli::parse();
    let debug_stages: HashSet<_> = cli.debug.into_iter().collect();

    let diagnostics = Diagnostics::default();
    let sources = SourceMap::default();
    let ast_table = AstTable::default();
    let crate_table = CrateTable::default();

    let Some(entrypoint) = load_file(&sources, &cli.entrypoint) else {
        return ExitCode::FAILURE;
    };

    let crate_name = cli.crate_name.unwrap_or_else(|| {
        Path::new(&cli.entrypoint)
            .file_stem()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_else(|| "main".to_owned())
    });
    let entry_crate = crate_table
        .insert(Intern::from(crate_name.as_str()), entrypoint)
        .expect("the crate table is empty");

    for (name, path) in cli.crates {
        let Some(root) = load_file(&sources, &path) else {
            return ExitCode::FAILURE;
        };

        if crate_table
            .insert(Intern::from(name.as_str()), root)
            .is_none()
        {
            eprintln!("error: a crate named `{name}` was specified more than once");
            return ExitCode::FAILURE;
        }
    }

    // parse the entry crate, and every crate it (transitively) includes
    crate_table.request(entry_crate);
    while let Some(id) = crate_table.next_requested() {
        CParser::new(
            sources.clone(),
            id,
            diagnostics.clone(),
            ast_table.clone(),
            crate_table.clone(),
        )
        .parse();
    }

    if diagnostics.error_count() > 0 {
        return fail(&diagnostics, &sources);
    }

    if debug_stages.contains(&CompilerStage::Parser) {
        println!("AST: ");
        println!("{ast_table:#?}");
    }

    let Ok(output) =
        Resolver::new(&ast_table, crate_table, entry_crate, diagnostics.clone()).fill()
    else {
        return fail(&diagnostics, &sources);
    };

    if debug_stages.contains(&CompilerStage::NameRes) {
        println!("Nameres: ");
        println!("Scope arena: {:#?}", output.arena);
        println!("NRT: {:#?}", output.nrt);
        println!("SS table: {:#?}", output.subscopes);
        println!("Resolution table: {:#?}", output.resolutions);
    }

    // warnings, if any
    print_diagnostics(&diagnostics, &sources);
    ExitCode::SUCCESS
}

fn load_file(sources: &SourceMap, path: &str) -> Option<SourceIdx> {
    match SourceFile::from_disk(path) {
        Ok(file) => Some(sources.insert(file)),
        Err(e) => {
            eprintln!("error: failed to load `{path}`: {e}");
            None
        }
    }
}

fn fail(diagnostics: &Diagnostics, sources: &SourceMap) -> ExitCode {
    print_diagnostics(diagnostics, sources);

    let errors = diagnostics.error_count();
    let plural = if errors == 1 { "" } else { "s" };
    eprintln!("error: aborting due to {errors} previous error{plural}");

    ExitCode::FAILURE
}

fn print_diagnostics(diagnostics: &Diagnostics, sources: &SourceMap) {
    // there is nothing sensible left to do if stderr is unavailable
    _ = diagnostics.write_stderr(sources);
}

fn setup_panic() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!(" The compiler panicked, this is a bug and must be reported");
        eprintln!("+++++++++++++++++++++++++++++++++++++++++++++++++++++++++++");
        eprintln!("Info: ");
        if let Some(location) = info.location() {
            eprintln!("Panic at {location}");
        }
        eprintln!(
            "Payload: \n{}",
            info.payload_as_str().unwrap_or("<non-string payload>")
        );
    }));
}
