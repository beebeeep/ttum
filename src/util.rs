use ratatui::layout::{Constraint, Flex, Layout, Rect};

pub(crate) fn centered_rect(
    parent: Rect,
    horizontal_pct: u16,
    vertical_pct: u16,
    min_height: u16,
    min_width: u16,
) -> Rect {
    let height = ((parent.height as f32 * vertical_pct as f32 / 100.0) as u16).max(min_height);
    let width = ((parent.width as f32 * horizontal_pct as f32 / 100.0) as u16).max(min_width);
    let hor = Layout::horizontal([Constraint::Length(width)]).flex(Flex::Center);
    let ver = Layout::vertical([Constraint::Length(height)]).flex(Flex::Center);
    let [area] = ver.areas(parent);
    let [area] = hor.areas(area);
    area
}
