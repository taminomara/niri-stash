//! `niri-stash daemon [--no-focus-follows-mouse]` | `niri-stash stash|toggle|unstash`.

use niri_stash::{run_client, run_daemon, FocusPolicy, HotkeyRequest, EXIT_USAGE};

fn main() {
    // Default to info; `RUST_LOG` overrides. The journal adds timestamps, so none are formatted here.
    let mut logger = env_logger::Builder::new();
    logger.filter_level(log::LevelFilter::Info);
    logger.parse_default_env();
    logger.format_timestamp(None);
    logger.init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["daemon"] => run_daemon(FocusPolicy::AltTabOnly),
        // With focus-follows-mouse off in niri, every focus change is deliberate, so any focus
        // landing on a stashed window brings it back, not only the switcher's.
        ["daemon", "--no-focus-follows-mouse"] => run_daemon(FocusPolicy::AnyFocus),
        ["stash"] => run_client(HotkeyRequest::Stash),
        ["toggle"] => run_client(HotkeyRequest::Toggle),
        ["unstash"] => run_client(HotkeyRequest::Unstash),
        _ => {
            eprintln!("usage: niri-stash <daemon [--no-focus-follows-mouse]|stash|toggle|unstash>");
            EXIT_USAGE
        }
    };
    std::process::exit(code);
}
