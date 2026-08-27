use std::path::PathBuf;

use anyhow::Result;
use log::LevelFilter;
use omp_harness::app::tui;

pub fn run() -> Result<()> {
    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .parse_default_env()
        .init();

    let initial_project_paths = std::env::args_os().skip(1).map(PathBuf::from).collect();
    tui::run(initial_project_paths)
}
