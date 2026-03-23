use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph, Row, Table, Wrap,
    },
};

use crate::app::{App, AppState, View};

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Main content
            Constraint::Length(3), // Footer/status
        ])
        .split(frame.area());

    render_header(frame, chunks[0], app);

    match app.view {
        View::Dashboard => render_charts(frame, chunks[1], app),
        View::Results => render_results(frame, chunks[1], app),
    }

    render_footer(frame, chunks[2], app);
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let state_text = match app.state {
        AppState::Idle => "IDLE".to_string(),
        AppState::Connecting => "CONNECTING...".to_string(),
        AppState::Warming => "WARMING UP...".to_string(),
        AppState::Running => "RUNNING".to_string(),
        AppState::Paused => "PAUSED".to_string(),
        AppState::Error => "ERROR".to_string(),
    };

    let state_color = match app.state {
        AppState::Running => Color::Green,
        AppState::Paused => Color::Yellow,
        AppState::Error => Color::Red,
        _ => Color::White,
    };

    let view_label = match app.view {
        View::Dashboard => "Dashboard",
        View::Results => "Results",
    };

    let header = Paragraph::new(Line::from(vec![
        Span::raw("qstorm "),
        Span::styled(
            format!("[{}]", app.provider_name()),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw(format!(" ({} queries) - ", app.query_count())),
        Span::styled(state_text, Style::default().fg(state_color).bold()),
        Span::raw("  "),
        Span::styled(
            format!("[{}]", view_label),
            Style::default().fg(Color::Magenta),
        ),
    ]))
    .block(Block::default().borders(Borders::ALL));

    frame.render_widget(header, area);
}

fn render_charts(frame: &mut Frame, area: Rect, app: &App) {
    // 2x2 grid of charts
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let top_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);

    let bottom_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    render_qps_chart(frame, top_row[0], app);
    render_latency_chart(frame, top_row[1], app);
    render_p99_chart(frame, bottom_row[0], app);
    render_recall_chart(frame, bottom_row[1], app);
}

fn render_results(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Query bar
            Constraint::Min(0),    // Results table
        ])
        .split(area);

    // Query bar — input mode vs display mode
    if app.editing {
        let query_bar = Paragraph::new(Line::from(vec![
            Span::styled("/ ", Style::default().fg(Color::Yellow).bold()),
            Span::raw(&app.query_input),
            Span::styled("_", Style::default().fg(Color::Yellow).add_modifier(Modifier::SLOW_BLINK)),
        ]))
        .block(
            Block::default()
                .title(" Search ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        );
        frame.render_widget(query_bar, chunks[0]);
    } else if let Some(sample) = &app.last_sample {
        let hit_count = sample.results.results.len();
        let took = sample
            .results
            .took_ms
            .map(|t| format!(" in {}ms", t))
            .unwrap_or_default();

        let query_info = Paragraph::new(Line::from(vec![
            Span::styled("Query: ", Style::default().bold()),
            Span::styled(
                &sample.query,
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(format!("  ({} hits{})", hit_count, took)),
        ]))
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(query_info, chunks[0]);
    } else {
        let placeholder = Paragraph::new(
            Line::from(vec![
                Span::styled("Press ", Style::default().fg(Color::DarkGray)),
                Span::styled("[/]", Style::default().fg(Color::DarkGray).bold()),
                Span::styled(" to search", Style::default().fg(Color::DarkGray)),
            ]),
        )
        .block(
            Block::default()
                .title(" Search ")
                .borders(Borders::ALL),
        );
        frame.render_widget(placeholder, chunks[0]);
    }

    // Results table (or empty placeholder)
    let Some(sample) = &app.last_sample else {
        let placeholder = Paragraph::new("")
            .block(
                Block::default()
                    .title(" Results ")
                    .borders(Borders::ALL),
            );
        frame.render_widget(placeholder, chunks[1]);
        return;
    };

    // Results table
    let header = Row::new(vec![
        "#",
        "ID",
        "Score",
        "Payload",
    ])
    .style(Style::default().bold().fg(Color::Cyan))
    .bottom_margin(1);

    let rows: Vec<Row> = sample
        .results
        .results
        .iter()
        .enumerate()
        .map(|(i, result)| {
            let payload_str = result
                .payload
                .as_ref()
                .map(|p| {
                    let s = serde_json::to_string(p).unwrap_or_default();
                    // Truncate long payloads
                    if s.len() > 120 {
                        format!("{}...", &s[..117])
                    } else {
                        s
                    }
                })
                .unwrap_or_else(|| "-".to_string());

            let style = if i == app.results_scroll {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            Row::new(vec![
                format!("{}", i + 1),
                result.id.clone(),
                format!("{:.4}", result.score),
                payload_str,
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),      // #
            Constraint::Length(24),     // ID
            Constraint::Length(10),     // Score
            Constraint::Min(20),       // Payload (fills remaining)
        ],
    )
    .header(header)
    .block(
        Block::default()
            .title(" Results ")
            .borders(Borders::ALL),
    );

    frame.render_widget(table, chunks[1]);
}

fn render_qps_chart(frame: &mut Frame, area: Rect, app: &App) {
    let data = app.history.qps_series();
    let max_y = data
        .iter()
        .map(|(_, y)| *y)
        .fold(0.0_f64, f64::max)
        .max(1.0);

    // Render name with running average QPS
    let name = format!(
        "QPS (avg: {:.1})",
        app.history
            .qps_series()
            .into_iter()
            .reduce(|a, b| (0.0, a.1 + b.1))
            .map(|(_, sum)| sum / data.len() as f64)
            .unwrap_or(0.0)
    );

    let dataset = Dataset::default()
        .name(name)
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Cyan))
        .data(&data);

    let chart = Chart::new(vec![dataset])
        .block(
            Block::default()
                .title(" Queries/Second ")
                .borders(Borders::ALL),
        )
        .x_axis(
            Axis::default()
                .bounds([0.0, data.len().max(1) as f64])
                .labels::<Vec<Span>>(vec![]),
        )
        .y_axis(Axis::default().bounds([0.0, max_y * 1.1]).labels(vec![
            Span::raw("0"),
            Span::raw(format!("{:.0}", max_y / 2.0)),
            Span::raw(format!("{:.0}", max_y)),
        ]));

    frame.render_widget(chart, area);
}

fn render_latency_chart(frame: &mut Frame, area: Rect, app: &App) {
    let p50_data = app.history.p50_series();
    let max_y = p50_data
        .iter()
        .map(|(_, y)| *y)
        .fold(0.0_f64, f64::max)
        .max(1.0);

    // Render name with running average QPS
    let name = format!(
        "p50 (avg: {:.1})",
        app.history
            .p50_series()
            .into_iter()
            .reduce(|a, b| (0.0, a.1 + b.1))
            .map(|(_, sum)| sum / p50_data.len() as f64)
            .unwrap_or(0.0)
    );

    let dataset = Dataset::default()
        .name(name)
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Green))
        .data(&p50_data);

    let chart = Chart::new(vec![dataset])
        .block(
            Block::default()
                .title(" Latency p50 (ms) ")
                .borders(Borders::ALL),
        )
        .x_axis(
            Axis::default()
                .bounds([0.0, p50_data.len().max(1) as f64])
                .labels::<Vec<Span>>(vec![]),
        )
        .y_axis(Axis::default().bounds([0.0, max_y * 1.1]).labels(vec![
            Span::raw("0"),
            Span::raw(format!("{:.1}", max_y / 2.0)),
            Span::raw(format!("{:.1}", max_y)),
        ]));

    frame.render_widget(chart, area);
}

fn render_p99_chart(frame: &mut Frame, area: Rect, app: &App) {
    let p99_data = app.history.p99_series();
    let max_y = p99_data
        .iter()
        .map(|(_, y)| *y)
        .fold(0.0_f64, f64::max)
        .max(1.0);

    // Render name with running average QPS
    let name = format!(
        "p99 (avg: {:.1})",
        app.history
            .p99_series()
            .into_iter()
            .reduce(|a, b| (0.0, a.1 + b.1))
            .map(|(_, sum)| sum / p99_data.len() as f64)
            .unwrap_or(0.0)
    );


    let dataset = Dataset::default()
        .name(name)
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Red))
        .data(&p99_data);

    let chart = Chart::new(vec![dataset])
        .block(
            Block::default()
                .title(" Latency p99 (ms) ")
                .borders(Borders::ALL),
        )
        .x_axis(
            Axis::default()
                .bounds([0.0, p99_data.len().max(1) as f64])
                .labels::<Vec<Span>>(vec![]),
        )
        .y_axis(Axis::default().bounds([0.0, max_y * 1.1]).labels(vec![
            Span::raw("0"),
            Span::raw(format!("{:.1}", max_y / 2.0)),
            Span::raw(format!("{:.1}", max_y)),
        ]));

    frame.render_widget(chart, area);
}

fn render_recall_chart(frame: &mut Frame, area: Rect, app: &App) {
    let recall_data = app.history.recall_series();

    if recall_data.is_empty() {
        // Show placeholder when no recall data
        let placeholder = Paragraph::new("Recall@k\n(no ground truth)")
            .block(
                Block::default()
                    .title(" Recall@k (%) ")
                    .borders(Borders::ALL),
            )
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: true });
        frame.render_widget(placeholder, area);
        return;
    }

    let dataset = Dataset::default()
        .name("recall")
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Magenta))
        .data(&recall_data);

    let chart = Chart::new(vec![dataset])
        .block(
            Block::default()
                .title(" Recall@k (%) ")
                .borders(Borders::ALL),
        )
        .x_axis(
            Axis::default()
                .bounds([0.0, recall_data.len().max(1) as f64])
                .labels::<Vec<Span>>(vec![]),
        )
        .y_axis(Axis::default().bounds([0.0, 100.0]).labels(vec![
            Span::raw("0"),
            Span::raw("50"),
            Span::raw("100"),
        ]));

    frame.render_widget(chart, area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let content = match app.view {
        View::Dashboard => {
            let latest = app.history.latest();
            let stats = if let Some(m) = latest {
                format!(
                    "QPS: {:.1} | p50: {:.2}ms | p99: {:.2}ms | Success: {} | Failed: {}",
                    m.qps,
                    m.latency.p50_us as f64 / 1000.0,
                    m.latency.p99_us as f64 / 1000.0,
                    m.success_count,
                    m.failure_count,
                )
            } else {
                "Waiting for data...".to_string()
            };

            Line::from(vec![
                Span::raw(stats),
                Span::raw(" | "),
                Span::styled("[Space]", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" Pause "),
                Span::styled("[Tab]", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" Results "),
                Span::styled("[q]", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" Quit"),
            ])
        }
        View::Results if app.editing => Line::from(vec![
            Span::styled("[Enter]", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" Search "),
            Span::styled("[Esc]", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" Cancel"),
        ]),
        View::Results => Line::from(vec![
            Span::styled("[/]", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" Search "),
            Span::styled("[r]", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" Refresh "),
            Span::styled("[j/k]", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" Scroll "),
            Span::styled("[Tab]", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" Dashboard "),
            Span::styled("[q]", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" Quit"),
        ]),
    };

    let footer = Paragraph::new(content).block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, area);
}