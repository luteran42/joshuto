use lscolors::LsColors;
use ratatui::style::Style;
use std::path::Path;

use crate::config::app::AppConfig;
use crate::fs::{FileType, JoshutoDirEntry};
use crate::utils::unix;

use crate::THEME_T;

/// Allows patching a ratatui style if there is `Some` style, otherwise returns a clone.
pub trait PathStyleIfSome {
    fn patch_optionally(&self, other: Option<Style>) -> Style;
}

/// Path a ratatui style with another optional style.
/// If the other optional style is `None`, a clone of the original style is returned.
impl PathStyleIfSome for Style {
    fn patch_optionally(&self, other_option: Option<Style>) -> Style {
        if let Some(other) = other_option {
            self.patch(other)
        } else {
            *self
        }
    }
}

/// Returns the display style for `entry`: selection highlight takes priority, then LS_COLORS
/// (if enabled), then the theme's built-in per-type/per-extension styles.
pub fn entry_style(config: &AppConfig, entry: &JoshutoDirEntry) -> Style {
    let metadata = &entry.metadata;
    let filetype = metadata.file_type();

    if entry.is_marked_cut() {
        return mark_selected_style("cut");
    }
    if entry.is_marked_copy() {
        return mark_selected_style("copy");
    }
    if entry.is_marked_sym() {
        return mark_selected_style("symlink");
    }
    if entry.is_permanent_selected() {
        return permanent_selected_style();
    }
    if entry.is_visual_mode_selected() {
        return visual_mode_selected_style();
    }

    match &THEME_T.lscolors {
        Some(lscolors) => {
            let path = entry.file_path();
            lscolors_style(lscolors, path)
                .unwrap_or_else(|| default_style(config, entry, metadata, filetype))
        }
        None => default_style(config, entry, metadata, filetype),
    }
}

/// Returns the themed prefix string to display before `entry`'s name, based on its selection state.
pub fn entry_prefix(entry: &JoshutoDirEntry) -> &str {
    if entry.is_marked_cut() {
        if let Some(cut_mark) = THEME_T.mark.get("cut") {
            return &cut_mark.prefix;
        }
    }
    if entry.is_marked_copy() {
        if let Some(copy_mark) = THEME_T.mark.get("copy") {
            return &copy_mark.prefix;
        }
    }
    if entry.is_marked_sym() {
        if let Some(sym_mark) = THEME_T.mark.get("symlink") {
            return &sym_mark.prefix;
        }
    }
    if entry.is_visual_mode_selected() {
        &THEME_T.visual_mode_selection.prefix
    } else if entry.is_permanent_selected() {
        &THEME_T.selection.prefix
    } else {
        ""
    }
}

fn default_style(
    config: &AppConfig,
    entry: &JoshutoDirEntry,
    metadata: &crate::fs::JoshutoMetadata,
    filetype: FileType,
) -> Style {
    // Symlinks: style by whether the target actually resolves (broken links / loops get
    // `link_invalid`, resolved lazily via the cached follow-stat).
    if metadata.file_type() == FileType::Link {
        return if metadata.link_valid() {
            symlink_valid_style()
        } else {
            symlink_invalid_style()
        };
    }
    match filetype {
        FileType::Directory => directory_style(),
        FileType::File => file_style(config, entry),
        FileType::Link => Style::default()
            .fg(THEME_T.link.fg)
            .bg(THEME_T.link.bg)
            .add_modifier(THEME_T.link.modifier),
        _ => Style::default()
            .fg(THEME_T.socket.fg)
            .bg(THEME_T.socket.bg)
            .add_modifier(THEME_T.socket.modifier),
    }
}

fn visual_mode_selected_style() -> Style {
    Style::default()
        .fg(THEME_T.visual_mode_selection.fg)
        .bg(THEME_T.visual_mode_selection.bg)
        .add_modifier(THEME_T.visual_mode_selection.modifier)
}

pub fn permanent_selected_style() -> Style {
    Style::default()
        .fg(THEME_T.selection.fg)
        .bg(THEME_T.selection.bg)
        .add_modifier(THEME_T.selection.modifier)
}

pub fn mark_selected_style(file_op: &str) -> Style {
    if let Some(mark) = THEME_T.mark.get(file_op) {
        Style::default()
            .fg(mark.fg)
            .bg(mark.bg)
            .add_modifier(mark.modifier)
    } else {
        Style::default()
    }
}

pub fn symlink_valid_style() -> Style {
    Style::default()
        .fg(THEME_T.link.fg)
        .bg(THEME_T.link.bg)
        .add_modifier(THEME_T.link.modifier)
}

pub fn symlink_invalid_style() -> Style {
    Style::default()
        .fg(THEME_T.link_invalid.fg)
        .bg(THEME_T.link_invalid.bg)
        .add_modifier(THEME_T.link_invalid.modifier)
}

fn directory_style() -> Style {
    Style::default()
        .fg(THEME_T.directory.fg)
        .bg(THEME_T.directory.bg)
        .add_modifier(THEME_T.directory.modifier)
}

fn file_style(config: &AppConfig, entry: &JoshutoDirEntry) -> Style {
    let regular_style = Style::default()
        .fg(THEME_T.regular.fg)
        .bg(THEME_T.regular.bg)
        .add_modifier(THEME_T.regular.modifier);
    let metadata = &entry.metadata;
    if unix::is_executable(metadata.mode()) {
        Style::default()
            .fg(THEME_T.executable.fg)
            .bg(THEME_T.executable.bg)
            .add_modifier(THEME_T.executable.modifier)
    } else {
        entry
            .ext()
            .and_then(|s| {
                if config.case_insensitive_ext {
                    THEME_T.ext.get(&s.to_lowercase())
                } else {
                    THEME_T.ext.get(s)
                }
            })
            .map(|theme| {
                Style::default()
                    .fg(theme.fg)
                    .bg(theme.bg)
                    .add_modifier(theme.modifier)
            })
            .unwrap_or(regular_style)
    }
}

/// Maps a `nu_ansi_term` color to a ratatui color.
fn ratatui_color(color: nu_ansi_term::Color) -> ratatui::style::Color {
    use nu_ansi_term::Color as N;
    use ratatui::style::Color as R;
    match color {
        N::Black => R::Black,
        N::DarkGray => R::DarkGray,
        N::Red => R::Red,
        N::LightRed => R::LightRed,
        N::Green => R::Green,
        N::LightGreen => R::LightGreen,
        N::Yellow => R::Yellow,
        N::LightYellow => R::LightYellow,
        N::Blue => R::Blue,
        N::LightBlue => R::LightBlue,
        // ANSI color #5 is magenta; nu_ansi_term spells it both `Purple` and `Magenta`
        N::Purple | N::Magenta => R::Magenta,
        N::LightPurple | N::LightMagenta => R::LightMagenta,
        N::Cyan => R::Cyan,
        N::LightCyan => R::LightCyan,
        N::White => R::White,
        N::LightGray => R::Gray,
        N::Fixed(i) => R::Indexed(i),
        N::Rgb(r, g, b) => R::Rgb(r, g, b),
        N::Default => R::Reset,
    }
}

fn lscolors_style(lscolors: &LsColors, path: &Path) -> Option<Style> {
    let nu_style = lscolors.style_for_path(path)?.to_nu_ansi_term_style();
    let mut style = Style::default();
    if let Some(fg) = nu_style.foreground {
        style = style.fg(ratatui_color(fg));
    }
    if let Some(bg) = nu_style.background {
        style = style.bg(ratatui_color(bg));
    }
    let mut modifier = ratatui::style::Modifier::empty();
    let nu_ansi_term::Style {
        is_bold,
        is_dimmed,
        is_italic,
        is_underline,
        is_reverse,
        is_hidden,
        is_strikethrough,
        ..
    } = nu_style;
    if is_bold || is_dimmed {
        modifier |= ratatui::style::Modifier::BOLD;
    }
    if is_italic {
        modifier |= ratatui::style::Modifier::ITALIC;
    }
    if is_underline {
        modifier |= ratatui::style::Modifier::UNDERLINED;
    }
    if is_reverse {
        modifier |= ratatui::style::Modifier::REVERSED;
    }
    if is_hidden {
        modifier |= ratatui::style::Modifier::HIDDEN;
    }
    if is_strikethrough {
        modifier |= ratatui::style::Modifier::CROSSED_OUT;
    }
    Some(style.add_modifier(modifier))
}
