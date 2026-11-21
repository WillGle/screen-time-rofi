use anyhow::Result;

pub mod capture;
pub mod cli;
pub mod daemon;
pub mod db;
pub mod doctor;
pub mod idle;
pub mod migrations;
pub mod paths;
pub mod render;
pub mod sessions;
pub mod snapshot;
pub mod timeutil;
pub mod ui;
pub mod view;

pub fn run() -> Result<()> {
    cli::run()
}
