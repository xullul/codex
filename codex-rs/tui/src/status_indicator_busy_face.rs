use std::time::Duration;
use std::time::Instant;

use ratatui::style::Stylize;
use ratatui::text::Span;

const BUSY_FACE_FRAMES: [&str; 12] = [
    "[>_<]", "[>_<]", "[-_-]", "[o_o]", "[o_o]", "[^_^]", "[^v^]", "[^_^]", "[>_<]", "[._.]",
    "[o_o]", "[>_<]",
];

pub(crate) const BUSY_FACE_INTERVAL: Duration = Duration::from_millis(130);

pub(crate) fn busy_face_preview_frame() -> &'static str {
    "[>_]"
}

pub(crate) fn busy_face_text_at(origin: Instant, now: Instant) -> &'static str {
    let elapsed = now.saturating_duration_since(origin);
    let frame_index = (elapsed.as_millis() / BUSY_FACE_INTERVAL.as_millis()) as usize;
    BUSY_FACE_FRAMES[frame_index % BUSY_FACE_FRAMES.len()]
}

pub(crate) fn busy_face_span(
    origin: Instant,
    animations_enabled: bool,
    now: Instant,
) -> Span<'static> {
    let frame = if animations_enabled {
        busy_face_text_at(origin, now)
    } else {
        busy_face_preview_frame()
    };
    frame.magenta().bold()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn busy_face_preview_frame_is_static_codex_mark() {
        assert_eq!(busy_face_preview_frame(), "[>_]");
    }

    #[test]
    fn busy_face_cycles_frames_at_fixed_interval() {
        let origin = Instant::now();

        assert_eq!(busy_face_text_at(origin, origin), "[>_<]");
        assert_eq!(
            busy_face_text_at(origin, origin + BUSY_FACE_INTERVAL),
            "[>_<]"
        );
        assert_eq!(
            busy_face_text_at(origin, origin + BUSY_FACE_INTERVAL * 2),
            "[-_-]"
        );
        assert_eq!(
            busy_face_text_at(
                origin,
                origin + BUSY_FACE_INTERVAL * BUSY_FACE_FRAMES.len() as u32,
            ),
            "[>_<]"
        );
    }

    #[test]
    fn busy_face_frames_keep_brackets_and_width() {
        let expected_width = UnicodeWidthStr::width(BUSY_FACE_FRAMES[0]);

        for frame in BUSY_FACE_FRAMES {
            assert!(
                frame.starts_with('[') && frame.ends_with(']'),
                "expected bracketed face frame, got {frame}",
            );
            assert_eq!(UnicodeWidthStr::width(frame), expected_width);
        }
    }

    #[test]
    fn busy_face_span_uses_preview_when_animations_are_disabled() {
        let origin = Instant::now();
        let span = busy_face_span(
            origin,
            /*animations_enabled*/ false,
            origin + BUSY_FACE_INTERVAL * 3,
        );

        assert_eq!(span.content.as_ref(), busy_face_preview_frame());
    }
}
