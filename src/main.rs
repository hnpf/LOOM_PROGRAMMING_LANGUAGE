mod lexer;
mod parser;
mod ast;
mod eval;
mod value;
mod env;
mod weave;
mod doc;

mod error;

use clap::{Parser as ClapParser, Subcommand};
use std::fs;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::eval::eval;
use crate::env::Env;
use crate::weave::weave;
use crate::doc::generate_docs;
use crate::value::Value;

#[derive(ClapParser)]
#[command(name = "loom")]
#[command(about = "Welcome to The Loom Programming Language!", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(help = "The .lm file to run")]
    file: Option<String>,

    #[arg(trailing_var_arg = true, help = "Arguments to pass to the script")]
    args: Vec<String>,

    #[arg(short, long, help = "Enable verbose logging")]
    verbose: bool,

    #[arg(long, help = "Enable verbose execution tracing of //? comments")]
    verbose_trace: bool,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        file: String,
        #[arg(short, long, help = "Enable verbose logging")]
        verbose: bool,
        #[arg(long, help = "Enable verbose execution tracing of //? comments")]
        verbose_trace: bool,
    },
    Check {
        file: String,
    },
    Doc {
        file: String,
    },
    Weave {
        file: String,
        #[arg(short, long, default_value = "main")]
        output: String,
    },
}

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()?;

    runtime.block_on(async_main())
}

async fn async_main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    
    let log_level = if cli.verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    let (command, file_to_run, verbose) = match (&cli.command, &cli.file) {
        (Some(Commands::Run { file, verbose, .. }), _) => (Some("run"), Some(file.clone()), *verbose || cli.verbose),
        (Some(Commands::Check { file }), _) => (Some("check"), Some(file.clone()), cli.verbose),
        (Some(Commands::Doc { file }), _) => {
            generate_docs(file)?;
            return Ok(());
        }
        (Some(Commands::Weave { file, output }), _) => {
            let source = fs::read_to_string(file)?;
            log::debug!("Weaving file: {} with length {}", file, source.len());
            let mut lexer = Lexer::new(&source);
            let tokens = match lexer.tokenize() {
                Ok(t) => t,
                Err(e) => {
                    log::error!("Lexer Error: {}", e);
                    return Ok(());
                }
            };
            let mut parser = Parser::new(tokens);
            let ast = match parser.parse() {
                Ok(a) => a,
                Err(e) => {
                    log::error!("Parser Error: {}", e);
                    return Ok(());
                }
            };
            log::info!("Starting weave for output: {}", output);
            weave(ast, output)?;
            log::info!("Weave complete.");
            return Ok(());
        }
        (None, Some(file)) => (Some("run"), Some(file.clone()), cli.verbose),
        _ => {
            // default help if nothing is provided
            use clap::CommandFactory;
            Cli::command().print_help()?;
            println!("");
            return Ok(());
        }
    };

    if let (Some(cmd), Some(file)) = (command, file_to_run) {
        let verbose_trace = match &cli.command {
            Some(Commands::Run { verbose_trace, .. }) => *verbose_trace || cli.verbose_trace,
            _ => cli.verbose_trace,
        };
        log::info!("Action: {}, File: {}", cmd, file);
        let source = match fs::read_to_string(&file) {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to read file {}: {}", file, e);
                return Ok(());
            }
        };
        log::debug!("File read successfully ({} bytes).", source.len());
        
        let mut lexer = Lexer::with_verbose(&source, verbose);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(e) => {
                log::error!("Lexer Error: {}", e);
                return Ok(());
            }
        };
        log::debug!("Tokenization complete ({} tokens).", tokens.len());
        
        let mut parser = Parser::new(tokens);
        let ast = match parser.parse() {
            Ok(a) => a,
            Err(e) => {
                log::error!("Parser Error: {}", e);
                return Ok(());
            }
        };
        log::debug!("Parsing complete.");

        match cmd {
            "run" => {
                let env = Env::with_verbose(verbose, verbose_trace);
                let loom_args: Vec<Value> = cli.args.into_iter().map(Value::Str).collect();
                env.set("args".to_string(), Value::List(std::sync::Arc::new(std::sync::RwLock::new(loom_args))));
                log::info!("Starting execution...");
                
                let env_clone = env.clone();
                let handle = tokio::spawn(async move {
                    eval(ast, &env_clone).await
                });
                
                match handle.await? {
                    Ok(val) => {
                        log::info!("Execution finished successfully.");
                        if val != Value::None {
                            log::debug!("Final value: {:?}", val);
                        }
                    }
                    Err(e) => {
                        log::error!("Runtime Error: {}", e);
                        
                        // Generate crash report
                        let mut report = String::new();
                        report.push_str("# 💥 Loom Crash Report\n\n");
                        report.push_str("## Error Details\n");
                        report.push_str(&format!("`{}`\n\n", e));
                        
                        report.push_str("## Last 10 State Changes\n");
                        if let Ok(history) = env.history.read() {
                            for (name, val) in history.iter().rev().take(10).rev() {
                                report.push_str(&format!("- `{}` = `{:?}`\n", name, val));
                            }
                        }
                        
                        report.push_str("\n## Environment Context\n");
                        report.push_str(&format!("- File: `{}`\n", file));
                        report.push_str(&format!("- Verbose: `{}`\n", verbose));
                        report.push_str(&format!("- Trace: `{}`\n", verbose_trace));

                        if let Err(write_err) = fs::write("crash_report.md", report) {
                            log::error!("Failed to write crash report: {}", write_err);
                        } else {
                            log::info!("Crash report generated: crash_report.md");
                        }
                    }
                }
            }
            "check" => {
                println!("Check successful for {}", file);
            }
            _ => unreachable!(),
        }
    }

    Ok(())
}
