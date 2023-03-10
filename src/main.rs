#![feature(box_syntax)]
#![feature(panic_info_message)]
#![feature(let_chains)]

use std::{env::args, path::PathBuf};

use crossterm::execute;

use self::{
    lexer::Lexer,
    runtime::{Runtime, StepResult, Verbosity},
};

mod error;
mod expr;
mod lexer;
mod matching;
mod parse;
mod repl;
mod rule;
mod runtime;
mod tests;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::panic::set_hook(Box::new(|info| {
        crossterm::terminal::disable_raw_mode().unwrap();
        println!("\nThe program has panicked. Please report this to https://github.com/willothy/noq/issues");
        if let Some(location) = info.location() {
            if let Some(payload) = info.message() {
                println!("Panicked with \"{}\" at {}", payload, location);
            } else {
                println!("Panicked with no message at {}", location);
            }
            #[cfg(debug_assertions)]
            println!("Backtrace:\n{}", std::backtrace::Backtrace::force_capture());
        }
    }));

    if let Some(file) = args().nth(1) {
        let path = PathBuf::from(file);
        let source = std::fs::read_to_string(&path).unwrap();
        let mut lexer = Lexer::new(source.chars().peekable()).with_file(
            path.file_name().unwrap().to_str().unwrap().to_string(),
            path,
        );

        let mut runtime = Runtime::new();
        runtime.verbosity = Verbosity::Normal;

        while !lexer.exhausted {
            match runtime.step(&mut lexer) {
                Ok(StepResult {
                    results: Some(output),
                    cmd_for_each: cmd_per,
                    clear,
                    ..
                }) => {
                    if clear {
                        execute!(
                            std::io::stdout(),
                            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
                        )
                        .unwrap();
                    }
                    for (idx, out) in output.iter().enumerate() {
                        for line in out {
                            if cmd_per {
                                println!(" => {}", line);
                            } else {
                                if idx == 0 {
                                    println!("=> {}", line);
                                } else {
                                    println!("{}", line);
                                }
                            }
                        }
                    }
                }
                Ok(StepResult { results: None, .. }) => (),
                Err(e) => {
                    println!(" !> {}", e);
                    break;
                }
            }
        }
    } else {
        repl::Repl::run();
    }
    Ok(())
}
