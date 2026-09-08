//! Reading one value without echoing it. A pipe is read whole; a terminal is
//! read with the echo bit off and put back afterwards.

use std::io::{BufRead, IsTerminal, Read, Write};

use crate::error::CliError;

/// True when something is feeding stdin, so there is nothing to prompt for.
pub fn piped() -> bool {
    !std::io::stdin().is_terminal()
}

/// The value for a key. It is never written to stdout, stderr or a log.
pub fn read_value(prompt: &str) -> Result<String, CliError> {
    let stdin = std::io::stdin();
    if piped() {
        let mut buffer = String::new();
        stdin
            .lock()
            .read_to_string(&mut buffer)
            .map_err(unreadable)?;
        return Ok(trimmed(&buffer));
    }

    let mut stderr = std::io::stderr();
    let _ = write!(stderr, "{prompt}");
    let _ = stderr.flush();

    let echo = Echo::off();
    let mut line = String::new();
    let read = stdin.lock().read_line(&mut line);
    drop(echo);
    let _ = writeln!(stderr);
    read.map_err(unreadable)?;
    Ok(trimmed(&line))
}

fn trimmed(value: &str) -> String {
    value.trim_end_matches(['\n', '\r']).to_string()
}

fn unreadable(e: std::io::Error) -> CliError {
    CliError::new(
        "unreadable_input",
        format!("the value could not be read: {e}."),
        "Pipe the value in, or type it at a terminal.",
    )
}

/// Echo off for as long as it is held.
struct Echo(Option<Saved>);

impl Echo {
    fn off() -> Echo {
        Echo(Saved::off())
    }
}

impl Drop for Echo {
    fn drop(&mut self) {
        if let Some(saved) = self.0.take() {
            saved.restore();
        }
    }
}

#[cfg(unix)]
mod platform {
    /// `c_lflag` is the fourth flag word of `termios` on every unix penv ships
    /// for; only the width of a flag differs, so the array carries both.
    #[cfg(target_os = "linux")]
    type Flag = u32;
    #[cfg(not(target_os = "linux"))]
    type Flag = u64;

    const LFLAG: usize = 3;
    const ECHO: Flag = 0o10;
    const TCSANOW: i32 = 0;
    const STDIN: i32 = 0;

    /// Wider than any real `termios`, so the C call fills a prefix of it.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Termios([Flag; 24]);

    unsafe extern "C" {
        fn tcgetattr(fd: i32, termios: *mut Termios) -> i32;
        fn tcsetattr(fd: i32, actions: i32, termios: *const Termios) -> i32;
    }

    pub struct Saved(Termios);

    impl Saved {
        pub fn off() -> Option<Saved> {
            let mut termios = Termios([0; 24]);
            if unsafe { tcgetattr(STDIN, &mut termios) } != 0 {
                return None;
            }
            let saved = termios;
            termios.0[LFLAG] &= !ECHO;
            if unsafe { tcsetattr(STDIN, TCSANOW, &termios) } != 0 {
                return None;
            }
            Some(Saved(saved))
        }

        pub fn restore(self) {
            unsafe { tcsetattr(STDIN, TCSANOW, &self.0) };
        }
    }
}

#[cfg(windows)]
mod platform {
    const STD_INPUT_HANDLE: u32 = -10i32 as u32;
    const ENABLE_ECHO_INPUT: u32 = 0x0004;

    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> isize;
        fn GetConsoleMode(handle: isize, mode: *mut u32) -> i32;
        fn SetConsoleMode(handle: isize, mode: u32) -> i32;
    }

    pub struct Saved {
        handle: isize,
        mode: u32,
    }

    impl Saved {
        pub fn off() -> Option<Saved> {
            let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
            let mut mode = 0u32;
            if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
                return None;
            }
            if unsafe { SetConsoleMode(handle, mode & !ENABLE_ECHO_INPUT) } == 0 {
                return None;
            }
            Some(Saved { handle, mode })
        }

        pub fn restore(self) {
            unsafe { SetConsoleMode(self.handle, self.mode) };
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    pub struct Saved;

    impl Saved {
        /// Nothing to turn off, so nothing is read at a terminal either.
        pub fn off() -> Option<Saved> {
            None
        }

        pub fn restore(self) {}
    }
}

use platform::Saved;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trailing_newline_is_not_part_of_the_value() {
        assert_eq!(trimmed("sk_test_FAKE\n"), "sk_test_FAKE");
        assert_eq!(trimmed("sk_test_FAKE\r\n"), "sk_test_FAKE");
        assert_eq!(trimmed("sk_test_FAKE"), "sk_test_FAKE");
        assert_eq!(trimmed(" spaced "), " spaced ", "only the newline goes");
    }

    #[test]
    fn turning_echo_off_and_back_on_is_safe_without_a_terminal() {
        let echo = Echo::off();
        drop(echo);
    }
}
