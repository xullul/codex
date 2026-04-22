use std::time::Duration;
use std::time::Instant;

use ratatui::style::Stylize;
use ratatui::text::Span;

const BUSY_INDICATOR_FRAMES: [&str; 4] = ["◜○◝", "◝○◞", "◞○◟", "◟○◜"];

pub(crate) const BUSY_INDICATOR_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) fn busy_indicator_preview_frame() -> &'static str {
    BUSY_INDICATOR_FRAMES[0]
}

pub(crate) fn busy_indicator_text_at(origin: Instant, now: Instant) -> &'static str {
    let elapsed = now.saturating_duration_since(origin);
    let frame_index = (elapsed.as_millis() / BUSY_INDICATOR_INTERVAL.as_millis()) as usize;
    BUSY_INDICATOR_FRAMES[frame_index % BUSY_INDICATOR_FRAMES.len()]
}

pub(crate) fn busy_indicator_span(
    origin: Instant,
    animations_enabled: bool,
    now: Instant,
) -> Span<'static> {
    let frame = if animations_enabled {
        busy_indicator_text_at(origin, now)
    } else {
        busy_indicator_preview_frame()
    };
    frame.bold()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn busy_indicator_cycles_frames_at_fixed_interval() {
        let origin = Instant::now();

        assert_eq!(
            busy_indicator_text_at(origin, origin),
            busy_indicator_preview_frame()
        );
        assert_eq!(
            busy_indicator_text_at(origin, origin + BUSY_INDICATOR_INTERVAL),
            "◝○◞"
        );
        assert_eq!(
            busy_indicator_text_at(origin, origin + BUSY_INDICATOR_INTERVAL * 4),
            busy_indicator_preview_frame()
        );
    }
}
