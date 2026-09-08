//! The one place the pure detector is wired to this process: the environment,
//! the terminal, the filesystem and the parent chain.

use penv_agent::{Context, Detection, detect};

use crate::ancestry::Processes;
use crate::env::Env;

pub fn detect_here(env: &Env, tty: bool) -> Detection {
    let exists = |path: &str| std::path::Path::new(path).exists();
    let processes = Processes;
    let mut cx = Context::new(env.as_map());
    cx.tty = tty;
    cx.git_editor_noninteractive = penv_agent::git_editor_noninteractive(env.as_map());
    cx.path_exists = &exists;
    cx.ancestry = &processes;
    detect(&cx)
}
