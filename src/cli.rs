use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};

use crate::paths::Paths;
use crate::{daemon, doctor, render, ui, view};

#[derive(Debug, Parser)]
#[command(
    name = "screen-time",
    version,
    about = "Local, per-user screen-time tracker with a static Rofi day timeline",
    arg_required_else_help = true,
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run the per-user tracker daemon.
    Daemon(DaemonArgs),
    /// Render the main screen or one app detail.
    Render(RenderArgs),
    /// Check runtime, database, and snapshot health.
    Doctor,
}

#[derive(Debug, Args)]
pub struct DaemonArgs {}

#[derive(Debug, Args)]
pub struct RenderArgs {
    /// Output format. Only `rofi` is supported.
    #[arg(long, value_name = "FORMAT", default_value = "rofi")]
    pub format: String,

    /// View a specific local date (YYYY-MM-DD). Defaults to today.
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub date: Option<String>,

    /// Open one app detail by key. Defaults to the main day view.
    #[arg(long, value_name = "KEY")]
    pub app: Option<String>,

    /// Test hook: render as of this RFC 3339 timestamp.
    #[arg(long, hide = true, value_name = "RFC3339")]
    pub now: Option<String>,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Daemon(_) => daemon::run(),
        Command::Render(args) => run_render(args),
        Command::Doctor => doctor::run(),
    }
}

fn run_render(args: RenderArgs) -> Result<()> {
    if args.format != "rofi" {
        bail!(
            "unsupported format {:?}; only `rofi` is available",
            args.format
        );
    }
    if let Some(now) = &args.now {
        // SAFETY: the renderer is single-threaded here. The test hook is an
        // explicit override, never used by the daemon or the installed UI.
        unsafe { std::env::set_var("SCREEN_TIME_NOW", now) };
    }

    let now = crate::timeutil::now();
    let paths = Paths::resolve()?;

    if std::env::var_os("ROFI_RETV").is_some() {
        let output = ui::handle_rofi(&paths, &args, now)?;
        print!("{output}");
        return Ok(());
    }

    let requested_date = match &args.date {
        Some(date) => crate::timeutil::parse_date(date)
            .with_context(|| format!("invalid --date value {date:?}"))?,
        None => now.date_naive(),
    };
    let output = match &args.app {
        Some(app) => {
            let detail = view::load_detail(&paths, requested_date, app, now)?;
            render::render_detail(&detail, render::OutputMode::Plain)
        }
        None => {
            let day = view::load_main(&paths, requested_date, now)?;
            render::render_main(&day, render::OutputMode::Plain)
        }
    };
    print!("{output}");
    Ok(())
}
