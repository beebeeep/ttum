use ratatui::layout::{Constraint, Flex, Layout, Rect};

pub(crate) fn centered_rect(parent: Rect, horizontal_pct: u16, vertical_pct: u16) -> Rect {
    let hor = Layout::horizontal([Constraint::Percentage(horizontal_pct)]).flex(Flex::Center);
    let ver = Layout::vertical([Constraint::Percentage(vertical_pct)]).flex(Flex::Center);
    let [area] = ver.areas(parent);
    let [area] = hor.areas(area);
    area
}
