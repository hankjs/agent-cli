use std::io::{self, Write};

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{self, ClearType},
    ExecutableCommand,
};

fn main() {
    let mut stdout = io::stdout();
    terminal::enable_raw_mode().expect("failed to enable raw mode");

    stdout.execute(terminal::Clear(ClearType::All)).unwrap();
    stdout.execute(crossterm::cursor::MoveTo(0, 0)).unwrap();
    print_flush(&mut stdout, "\r\nhank-cli v0.1.0\r\n");
    print_flush(&mut stdout, "Type a message and press Enter. Ctrl+C to quit.\r\n\r\n");
    print_prompt(&mut stdout);

    let mut input = String::new();

    loop {
        if let Ok(Event::Key(key_event)) = event::read() {
            match (key_event.code, key_event.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                (KeyCode::Enter, _) => {
                    print_flush(&mut stdout, "\r\n");
                    handle_input(&mut stdout, input.trim());
                    input.clear();
                    print_prompt(&mut stdout);
                }
                (KeyCode::Backspace, _) => {
                    if !input.is_empty() {
                        input.pop();
                        print_flush(&mut stdout, "\x08 \x08");
                    }
                }
                (KeyCode::Char(c), _) => {
                    input.push(c);
                    print_flush(&mut stdout, &c.to_string());
                }
                _ => {}
            }
        }
    }

    terminal::disable_raw_mode().expect("failed to disable raw mode");
    print_flush(&mut stdout, "\r\nBye!\r\n");
}

fn handle_input(stdout: &mut io::Stdout, input: &str) {
    match input {
        "ping" => print_flush(stdout, "pong\r\n"),
        "pong" => print_flush(stdout, "ping\r\n"),
        "" => {}
        other => print_flush(stdout, &format!("unknown command: {other}\r\n")),
    }
}

fn print_prompt(stdout: &mut io::Stdout) {
    print_flush(stdout, "> ");
}

fn print_flush(stdout: &mut io::Stdout, s: &str) {
    write!(stdout, "{s}").unwrap();
    stdout.flush().unwrap();
}
