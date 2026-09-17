//! Entrypoint for the alexandria compiler's executable.

use std::{collections::HashSet, ops::ControlFlow, process::ExitCode};

use clap::Parser;
use clap_derive::ValueEnum;
use diagnostic::Diagnostics;
use nameres::{
    NameResTable, ScopeArena,
    resolver::{ResolutionTable, Resolver, SubscopeTable},
};
use parser::{AstTable, Parser as CParser};
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
    if let ControlFlow::Break(..) = diagnostic_guard(
        &mut diagnostics,
        &mut sources,
        |diag, sources| match CParser::new(sources, entrypoint, diag, &ast_table).parse() {
            Ok(_) => Ok(()),
            Err(e) => {
                e.display(entrypoint, diag);
                Err(ExitCode::FAILURE)
            }
        },
    ) {
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
        entrypoint,
    )
    .fill()
    .is_err()
    {
        print_diagnostics(&diagnostics, &sources);
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

fn diagnostic_guard<F, T>(
    diagnostics: &mut Diagnostics,
    source_map: &mut SourceMap,
    f: F,
) -> ControlFlow<()>
where
    F: FnOnce(&mut Diagnostics, &mut SourceMap) -> T,
{
    let diagnostics_before = diagnostics.len();
    f(diagnostics, source_map);
    if diagnostics_before != diagnostics.len() {
        print_diagnostics(diagnostics, source_map);
        ControlFlow::Break(())
    } else {
        ControlFlow::Continue(())
    }
}

fn print_diagnostics(diagnostics: &Diagnostics, source_map: &SourceMap) {
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
