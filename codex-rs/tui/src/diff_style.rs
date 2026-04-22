use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

use crate::color::is_light;
use crate::color::perceptual_distance;
use crate::diff_render::DiffLineType;
use crate::render::highlight::DiffScopeBackgroundRgbs;
use crate::render::highlight::diff_scope_background_rgbs;
use crate::terminal_palette::StdoutColorLevel;
use crate::terminal_palette::XTERM_COLORS;
use crate::terminal_palette::default_bg;
use crate::terminal_palette::indexed_color;
use crate::terminal_palette::rgb_color;
use crate::terminal_palette::stdout_color_level;
use codex_terminal_detection::TerminalName;
use codex_terminal_detection::terminal_info;

// Strong semantic diff backgrounds. These intentionally use custom colors even
// though most TUI styling avoids them: edit blocks need full-line color fills
// that remain obvious across syntax themes. Text uses the default foreground
// on these fills unless a syntax foreground passes a contrast check.
const DARK_TC_ADD_LINE_BG_RGB: (u8, u8, u8) = (6, 77, 42); // #064D2A
const DARK_TC_ADD_GUTTER_BG_RGB: (u8, u8, u8) = (8, 122, 61); // #087A3D
const DARK_TC_DEL_LINE_BG_RGB: (u8, u8, u8) = (103, 27, 27); // #671B1B
const DARK_TC_DEL_GUTTER_BG_RGB: (u8, u8, u8) = (138, 37, 37); // #8A2525
const LIGHT_TC_ADD_LINE_BG_RGB: (u8, u8, u8) = (184, 247, 199); // #B8F7C7
const LIGHT_TC_ADD_GUTTER_BG_RGB: (u8, u8, u8) = (120, 226, 146); // #78E292
const LIGHT_TC_DEL_LINE_BG_RGB: (u8, u8, u8) = (255, 208, 208); // #FFD0D0
const LIGHT_TC_DEL_GUTTER_BG_RGB: (u8, u8, u8) = (255, 150, 150); // #FF9696
const LIGHT_TC_GUTTER_FG_RGB: (u8, u8, u8) = (31, 35, 40); // #1F2328

const DARK_256_ADD_LINE_BG_IDX: u8 = 22;
const DARK_256_ADD_GUTTER_BG_IDX: u8 = 28;
const DARK_256_DEL_LINE_BG_IDX: u8 = 52;
const DARK_256_DEL_GUTTER_BG_IDX: u8 = 88;
const LIGHT_256_ADD_LINE_BG_IDX: u8 = 120;
const LIGHT_256_ADD_GUTTER_BG_IDX: u8 = 114;
const LIGHT_256_DEL_LINE_BG_IDX: u8 = 217;
const LIGHT_256_DEL_GUTTER_BG_IDX: u8 = 210;
const LIGHT_256_GUTTER_FG_IDX: u8 = 236;

const MIN_DIFF_SYNTAX_CONTRAST_RATIO: f32 = 4.5;

/// Controls which color palette the diff renderer uses for backgrounds and
/// gutter styling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiffTheme {
    Dark,
    Light,
}

/// Palette depth the diff renderer will target.
///
/// This is the renderer's own notion of color depth, derived from the raw
/// [`StdoutColorLevel`] and adjusted for terminals that under-report color
/// support.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DiffColorLevel {
    TrueColor,
    Ansi256,
    Ansi16,
}

/// Subset of [`DiffColorLevel`] that supports tinted backgrounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RichDiffColorLevel {
    TrueColor,
    Ansi256,
}

impl RichDiffColorLevel {
    fn from_diff_color_level(level: DiffColorLevel) -> Option<Self> {
        match level {
            DiffColorLevel::TrueColor => Some(Self::TrueColor),
            DiffColorLevel::Ansi256 => Some(Self::Ansi256),
            DiffColorLevel::Ansi16 => None,
        }
    }
}

/// Pre-resolved background colors for insert and delete diff lines.
///
/// Line backgrounds fill the whole row. Gutter backgrounds fill the line-number
/// column for add/delete rows when the terminal has enough color depth.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResolvedDiffBackgrounds {
    add: Option<Color>,
    del: Option<Color>,
    add_gutter: Option<Color>,
    del_gutter: Option<Color>,
}

/// Precomputed render state for diff line styling.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DiffRenderStyleContext {
    pub(crate) theme: DiffTheme,
    pub(crate) color_level: DiffColorLevel,
    pub(crate) diff_backgrounds: ResolvedDiffBackgrounds,
}

/// Snapshot the current terminal environment into a reusable style context.
pub(crate) fn current_diff_render_style_context() -> DiffRenderStyleContext {
    let theme = diff_theme();
    let color_level = diff_color_level();
    let diff_backgrounds = resolve_diff_backgrounds(theme, color_level);
    DiffRenderStyleContext {
        theme,
        color_level,
        diff_backgrounds,
    }
}

fn resolve_diff_backgrounds(
    theme: DiffTheme,
    color_level: DiffColorLevel,
) -> ResolvedDiffBackgrounds {
    resolve_diff_backgrounds_for(theme, color_level, diff_scope_background_rgbs())
}

/// Core background-resolution logic, kept pure for testability.
///
/// Starts from the semantic palette. Theme-provided diff scope backgrounds are
/// accepted only when they are at least as visually strong as the semantic
/// baseline for the current light/dark mode; weak theme colors fall back to the
/// semantic palette so edit blocks remain prominent.
pub(crate) fn resolve_diff_backgrounds_for(
    theme: DiffTheme,
    color_level: DiffColorLevel,
    scope_backgrounds: DiffScopeBackgroundRgbs,
) -> ResolvedDiffBackgrounds {
    let Some(level) = RichDiffColorLevel::from_diff_color_level(color_level) else {
        return ResolvedDiffBackgrounds::default();
    };

    let add_line = resolve_line_color(
        theme,
        level,
        semantic_add_line_bg_rgb(theme),
        semantic_add_line_bg_ansi256(theme),
        scope_backgrounds.inserted,
    );
    let del_line = resolve_line_color(
        theme,
        level,
        semantic_del_line_bg_rgb(theme),
        semantic_del_line_bg_ansi256(theme),
        scope_backgrounds.deleted,
    );

    ResolvedDiffBackgrounds {
        add: Some(add_line),
        del: Some(del_line),
        add_gutter: Some(add_gutter_bg(theme, level)),
        del_gutter: Some(del_gutter_bg(theme, level)),
    }
}

#[cfg(test)]
pub(crate) fn fallback_diff_backgrounds(
    theme: DiffTheme,
    color_level: DiffColorLevel,
) -> ResolvedDiffBackgrounds {
    resolve_diff_backgrounds_for(theme, color_level, DiffScopeBackgroundRgbs::default())
}

fn resolve_line_color(
    theme: DiffTheme,
    color_level: RichDiffColorLevel,
    semantic_rgb: (u8, u8, u8),
    semantic_ansi256: u8,
    scope_rgb: Option<(u8, u8, u8)>,
) -> Color {
    match scope_rgb {
        Some(rgb) if is_scope_background_strong_enough(theme, semantic_rgb, rgb) => {
            color_from_rgb_for_level(rgb, color_level)
        }
        _ => match color_level {
            RichDiffColorLevel::TrueColor => rgb_color(semantic_rgb),
            RichDiffColorLevel::Ansi256 => indexed_color(semantic_ansi256),
        },
    }
}

fn is_scope_background_strong_enough(
    theme: DiffTheme,
    semantic_rgb: (u8, u8, u8),
    scope_rgb: (u8, u8, u8),
) -> bool {
    if is_light(scope_rgb) != matches!(theme, DiffTheme::Light) {
        return false;
    }

    let anchor = match theme {
        DiffTheme::Dark => (0, 0, 0),
        DiffTheme::Light => (255, 255, 255),
    };
    perceptual_distance(scope_rgb, anchor) >= perceptual_distance(semantic_rgb, anchor)
}

fn color_from_rgb_for_level(rgb: (u8, u8, u8), color_level: RichDiffColorLevel) -> Color {
    match color_level {
        RichDiffColorLevel::TrueColor => rgb_color(rgb),
        RichDiffColorLevel::Ansi256 => quantize_rgb_to_ansi256(rgb),
    }
}

fn quantize_rgb_to_ansi256(target: (u8, u8, u8)) -> Color {
    let best_index = XTERM_COLORS
        .iter()
        .enumerate()
        .skip(16)
        .min_by(|(_, a), (_, b)| {
            perceptual_distance(**a, target).total_cmp(&perceptual_distance(**b, target))
        })
        .map(|(index, _)| index as u8);
    match best_index {
        Some(index) => indexed_color(index),
        None => indexed_color(DARK_256_ADD_LINE_BG_IDX),
    }
}

/// Testable helper: picks [`DiffTheme`] from an explicit background sample.
pub(crate) fn diff_theme_for_bg(bg: Option<(u8, u8, u8)>) -> DiffTheme {
    if let Some(rgb) = bg
        && is_light(rgb)
    {
        return DiffTheme::Light;
    }
    DiffTheme::Dark
}

fn diff_theme() -> DiffTheme {
    diff_theme_for_bg(default_bg())
}

fn diff_color_level() -> DiffColorLevel {
    diff_color_level_for_terminal(
        stdout_color_level(),
        terminal_info().name,
        std::env::var_os("WT_SESSION").is_some(),
        has_force_color_override(),
    )
}

fn has_force_color_override() -> bool {
    std::env::var_os("FORCE_COLOR").is_some()
}

/// Map a raw [`StdoutColorLevel`] to a [`DiffColorLevel`] using
/// Windows Terminal-specific truecolor promotion rules.
pub(crate) fn diff_color_level_for_terminal(
    stdout_level: StdoutColorLevel,
    terminal_name: TerminalName,
    has_wt_session: bool,
    has_force_color_override: bool,
) -> DiffColorLevel {
    if has_wt_session && !has_force_color_override {
        return DiffColorLevel::TrueColor;
    }

    let base = match stdout_level {
        StdoutColorLevel::TrueColor => DiffColorLevel::TrueColor,
        StdoutColorLevel::Ansi256 => DiffColorLevel::Ansi256,
        StdoutColorLevel::Ansi16 | StdoutColorLevel::Unknown => DiffColorLevel::Ansi16,
    };

    if stdout_level == StdoutColorLevel::Ansi16
        && terminal_name == TerminalName::WindowsTerminal
        && !has_force_color_override
    {
        DiffColorLevel::TrueColor
    } else {
        base
    }
}

pub(crate) fn style_line_bg_for(
    kind: DiffLineType,
    diff_backgrounds: ResolvedDiffBackgrounds,
) -> Style {
    match kind {
        DiffLineType::Insert => diff_backgrounds
            .add
            .map_or_else(Style::default, |bg| Style::default().bg(bg)),
        DiffLineType::Delete => diff_backgrounds
            .del
            .map_or_else(Style::default, |bg| Style::default().bg(bg)),
        DiffLineType::Context => Style::default(),
    }
}

pub(crate) fn style_context() -> Style {
    Style::default()
}

pub(crate) fn style_gutter_for(
    kind: DiffLineType,
    theme: DiffTheme,
    color_level: DiffColorLevel,
    diff_backgrounds: ResolvedDiffBackgrounds,
) -> Style {
    match (kind, theme, color_level) {
        (DiffLineType::Insert, DiffTheme::Light, DiffColorLevel::Ansi16)
        | (DiffLineType::Delete, DiffTheme::Light, DiffColorLevel::Ansi16) => {
            Style::default().fg(light_gutter_fg(color_level))
        }
        (DiffLineType::Insert, DiffTheme::Light, _) => diff_backgrounds
            .add_gutter
            .map_or_else(Style::default, |bg| {
                Style::default().fg(light_gutter_fg(color_level)).bg(bg)
            }),
        (DiffLineType::Delete, DiffTheme::Light, _) => diff_backgrounds
            .del_gutter
            .map_or_else(Style::default, |bg| {
                Style::default().fg(light_gutter_fg(color_level)).bg(bg)
            }),
        (DiffLineType::Insert, DiffTheme::Dark, _) => diff_backgrounds
            .add_gutter
            .map_or_else(style_gutter_dim, |bg| Style::default().bg(bg)),
        (DiffLineType::Delete, DiffTheme::Dark, _) => diff_backgrounds
            .del_gutter
            .map_or_else(style_gutter_dim, |bg| Style::default().bg(bg)),
        (DiffLineType::Context, _, _) => style_gutter_dim(),
    }
}

pub(crate) fn style_sign_add(diff_backgrounds: ResolvedDiffBackgrounds) -> Style {
    match diff_backgrounds.add {
        Some(_) => Style::default(),
        None => Style::default().fg(Color::Green),
    }
    .add_modifier(Modifier::BOLD)
}

pub(crate) fn style_sign_del(diff_backgrounds: ResolvedDiffBackgrounds) -> Style {
    match diff_backgrounds.del {
        Some(_) => Style::default(),
        None => Style::default().fg(Color::Red),
    }
    .add_modifier(Modifier::BOLD)
}

pub(crate) fn style_add(
    theme: DiffTheme,
    color_level: DiffColorLevel,
    diff_backgrounds: ResolvedDiffBackgrounds,
) -> Style {
    match (theme, color_level, diff_backgrounds.add) {
        (_, DiffColorLevel::Ansi16, _) => Style::default().fg(Color::Green),
        (DiffTheme::Light, DiffColorLevel::TrueColor, Some(bg))
        | (DiffTheme::Light, DiffColorLevel::Ansi256, Some(bg))
        | (DiffTheme::Dark, DiffColorLevel::TrueColor, Some(bg))
        | (DiffTheme::Dark, DiffColorLevel::Ansi256, Some(bg)) => Style::default().bg(bg),
        (DiffTheme::Light, DiffColorLevel::TrueColor, None)
        | (DiffTheme::Light, DiffColorLevel::Ansi256, None) => Style::default(),
        (DiffTheme::Dark, DiffColorLevel::TrueColor, None)
        | (DiffTheme::Dark, DiffColorLevel::Ansi256, None) => Style::default().fg(Color::Green),
    }
}

pub(crate) fn style_del(
    theme: DiffTheme,
    color_level: DiffColorLevel,
    diff_backgrounds: ResolvedDiffBackgrounds,
) -> Style {
    match (theme, color_level, diff_backgrounds.del) {
        (_, DiffColorLevel::Ansi16, _) => Style::default().fg(Color::Red),
        (DiffTheme::Light, DiffColorLevel::TrueColor, Some(bg))
        | (DiffTheme::Light, DiffColorLevel::Ansi256, Some(bg))
        | (DiffTheme::Dark, DiffColorLevel::TrueColor, Some(bg))
        | (DiffTheme::Dark, DiffColorLevel::Ansi256, Some(bg)) => Style::default().bg(bg),
        (DiffTheme::Light, DiffColorLevel::TrueColor, None)
        | (DiffTheme::Light, DiffColorLevel::Ansi256, None) => Style::default(),
        (DiffTheme::Dark, DiffColorLevel::TrueColor, None)
        | (DiffTheme::Dark, DiffColorLevel::Ansi256, None) => Style::default().fg(Color::Red),
    }
}

pub(crate) fn style_syntax_for_diff(
    kind: DiffLineType,
    syntax_style: Style,
    line_style: Style,
) -> Style {
    let mut style = syntax_style;
    if let Some(bg) = line_style.bg {
        style.bg = Some(bg);
        style.fg = diff_syntax_foreground(style.fg, bg);
    } else if matches!(kind, DiffLineType::Delete) {
        style.add_modifier |= Modifier::DIM;
    }
    style
}

fn diff_syntax_foreground(fg: Option<Color>, bg: Color) -> Option<Color> {
    let fg = fg?;
    let fg_rgb = fixed_rgb_for_color(fg)?;
    let bg_rgb = fixed_rgb_for_color(bg)?;

    (contrast_ratio(fg_rgb, bg_rgb) >= MIN_DIFF_SYNTAX_CONTRAST_RATIO).then_some(fg)
}

fn fixed_rgb_for_color(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(index) if index >= 16 => Some(XTERM_COLORS[index as usize]),
        Color::Indexed(_)
        | Color::Reset
        | Color::Black
        | Color::Red
        | Color::Green
        | Color::Yellow
        | Color::Blue
        | Color::Magenta
        | Color::Cyan
        | Color::Gray
        | Color::DarkGray
        | Color::LightRed
        | Color::LightGreen
        | Color::LightYellow
        | Color::LightBlue
        | Color::LightMagenta
        | Color::LightCyan
        | Color::White => None,
    }
}

fn contrast_ratio(a: (u8, u8, u8), b: (u8, u8, u8)) -> f32 {
    let lighter = relative_luminance(a).max(relative_luminance(b));
    let darker = relative_luminance(a).min(relative_luminance(b));
    (lighter + 0.05) / (darker + 0.05)
}

fn relative_luminance((r, g, b): (u8, u8, u8)) -> f32 {
    fn channel(c: u8) -> f32 {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

fn semantic_add_line_bg_rgb(theme: DiffTheme) -> (u8, u8, u8) {
    match theme {
        DiffTheme::Dark => DARK_TC_ADD_LINE_BG_RGB,
        DiffTheme::Light => LIGHT_TC_ADD_LINE_BG_RGB,
    }
}

fn semantic_del_line_bg_rgb(theme: DiffTheme) -> (u8, u8, u8) {
    match theme {
        DiffTheme::Dark => DARK_TC_DEL_LINE_BG_RGB,
        DiffTheme::Light => LIGHT_TC_DEL_LINE_BG_RGB,
    }
}

fn semantic_add_line_bg_ansi256(theme: DiffTheme) -> u8 {
    match theme {
        DiffTheme::Dark => DARK_256_ADD_LINE_BG_IDX,
        DiffTheme::Light => LIGHT_256_ADD_LINE_BG_IDX,
    }
}

fn semantic_del_line_bg_ansi256(theme: DiffTheme) -> u8 {
    match theme {
        DiffTheme::Dark => DARK_256_DEL_LINE_BG_IDX,
        DiffTheme::Light => LIGHT_256_DEL_LINE_BG_IDX,
    }
}

fn add_gutter_bg(theme: DiffTheme, color_level: RichDiffColorLevel) -> Color {
    match (theme, color_level) {
        (DiffTheme::Dark, RichDiffColorLevel::TrueColor) => rgb_color(DARK_TC_ADD_GUTTER_BG_RGB),
        (DiffTheme::Dark, RichDiffColorLevel::Ansi256) => indexed_color(DARK_256_ADD_GUTTER_BG_IDX),
        (DiffTheme::Light, RichDiffColorLevel::TrueColor) => rgb_color(LIGHT_TC_ADD_GUTTER_BG_RGB),
        (DiffTheme::Light, RichDiffColorLevel::Ansi256) => {
            indexed_color(LIGHT_256_ADD_GUTTER_BG_IDX)
        }
    }
}

fn del_gutter_bg(theme: DiffTheme, color_level: RichDiffColorLevel) -> Color {
    match (theme, color_level) {
        (DiffTheme::Dark, RichDiffColorLevel::TrueColor) => rgb_color(DARK_TC_DEL_GUTTER_BG_RGB),
        (DiffTheme::Dark, RichDiffColorLevel::Ansi256) => indexed_color(DARK_256_DEL_GUTTER_BG_IDX),
        (DiffTheme::Light, RichDiffColorLevel::TrueColor) => rgb_color(LIGHT_TC_DEL_GUTTER_BG_RGB),
        (DiffTheme::Light, RichDiffColorLevel::Ansi256) => {
            indexed_color(LIGHT_256_DEL_GUTTER_BG_IDX)
        }
    }
}

fn light_gutter_fg(color_level: DiffColorLevel) -> Color {
    match color_level {
        DiffColorLevel::TrueColor => rgb_color(LIGHT_TC_GUTTER_FG_RGB),
        DiffColorLevel::Ansi256 => indexed_color(LIGHT_256_GUTTER_FG_IDX),
        DiffColorLevel::Ansi16 => Color::Black,
    }
}

fn style_gutter_dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;

    fn background_style_snapshot(
        theme: DiffTheme,
        color_level: DiffColorLevel,
        scope_backgrounds: DiffScopeBackgroundRgbs,
    ) -> String {
        let backgrounds = resolve_diff_backgrounds_for(theme, color_level, scope_backgrounds);
        let add_line = style_line_bg_for(DiffLineType::Insert, backgrounds);
        let del_line = style_line_bg_for(DiffLineType::Delete, backgrounds);
        let add_gutter = style_gutter_for(DiffLineType::Insert, theme, color_level, backgrounds);
        let del_gutter = style_gutter_for(DiffLineType::Delete, theme, color_level, backgrounds);
        let add_sign = style_sign_add(backgrounds);
        let del_sign = style_sign_del(backgrounds);

        format!(
            "add_line={add_line:?}\nadd_gutter={add_gutter:?}\nadd_sign={add_sign:?}\ndel_line={del_line:?}\ndel_gutter={del_gutter:?}\ndel_sign={del_sign:?}"
        )
    }

    #[test]
    fn truecolor_dark_theme_uses_strong_semantic_backgrounds() {
        let backgrounds = fallback_diff_backgrounds(DiffTheme::Dark, DiffColorLevel::TrueColor);
        assert_eq!(
            style_line_bg_for(DiffLineType::Insert, backgrounds),
            Style::default().bg(rgb_color(DARK_TC_ADD_LINE_BG_RGB))
        );
        assert_eq!(
            style_line_bg_for(DiffLineType::Delete, backgrounds),
            Style::default().bg(rgb_color(DARK_TC_DEL_LINE_BG_RGB))
        );
        assert_eq!(
            style_gutter_for(
                DiffLineType::Insert,
                DiffTheme::Dark,
                DiffColorLevel::TrueColor,
                backgrounds
            ),
            Style::default().bg(rgb_color(DARK_TC_ADD_GUTTER_BG_RGB))
        );
        assert_eq!(
            style_gutter_for(
                DiffLineType::Delete,
                DiffTheme::Dark,
                DiffColorLevel::TrueColor,
                backgrounds
            ),
            Style::default().bg(rgb_color(DARK_TC_DEL_GUTTER_BG_RGB))
        );
    }

    #[test]
    fn truecolor_light_theme_uses_readable_semantic_backgrounds() {
        let backgrounds = fallback_diff_backgrounds(DiffTheme::Light, DiffColorLevel::TrueColor);
        assert_eq!(
            style_line_bg_for(DiffLineType::Insert, backgrounds),
            Style::default().bg(rgb_color(LIGHT_TC_ADD_LINE_BG_RGB))
        );
        assert_eq!(
            style_line_bg_for(DiffLineType::Delete, backgrounds),
            Style::default().bg(rgb_color(LIGHT_TC_DEL_LINE_BG_RGB))
        );
        assert_eq!(
            style_gutter_for(
                DiffLineType::Insert,
                DiffTheme::Light,
                DiffColorLevel::TrueColor,
                backgrounds
            ),
            Style::default()
                .fg(rgb_color(LIGHT_TC_GUTTER_FG_RGB))
                .bg(rgb_color(LIGHT_TC_ADD_GUTTER_BG_RGB))
        );
        assert_eq!(
            style_gutter_for(
                DiffLineType::Delete,
                DiffTheme::Light,
                DiffColorLevel::TrueColor,
                backgrounds
            ),
            Style::default()
                .fg(rgb_color(LIGHT_TC_GUTTER_FG_RGB))
                .bg(rgb_color(LIGHT_TC_DEL_GUTTER_BG_RGB))
        );
    }

    #[test]
    fn ansi256_uses_selected_indexed_backgrounds() {
        let dark = fallback_diff_backgrounds(DiffTheme::Dark, DiffColorLevel::Ansi256);
        assert_eq!(
            style_line_bg_for(DiffLineType::Insert, dark),
            Style::default().bg(indexed_color(DARK_256_ADD_LINE_BG_IDX))
        );
        assert_eq!(
            style_gutter_for(
                DiffLineType::Insert,
                DiffTheme::Dark,
                DiffColorLevel::Ansi256,
                dark
            ),
            Style::default().bg(indexed_color(DARK_256_ADD_GUTTER_BG_IDX))
        );
        assert_eq!(
            style_line_bg_for(DiffLineType::Delete, dark),
            Style::default().bg(indexed_color(DARK_256_DEL_LINE_BG_IDX))
        );
        assert_eq!(
            style_gutter_for(
                DiffLineType::Delete,
                DiffTheme::Dark,
                DiffColorLevel::Ansi256,
                dark
            ),
            Style::default().bg(indexed_color(DARK_256_DEL_GUTTER_BG_IDX))
        );

        let light = fallback_diff_backgrounds(DiffTheme::Light, DiffColorLevel::Ansi256);
        assert_eq!(
            style_line_bg_for(DiffLineType::Insert, light),
            Style::default().bg(indexed_color(LIGHT_256_ADD_LINE_BG_IDX))
        );
        assert_eq!(
            style_gutter_for(
                DiffLineType::Insert,
                DiffTheme::Light,
                DiffColorLevel::Ansi256,
                light
            ),
            Style::default()
                .fg(indexed_color(LIGHT_256_GUTTER_FG_IDX))
                .bg(indexed_color(LIGHT_256_ADD_GUTTER_BG_IDX))
        );
        assert_eq!(
            style_line_bg_for(DiffLineType::Delete, light),
            Style::default().bg(indexed_color(LIGHT_256_DEL_LINE_BG_IDX))
        );
        assert_eq!(
            style_gutter_for(
                DiffLineType::Delete,
                DiffTheme::Light,
                DiffColorLevel::Ansi256,
                light
            ),
            Style::default()
                .fg(indexed_color(LIGHT_256_GUTTER_FG_IDX))
                .bg(indexed_color(LIGHT_256_DEL_GUTTER_BG_IDX))
        );
    }

    #[test]
    fn ansi16_uses_foreground_only_diff_styles() {
        let backgrounds = fallback_diff_backgrounds(DiffTheme::Dark, DiffColorLevel::Ansi16);
        assert_eq!(
            style_line_bg_for(DiffLineType::Insert, backgrounds),
            Style::default()
        );
        assert_eq!(
            style_line_bg_for(DiffLineType::Delete, backgrounds),
            Style::default()
        );
        assert_eq!(
            style_add(DiffTheme::Dark, DiffColorLevel::Ansi16, backgrounds),
            Style::default().fg(Color::Green)
        );
        assert_eq!(
            style_del(DiffTheme::Dark, DiffColorLevel::Ansi16, backgrounds),
            Style::default().fg(Color::Red)
        );
        assert_eq!(
            style_sign_add(backgrounds),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        );
        assert_eq!(
            style_sign_del(backgrounds),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        );
    }

    #[test]
    fn weak_syntax_scope_backgrounds_do_not_override_semantic_palette() {
        let backgrounds = resolve_diff_backgrounds_for(
            DiffTheme::Dark,
            DiffColorLevel::TrueColor,
            DiffScopeBackgroundRgbs {
                inserted: Some((33, 58, 43)),
                deleted: Some((74, 34, 29)),
            },
        );
        assert_eq!(
            style_line_bg_for(DiffLineType::Insert, backgrounds),
            Style::default().bg(rgb_color(DARK_TC_ADD_LINE_BG_RGB))
        );
        assert_eq!(
            style_line_bg_for(DiffLineType::Delete, backgrounds),
            Style::default().bg(rgb_color(DARK_TC_DEL_LINE_BG_RGB))
        );
    }

    #[test]
    fn strong_custom_diff_scope_backgrounds_are_preserved() {
        let backgrounds = resolve_diff_backgrounds_for(
            DiffTheme::Dark,
            DiffColorLevel::TrueColor,
            DiffScopeBackgroundRgbs {
                inserted: Some((0, 96, 0)),
                deleted: Some((150, 0, 0)),
            },
        );
        assert_eq!(
            style_line_bg_for(DiffLineType::Insert, backgrounds),
            Style::default().bg(rgb_color((0, 96, 0)))
        );
        assert_eq!(
            style_line_bg_for(DiffLineType::Delete, backgrounds),
            Style::default().bg(rgb_color((150, 0, 0)))
        );
    }

    #[test]
    fn readable_syntax_foreground_is_preserved_over_diff_background() {
        let backgrounds = fallback_diff_backgrounds(DiffTheme::Dark, DiffColorLevel::TrueColor);
        let line_style = style_line_bg_for(DiffLineType::Insert, backgrounds);
        let syntax_style = Style::default()
            .fg(rgb_color((205, 214, 244)))
            .bg(rgb_color((1, 2, 3)))
            .add_modifier(Modifier::ITALIC);

        assert_eq!(
            style_syntax_for_diff(DiffLineType::Insert, syntax_style, line_style),
            Style::default()
                .fg(rgb_color((205, 214, 244)))
                .bg(rgb_color(DARK_TC_ADD_LINE_BG_RGB))
                .add_modifier(Modifier::ITALIC)
        );
    }

    #[test]
    fn low_contrast_syntax_foreground_uses_default_over_diff_background() {
        let backgrounds = fallback_diff_backgrounds(DiffTheme::Light, DiffColorLevel::TrueColor);
        let line_style = style_line_bg_for(DiffLineType::Insert, backgrounds);
        let syntax_style = Style::default().fg(rgb_color((223, 142, 29)));

        assert_eq!(
            style_syntax_for_diff(DiffLineType::Insert, syntax_style, line_style),
            Style::default().bg(rgb_color(LIGHT_TC_ADD_LINE_BG_RGB))
        );
    }

    #[test]
    fn ansi_syntax_foreground_uses_default_over_diff_background() {
        let backgrounds = fallback_diff_backgrounds(DiffTheme::Dark, DiffColorLevel::TrueColor);
        let line_style = style_line_bg_for(DiffLineType::Insert, backgrounds);
        let syntax_style = Style::default().fg(Color::Green);

        assert_eq!(
            style_syntax_for_diff(DiffLineType::Insert, syntax_style, line_style),
            Style::default().bg(rgb_color(DARK_TC_ADD_LINE_BG_RGB))
        );
    }

    #[test]
    fn delete_syntax_span_with_background_does_not_add_dim() {
        let backgrounds = fallback_diff_backgrounds(DiffTheme::Dark, DiffColorLevel::TrueColor);
        let line_style = style_line_bg_for(DiffLineType::Delete, backgrounds);
        let syntax_style = Style::default().fg(rgb_color((249, 226, 175)));

        assert_eq!(
            style_syntax_for_diff(DiffLineType::Delete, syntax_style, line_style),
            Style::default()
                .fg(rgb_color((249, 226, 175)))
                .bg(rgb_color(DARK_TC_DEL_LINE_BG_RGB))
        );
    }

    #[test]
    fn delete_syntax_span_without_background_adds_dim() {
        let syntax_style = Style::default().fg(Color::Green);

        assert_eq!(
            style_syntax_for_diff(DiffLineType::Delete, syntax_style, Style::default()),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::DIM)
        );
    }

    #[test]
    fn style_matrix_records_diff_semantic_colors() {
        let snapshot = [
            (
                "dark_truecolor",
                background_style_snapshot(
                    DiffTheme::Dark,
                    DiffColorLevel::TrueColor,
                    DiffScopeBackgroundRgbs::default(),
                ),
            ),
            (
                "light_truecolor",
                background_style_snapshot(
                    DiffTheme::Light,
                    DiffColorLevel::TrueColor,
                    DiffScopeBackgroundRgbs::default(),
                ),
            ),
            (
                "dark_ansi256",
                background_style_snapshot(
                    DiffTheme::Dark,
                    DiffColorLevel::Ansi256,
                    DiffScopeBackgroundRgbs::default(),
                ),
            ),
            (
                "dark_ansi16",
                background_style_snapshot(
                    DiffTheme::Dark,
                    DiffColorLevel::Ansi16,
                    DiffScopeBackgroundRgbs::default(),
                ),
            ),
            (
                "strong_custom",
                background_style_snapshot(
                    DiffTheme::Dark,
                    DiffColorLevel::TrueColor,
                    DiffScopeBackgroundRgbs {
                        inserted: Some((0, 96, 0)),
                        deleted: Some((150, 0, 0)),
                    },
                ),
            ),
        ]
        .into_iter()
        .map(|(name, snapshot)| format!("[{name}]\n{snapshot}"))
        .collect::<Vec<_>>()
        .join("\n\n");

        assert_snapshot!("diff_style_matrix", snapshot);
    }

    #[test]
    fn windows_terminal_promotes_ansi16_to_truecolor_for_diffs() {
        assert_eq!(
            diff_color_level_for_terminal(
                StdoutColorLevel::Ansi16,
                TerminalName::WindowsTerminal,
                /*has_wt_session*/ false,
                /*has_force_color_override*/ false,
            ),
            DiffColorLevel::TrueColor
        );
    }

    #[test]
    fn wt_session_promotes_ansi16_to_truecolor_for_diffs() {
        assert_eq!(
            diff_color_level_for_terminal(
                StdoutColorLevel::Ansi16,
                TerminalName::Unknown,
                /*has_wt_session*/ true,
                /*has_force_color_override*/ false,
            ),
            DiffColorLevel::TrueColor
        );
    }

    #[test]
    fn non_windows_terminal_keeps_ansi16_diff_palette() {
        assert_eq!(
            diff_color_level_for_terminal(
                StdoutColorLevel::Ansi16,
                TerminalName::WezTerm,
                /*has_wt_session*/ false,
                /*has_force_color_override*/ false,
            ),
            DiffColorLevel::Ansi16
        );
    }

    #[test]
    fn wt_session_promotes_unknown_color_level_to_truecolor() {
        assert_eq!(
            diff_color_level_for_terminal(
                StdoutColorLevel::Unknown,
                TerminalName::WindowsTerminal,
                /*has_wt_session*/ true,
                /*has_force_color_override*/ false,
            ),
            DiffColorLevel::TrueColor
        );
    }

    #[test]
    fn non_wt_windows_terminal_keeps_unknown_color_level_conservative() {
        assert_eq!(
            diff_color_level_for_terminal(
                StdoutColorLevel::Unknown,
                TerminalName::WindowsTerminal,
                /*has_wt_session*/ false,
                /*has_force_color_override*/ false,
            ),
            DiffColorLevel::Ansi16
        );
    }

    #[test]
    fn explicit_force_override_keeps_ansi16_on_windows_terminal() {
        assert_eq!(
            diff_color_level_for_terminal(
                StdoutColorLevel::Ansi16,
                TerminalName::WindowsTerminal,
                /*has_wt_session*/ false,
                /*has_force_color_override*/ true,
            ),
            DiffColorLevel::Ansi16
        );
    }

    #[test]
    fn explicit_force_override_keeps_ansi256_on_windows_terminal() {
        assert_eq!(
            diff_color_level_for_terminal(
                StdoutColorLevel::Ansi256,
                TerminalName::WindowsTerminal,
                /*has_wt_session*/ true,
                /*has_force_color_override*/ true,
            ),
            DiffColorLevel::Ansi256
        );
    }
}
