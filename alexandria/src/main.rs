//! Entrypoint for the alexandria compiler's executable.

use std::{collections::HashSet, error::Error, ops::ControlFlow, process::ExitCode};

use clap::Parser;
use clap_derive::ValueEnum;
use diagnostic::Diagnostics;
use lexer::Intern;
use nameres::{
    NameResTable, ScopeArena,
    resolver::{ResolutionTable, Resolver, SubscopeTable},
};
use parser::{Parser as CParser, ast_table::AstTable, crate_table::CrateTable};
use source::{SourceFile, SourceIdx, SourceMap};

#[derive(Parser)]
#[command(version, about)]
#[expect(missing_docs)]
pub struct Cli {
    /// The entrypoint file to compile (e.g., main.aa).
    entrypoint: String,
    /// Which compiler stages to debug.
    #[arg(long, short, value_delimiter = ',', value_enum)]
    debug: Vec<CompilerStage>,
    /// Further crates to compile against.
    #[arg(long, short, value_enum, value_parser = parse_key_val::<String, String>)]
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
        .ok_or_else(|| format!("invalid KEY=value: no `=` found in `{}`", s))?;

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

fn main() -> Result<(), ExitCode> {
    setup_panic();
    let cli = Cli::parse();
    let debug_stages: HashSet<_> = cli.debug.into_iter().collect();
    dbg!(&debug_stages);

    let mut diagnostics: Diagnostics = Diagnostics::default();
    let mut sources = SourceMap::default();
    let entrypoint = load_entrypoint(&mut sources, cli.entrypoint)?;

    let ast_table = AstTable::default();
    let crate_table = CrateTable::default();

    for (cr, crf) in cli.crates {
        let Ok(source_file) = SourceFile::from_disk(crf) else {
            eprintln!("unable to open entrypoint for crate '{cr}'");
            return Err(ExitCode::FAILURE);
        };

        let idx = sources.insert(source_file);

        crate_table.insert(Intern::from(cr.as_str()), idx);
    }

    if let ControlFlow::Break(..) =
        diagnostic_guard(diagnostics.clone(), sources.clone(), |diag, sources| {
            CParser::new(
                sources,
                entrypoint,
                diag.clone(),
                ast_table.clone(),
                crate_table.clone(),
                Intern::from("test"),
            )
            .unwrap()
            .parse();

            match crate_table
                .status_by_name(Intern::from("test"))
                .unwrap()
                .get()
                .unwrap()
            {
                Ok(_) => Ok(()),
                Err(e) => {
                    e.display(entrypoint, diag);
                    Err(ExitCode::FAILURE)
                }
            }
        })
    {
        return Err(ExitCode::FAILURE);
    }

    if debug_stages.contains(&CompilerStage::Parser) {
        println!("AST: ");
        println!("{ast_table:#?}");
    }

    let mut scope_arena = ScopeArena::default();
    let mut nrt = NameResTable::default();
    let mut subscopes = SubscopeTable::default();
    let mut res_table = ResolutionTable::default();
    if Resolver::new(
        &mut scope_arena,
        &mut nrt,
        &mut subscopes,
        &mut diagnostics,
        &mut res_table,
        &ast_table,
        crate_table,
        entrypoint,
    )
    .fill()
    .is_err()
    {
        print_diagnostics(diagnostics, sources);
        return Err(ExitCode::FAILURE);
    };

    if debug_stages.contains(&CompilerStage::NameRes) {
        println!("Nameres: ");
        println!("Scope arena: {scope_arena:#?}");
        println!("NRT: {nrt:#?}");
        println!("SS table: {subscopes:#?}");
        println!("Resolution table: {subscopes:#?}");
    }

    Ok(())
}

fn diagnostic_guard<F, T>(diagnostics: Diagnostics, source_map: SourceMap, f: F) -> ControlFlow<()>
where
    F: FnOnce(Diagnostics, SourceMap) -> T,
{
    let diagnostics_before = diagnostics.len();
    f(diagnostics.clone(), source_map.clone());
    if diagnostics_before != diagnostics.len() {
        print_diagnostics(diagnostics, source_map);
        ControlFlow::Break(())
    } else {
        ControlFlow::Continue(())
    }
}

fn print_diagnostics(diagnostics: Diagnostics, source_map: SourceMap) {
    diagnostics.write_stderr(source_map).unwrap();
}

fn load_entrypoint(sources: &mut SourceMap, entrypoint: String) -> Result<SourceIdx, ExitCode> {
    let Ok(file) = SourceFile::from_disk(entrypoint) else {
        return Err(ExitCode::FAILURE);
    };
    Ok(sources.insert(file))
}

fn setup_panic() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!(" The compiler panicked, this is a bug and must be reported");
        eprintln!("+++++++++++++++++++++++++++++++++++++++++++++++++++++++++++");
        eprintln!("Info: ");
        eprintln!("Panic at {}", info.location().unwrap());
        eprintln!("Payload: \n{}", info.payload_as_str().unwrap());
    }));
}
