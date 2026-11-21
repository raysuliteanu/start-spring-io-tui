use std::process::ExitCode;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use clap::Parser;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event as TermEvent, KeyEventKind};
use start_spring_io_tui::api::{Client, DEFAULT_URL};
use start_spring_io_tui::app::{App, Mode};
use start_spring_io_tui::cache::Cache;
use start_spring_io_tui::worker::{self, Event, Job};
use start_spring_io_tui::{cli, ui};

fn main() -> ExitCode {
    let args = cli::Args::parse();
    let Some(cache_dir) = Cache::default_dir() else {
        eprintln!("error: cannot determine a cache directory for this platform");
        return ExitCode::FAILURE;
    };
    let out_dir = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("error: cannot read current directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (client, mode) = if args.offline {
        let mode = Mode::Offline {
            requested: true,
            cached_at: None,
        };
        (None, mode)
    } else {
        (Some(Client::new(DEFAULT_URL)), Mode::Connecting)
    };
    let (jobs, events) = worker::spawn(client, Cache::new(cache_dir), out_dir);
    let mut app = App::new(mode);
    // With no client the worker loads the cache, so forced offline uses the same path.
    send(&jobs, Some(Job::LoadMetadata));

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, &jobs, &events);
    ratatui::restore();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn send(jobs: &Sender<Job>, job: Option<Job>) {
    if let Some(job) = job {
        // The worker only stops when this sender is dropped, so a send can't fail while we run.
        let _ = jobs.send(job);
    }
}

fn run(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    jobs: &Sender<Job>,
    events: &Receiver<Event>,
) -> std::io::Result<()> {
    while !app.quit {
        terminal.draw(|frame| ui::draw(frame, app))?;
        if event::poll(Duration::from_millis(100))?
            && let TermEvent::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            send(jobs, app.on_key(key));
        }
        while let Ok(ev) = events.try_recv() {
            send(jobs, app.on_event(ev));
        }
    }
    Ok(())
}
