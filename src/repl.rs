use std::{
    collections::HashMap,
    io::{stdin, stdout, Write},
};

use crossterm::{
    cursor::{self, MoveToColumn},
    event::{read, Event, KeyCode, KeyEvent},
    execute,
    style::Print,
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType},
};

use crate::{
    lexer::Lexer,
    rule::{Expr, Rule},
};

pub struct Repl {
    prompt: String,
    input_buf: String,
    lines: Vec<String>,
    history: Vec<String>,
    history_index: usize,
    state: ReplState,
    input_idx: usize,
}

struct ReplState {
    rules: HashMap<String, Rule>,
    shape: Option<Expr>,
    quit: bool,
}

type ReplResult = Result<Option<String>, String>;

impl Repl {
    pub fn run() {
        let mut repl = Repl {
            prompt: "> ".to_string(),
            input_buf: String::new(),
            input_idx: 0,
            lines: Vec::new(),
            history: Vec::new(),
            history_index: 0,
            state: ReplState {
                rules: HashMap::new(),
                shape: None,
                quit: false,
            },
        };
        repl.run_loop();
    }

    fn quit(&mut self, _: String) -> ReplResult {
        self.state.quit = true;
        Ok(None)
    }

    fn shape(&mut self, rest: String) -> ReplResult {
        if rest.trim().is_empty() {
            return Ok(if let Some(shape) = &self.state.shape {
                Some(format!("{}", shape))
            } else {
                Some(format!("No shape defined"))
            });
        }
        let shape = Expr::parse(Lexer::from(&*rest));
        self.state.shape = Some(shape.clone());
        Ok(Some(format!("{}", shape)))
    }

    fn rule(&mut self, rest: String) -> ReplResult {
        let Some(name) = rest.trim().split_whitespace().next() else {
            return Err("Expected rule name and rule".to_string());
        };
        let rule = Rule::parse(Lexer::from(&rest[name.len()..]));
        let res = format!("Added rule {}: {}", name, rule);
        self.state.rules.insert(name.to_owned(), rule);
        Ok(Some(res))
    }

    fn apply(&mut self, rest: String) -> ReplResult {
        let Some(rule_name) = rest.trim().split_whitespace().next() else {
            return Err("Expected rule name and rule".to_string());
        };
        let Some(rule) = self.state.rules.get(rule_name) else {
            return Err(format!("Unknown rule {}", rule_name));
        };
        let new_shape = rule.apply_all({
            let Some(shape) = &self.state.shape else {
                return Err(format!("No shape defined"));
            };
            shape
        })?;
        let new_shape_str = format!("{}", new_shape);
        self.state.shape = Some(new_shape);
        Ok(Some(new_shape_str))
    }

    fn run_loop(&mut self) {
        let mut stdout = stdout();
        enable_raw_mode().unwrap();
        execute!(stdout, Clear(ClearType::All)).unwrap();

        while !self.state.quit {
            // Use crossterm to print the latest <terminal height>
            let term_height = crossterm::terminal::size().unwrap().1;
            self.lines
                .iter()
                .rev()
                .take(term_height as usize - 3)
                .enumerate()
                .for_each(|(idx, line)| {
                    execute!(
                        stdout,
                        cursor::Hide,
                        cursor::MoveTo(0, term_height - 3 - idx as u16),
                        Clear(ClearType::CurrentLine),
                        Print(line)
                    )
                    .unwrap();
                });
            execute!(
                stdout,
                cursor::MoveTo(0, term_height - 1),
                Clear(ClearType::CurrentLine),
                Print(&(self.prompt.clone() + &self.input_buf)),
                MoveToColumn(self.prompt.len() as u16 + self.input_idx as u16),
                cursor::Show
            )
            .unwrap();

            match read().unwrap() {
                Event::Key(KeyEvent {
                    code: crossterm::event::KeyCode::Up,
                    modifiers: crossterm::event::KeyModifiers::NONE,
                    kind: crossterm::event::KeyEventKind::Press,
                    ..
                }) => {
                    if self.history_index < self.history.len() {
                        self.history_index += 1;
                        self.input_buf =
                            self.history[self.history.len() - self.history_index].clone();
                    }
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Down,
                    modifiers: crossterm::event::KeyModifiers::NONE,
                    kind: crossterm::event::KeyEventKind::Press,
                    ..
                }) => {
                    if self.history_index > 1 {
                        self.history_index -= 1;
                        self.input_buf =
                            self.history[self.history.len() - self.history_index].clone();
                    } else {
                        self.input_buf.clear();
                    }
                }
                Event::Key(KeyEvent {
                    code: code @ (KeyCode::Left | KeyCode::Right),
                    modifiers: crossterm::event::KeyModifiers::NONE,
                    kind: crossterm::event::KeyEventKind::Press,
                    ..
                }) => match code {
                    KeyCode::Left => {
                        self.input_idx
                            .checked_sub(1)
                            .map(|idx| self.input_idx = idx);
                    }
                    KeyCode::Right => {
                        if self.input_idx < self.input_buf.len() {
                            self.input_idx += 1;
                        }
                    }
                    _ => (),
                },
                Event::Key(KeyEvent {
                    code: crossterm::event::KeyCode::Char(c),
                    ..
                }) => {
                    if self.input_idx == self.input_buf.len() {
                        self.input_buf.push(c);
                    } else {
                        self.input_buf.insert(self.input_idx, c);
                    }
                    self.input_idx += 1;
                }
                Event::Key(KeyEvent {
                    code: crossterm::event::KeyCode::Backspace,
                    ..
                }) => {
                    if self.input_idx == self.input_buf.len() {
                        self.input_buf.pop();
                    } else {
                        self.input_buf
                            .remove(self.input_idx.checked_sub(1).unwrap_or(0));
                    }
                    self.input_idx
                        .checked_sub(1)
                        .map(|idx| self.input_idx = idx);
                }
                Event::Key(KeyEvent {
                    code: crossterm::event::KeyCode::Enter,
                    ..
                }) => {
                    self.history.push(self.input_buf.clone());
                    self.history_index = 0;
                    self.lines.push(self.prompt.clone() + &self.input_buf);

                    match self.handle_input() {
                        Ok(Some(output)) => self.lines.push(format!("  => {}", output)),
                        Ok(None) => (),
                        Err(err) => self.lines.push(format!("  => Error: {}", err)),
                    }
                    self.input_idx = 0;
                }
                _ => {}
            }
        }
        execute!(stdout, cursor::MoveDown(1), MoveToColumn(0), Print("")).unwrap();
        disable_raw_mode().unwrap();
    }

    fn handle_input(&mut self) -> ReplResult {
        let input = self.input_buf.drain(..).collect::<String>();

        let (cmd, args) = {
            let mut split = input.split_whitespace();
            let Some(cmd) = split.next() else {
                // No command given
                return Ok(None);
            };
            (cmd, split.collect::<Vec<&str>>().join(" "))
        };

        macro_rules! commands {
            (
                $(
                    $name:ident
                    $(
                        (
                            $($alias:ident),+
                        )
                    )?
                ),+
            ) => {
                match cmd {
                    $(
                        stringify!($name) $($(| stringify!($alias))+)? => self.$name(args),
                    )+
                    unknown => Err(format!("Unknown command {}. Expected one of {}.", unknown, stringify!($($name$(($($alias),+))?),+))),
                }
            }
        }

        commands!(shape, rule, apply, quit(q))
    }
}
