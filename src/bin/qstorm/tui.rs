use std::io::{Stdout, stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use qstorm::{BurstMetrics, runner::BenchmarkRunner};
use ratatui::prelude::*;
use tokio::sync::oneshot;

use crate::app::{App, AppState, View};
use crate::ui;

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

pub fn init() -> Result<Tui> {
    execute!(stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;
    Ok(terminal)
}

pub fn restore() -> Result<()> {
    execute!(stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}

pub async fn run(terminal: &mut Tui, mut app: App) -> Result<()> {
    // Initial connection (blocking is fine — TUI hasn't started yet)
    app.connect().await?;
    app.warmup().await?;

    let tick_rate = Duration::from_millis(100);
    let burst_interval = Duration::from_secs(1);
    let mut last_burst = std::time::Instant::now();

    // In-flight burst: runner is temporarily taken out of App
    let mut burst_rx: Option<
        oneshot::Receiver<(BenchmarkRunner, std::result::Result<BurstMetrics, qstorm::Error>)>,
    > = None;

    loop {
        terminal.draw(|frame| ui::render(frame, &app))?;

        // Poll for completed burst (non-blocking)
        if let Some(rx) = &mut burst_rx {
            match rx.try_recv() {
                Ok((runner, result)) => {
                    app.put_runner(runner);
                    burst_rx = None;
                    match result {
                        Ok(metrics) => {
                            app.history.push(metrics);
                            // Don't override Paused state
                            if app.state != AppState::Paused {
                                app.state = AppState::Idle;
                            }
                        }
                        Err(e) => {
                            tracing::error!("Burst failed: {}", e);
                            app.state = AppState::Error;
                        }
                    }
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => {
                    tracing::error!("Burst task dropped without completing");
                    app.state = AppState::Error;
                    burst_rx = None;
                }
            }
        }

        // Handle input with timeout
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if app.editing {
                        match key.code {
                            KeyCode::Enter => {
                                if app.has_runner() {
                                    let _ = app.submit_query().await;
                                }
                            }
                            KeyCode::Esc => {
                                app.cancel_editing();
                            }
                            KeyCode::Backspace => {
                                app.query_input.pop();
                            }
                            KeyCode::Char(c) => {
                                app.query_input.push(c);
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                // Wait for in-flight burst before disconnecting
                                if let Some(rx) = burst_rx.take() {
                                    if let Ok(Ok((runner, _))) =
                                        tokio::time::timeout(Duration::from_secs(2), rx).await
                                    {
                                        app.put_runner(runner);
                                    }
                                }
                                app.disconnect().await?;
                                return Ok(());
                            }
                            KeyCode::Char(' ') => {
                                app.toggle_pause();
                            }
                            KeyCode::Tab => {
                                app.toggle_view();
                                if app.view == View::Results
                                    && app.last_sample.is_none()
                                    && app.has_runner()
                                {
                                    let _ = app.run_sample().await;
                                }
                            }
                            KeyCode::Char('/') if app.view == View::Results => {
                                app.start_editing();
                            }
                            KeyCode::Char('r') if app.view == View::Results => {
                                if app.has_runner() {
                                    let _ = app.run_sample().await;
                                }
                            }
                            KeyCode::Up | KeyCode::Char('k') if app.view == View::Results => {
                                app.scroll_results(-1);
                            }
                            KeyCode::Down | KeyCode::Char('j') if app.view == View::Results => {
                                app.scroll_results(1);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Spawn burst in background if needed
        if burst_rx.is_none()
            && app.state != AppState::Paused
            && app.state != AppState::Error
            && app.has_runner()
            && last_burst.elapsed() >= burst_interval
        {
            if let Some(mut runner) = app.take_runner() {
                let (tx, rx) = oneshot::channel();
                app.state = AppState::Running;
                tokio::spawn(async move {
                    let result = runner.run_burst().await;
                    let _ = tx.send((runner, result));
                });
                burst_rx = Some(rx);
                last_burst = std::time::Instant::now();
            }
        }
    }
}
