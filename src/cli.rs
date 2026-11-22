use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};

use crate::config;
use crate::paths::Paths;
use crate::{daemon, doctor, render, snapshot, ui, view};

const CONFIG_TEMPLATE: &str = include_str!("../share/screen-time/config.example.toml");

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
    Doctor(DoctorArgs),
    /// Write a consistent database snapshot and manifest.
    Snapshot(SnapshotArgs),
    /// Compare the live database with a pre-update snapshot fingerprint.
    CheckData(CheckDataArgs),
    /// Print the schema version supported by this binary.
    SchemaVersion,
    /// Checkpoint and truncate the database WAL.
    Checkpoint,
    /// Print or create the optional user configuration.
    Config(ConfigArgs),
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print the commented configuration template.
    Example,
    /// Print the configuration file path.
    Path,
    /// Write the template if the config does not exist yet.
    Init {
        /// Replace an existing config file.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Args)]
pub struct CheckDataArgs {
    /// Snapshot directory created by `screen-time snapshot` (or its manifest).
    #[arg(long, value_name = "DIR")]
    pub against: PathBuf,

    /// Emit a machine-readable JSON report.
    #[arg(long)]
    pub json: bool,

    /// Accept a documented schema migration or expected data change.
    #[arg(long)]
    pub allow_migration: bool,
}

#[derive(Debug, Args)]
pub struct DaemonArgs {}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Emit a stable machine-readable JSON report.
    #[arg(long)]
    pub json: bool,

    /// Skip Hyprland, Rofi, logind, and runtime-directory checks.
    #[arg(long)]
    pub no_runtime: bool,
}

#[derive(Debug, Args)]
pub struct SnapshotArgs {
    /// Destination directory. Must not already exist.
    #[arg(long, value_name = "DIR")]
    pub to: PathBuf,

    /// Print the snapshot manifest as JSON.
    #[arg(long)]
    pub json: bool,

    /// Do not record the config file hash in the manifest.
    #[arg(long)]
    pub no_config_hash: bool,
}

#[derive(Debug, Args)]
pub struct RenderArgs {
    /// Output format: `rofi` for the UI payload, `rofi-theme` for the resolved
    /// generated theme file.
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
        Command::Doctor(args) => doctor::run(&doctor::Options {
            json: args.json,
            no_runtime: args.no_runtime,
        }),
        Command::Snapshot(args) => run_snapshot(args),
        Command::CheckData(args) => run_check_data(args),
        Command::SchemaVersion => {
            println!("{}", crate::migrations::CURRENT_VERSION);
            Ok(())
        }
        Command::Checkpoint => {
            let paths = Paths::resolve()?;
            let db = crate::db::Database::open_snapshot_source(&paths.db_path)?;
            db.checkpoint()?;
            println!("checkpointed {}", paths.db_path.display());
            Ok(())
        }
        Command::Config(args) => run_config(args),
    }
}

fn run_config(args: ConfigArgs) -> Result<()> {
    match args.command {
        ConfigCommand::Example => {
            print!("{CONFIG_TEMPLATE}");
            Ok(())
        }
        ConfigCommand::Path => {
            println!("{}", config::default_config_path().display());
            Ok(())
        }
        ConfigCommand::Init { force } => {
            let path = config::default_config_path();
            if path.exists() && !force {
                bail!(
                    "config already exists: {} (use --force to replace it)",
                    path.display()
                );
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("cannot create {}", parent.display()))?;
            }
            let mut options = fs::OpenOptions::new();
            options.write(true).mode(0o600);
            if force {
                options.create(true).truncate(true);
            } else {
                options.create_new(true);
            }
            let mut file = options
                .open(&path)
                .with_context(|| format!("cannot create {}", path.display()))?;
            file.write_all(CONFIG_TEMPLATE.as_bytes())
                .with_context(|| format!("cannot write {}", path.display()))?;
            file.sync_all()
                .with_context(|| format!("cannot fsync {}", path.display()))?;
            println!("created {}", path.display());
            Ok(())
        }
    }
}

fn run_check_data(args: CheckDataArgs) -> Result<()> {
    let paths = Paths::resolve()?;
    let snapshot_dir = if args.against.is_dir() {
        args.against.clone()
    } else {
        args.against
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| args.against.clone())
    };
    let report = snapshot::check_database_against_snapshot(
        &paths.db_path,
        &snapshot_dir,
        args.allow_migration,
    )?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if report.ok {
        println!(
            "data check ok: schema {}, {} counts, {} session seconds, {} apps",
            report.actual_schema_version,
            report
                .actual_fingerprint
                .counts
                .get("sessions")
                .copied()
                .unwrap_or(0),
            report.actual_fingerprint.session_seconds,
            report.actual_fingerprint.app_keys.len()
        );
    } else {
        for difference in &report.differences {
            eprintln!("data check: {difference}");
        }
    }
    if !report.ok {
        anyhow::bail!(
            "data check failed ({} difference(s))",
            report.differences.len()
        );
    }
    Ok(())
}

fn run_snapshot(args: SnapshotArgs) -> Result<()> {
    let paths = Paths::resolve()?;
    let manifest =
        snapshot::create_database_snapshot(&paths.db_path, &args.to, !args.no_config_hash)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&manifest)?);
    } else {
        println!("snapshot: {}", args.to.display());
        println!("database: {}", manifest.snapshot_db);
        println!("sha256: {}", manifest.sha256);
        println!("quick_check: {}", manifest.quick_check);
        println!("schema_version: {}", manifest.schema_version);
        println!(
            "counts: {} events, {} sessions",
            manifest.counts.get("events").copied().unwrap_or(0),
            manifest.counts.get("sessions").copied().unwrap_or(0)
        );
        println!(
            "fingerprint: {} session seconds, app-key hash {}",
            manifest.fingerprint.session_seconds, manifest.fingerprint.app_key_hash
        );
    }
    Ok(())
}

fn run_render(args: RenderArgs) -> Result<()> {
    if args.format != "rofi" && args.format != "rofi-theme" {
        bail!(
            "unsupported format {:?}; use `rofi` or `rofi-theme`",
            args.format
        );
    }
    if args.format == "rofi-theme" {
        print!("{}", ui::launcher_theme()?.rofi_rasi());
        return Ok(());
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
        Some(date) => {
            let date = crate::timeutil::parse_date(date)
                .with_context(|| format!("invalid --date value {date:?}"))?;
            crate::timeutil::validate_past_or_today(date, now)?;
            date
        }
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
