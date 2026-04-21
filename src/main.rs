use std::path::PathBuf;

use anyhow::Result;
use log::LevelFilter;
use pi_harness::app::App;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .parse_default_env()
        .init();

    let initial_project_paths = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let event_loop = EventLoop::<()>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let mut app = App::new(proxy, initial_project_paths)?;
    event_loop.run_app(&mut app)?;
    Ok(())
}
