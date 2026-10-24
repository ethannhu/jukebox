use std::error::Error;

use qrcodegen::{QrCode, QrCodeEcc};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::app::{Model, Page};

pub fn draw(frame: &mut Frame, model: &Model) {
    let qr_lines = model
        .qr_url
        .as_deref()
        .map(render_qr)
        .transpose()
        .ok()
        .flatten();
    let qr_height = qr_lines
        .as_ref()
        .map(|rows| rows.len() as u16 + 3)
        .unwrap_or(0);
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(qr_height),
        Constraint::Min(5),
        Constraint::Length(2),
    ])
    .split(frame.area());

    frame.render_widget(
        Paragraph::new("Netease Cloud Music · TUI")
            .block(Block::default().borders(Borders::ALL).title("ncm-rs")),
        areas[0],
    );
    if let Some(rows) = qr_lines {
        frame.render_widget(
            Paragraph::new(
                rows.iter()
                    .map(|row| Line::from(row.as_str()))
                    .collect::<Vec<_>>(),
            )
            .block(Block::default().borders(Borders::ALL).title("扫码登录")),
            areas[1],
        );
    }

    if model.show_controller {
        draw_controller(frame, areas[2], model);
    } else {
        let (title, rows): (&str, Vec<String>) = match &model.page {
            Page::Playlists => (
                "我的歌单",
                model
                    .playlists
                    .iter()
                    .map(|playlist| playlist.label.clone())
                    .collect(),
            ),
            Page::PlaylistDetail { title, songs } => {
                (title, songs.iter().map(|track| track.label()).collect())
            }
        };
        let items = rows
            .iter()
            .map(|row| ListItem::new(row.clone()))
            .collect::<Vec<_>>();
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        let mut state = ListState::default();
        if !rows.is_empty() {
            state.select(Some(model.selected));
        }
        frame.render_stateful_widget(list, areas[2], &mut state);
    }
    frame.render_widget(Paragraph::new(model.status.as_str()), areas[3]);
}

fn draw_controller(frame: &mut Frame, area: ratatui::layout::Rect, model: &Model) {
    let sections =
        Layout::vertical([Constraint::Percentage(65), Constraint::Percentage(35)]).split(area);
    let items = if model.queue.is_empty() {
        vec![ListItem::new("（暂无播放列表：先打开一个歌单）")]
    } else {
        model
            .queue
            .iter()
            .map(|track| ListItem::new(track.label()))
            .collect::<Vec<_>>()
    };
    let queue_title = model
        .queue_title
        .as_deref()
        .map(|title| format!("播放列表 · {title}"))
        .unwrap_or_else(|| "播放列表".to_string());
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(queue_title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default();
    if !model.queue.is_empty() {
        state.select(Some(model.queue_selected));
    }
    frame.render_stateful_widget(list, sections[0], &mut state);

    let selected = model
        .queue
        .get(model.queue_selected)
        .map(|track| track.label())
        .unwrap_or_else(|| "无".into());
    let source = model.queue_title.as_deref().unwrap_or("无");
    let current = model
        .current_track
        .as_ref()
        .map(|track| track.label())
        .unwrap_or_else(|| "无".into());
    let state_label = if model.is_playing {
        "播放中"
    } else if model.current_track.is_some() {
        "已停止"
    } else {
        "未播放"
    };
    let status = Paragraph::new(vec![
        Line::from(format!("播放状态：{state_label}")),
        Line::from(format!("当前歌曲：{current}")),
        Line::from(format!("选中歌曲：{selected}")),
        Line::from(format!(
            "进度：{} / {}",
            format_time(model.playback_position),
            format_time(model.playback_duration)
        )),
        Line::from(format!("队列来源：{source} · 共 {} 首", model.queue.len())),
        Line::from("空格播放/暂停 · j/← 上一首 · k/→ 下一首"),
        Line::from("↑/↓ 浏览 · Enter 播放 · p 返回歌单页"),
    ])
    .block(Block::default().borders(Borders::ALL).title("播放状态"));
    frame.render_widget(status, sections[1]);
}

fn format_time(seconds: u64) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn render_qr(value: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let qr = QrCode::encode_text(value, QrCodeEcc::Low)?;
    let size = qr.size();
    let mut rows = Vec::new();
    // Pair adjacent QR rows into half-block characters to keep the code compact.
    for y in (-4..size + 4).step_by(2) {
        let mut row = String::new();
        for x in -4..size + 4 {
            let top = x >= 0 && y >= 0 && x < size && y < size && qr.get_module(x, y);
            let bottom_y = y + 1;
            let bottom = x >= 0
                && bottom_y >= 0
                && x < size
                && bottom_y < size
                && qr.get_module(x, bottom_y);
            row.push(match (top, bottom) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            });
        }
        rows.push(row);
    }
    Ok(rows)
}
