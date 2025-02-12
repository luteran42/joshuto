use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::fs::{JoshutoDirList, LinkType};
use crate::tab::TabDisplayOption;
use crate::utils::format;
use crate::utils::style::{
    mark_selected_style, permanent_selected_style, symlink_invalid_style, symlink_valid_style,
};
use crate::utils::unix;
use crate::{THEME_T, TIMEZONE_STR};

pub struct TuiFooter<'a> {
    dirlist: &'a JoshutoDirList,
    tab_options: &'a TabDisplayOption,
}

impl<'a> TuiFooter<'a> {
    pub fn new(dirlist: &'a JoshutoDirList, tab_options: &'a TabDisplayOption) -> Self {
        Self {
            dirlist,
            tab_options,
        }
    }
}

impl Widget for TuiFooter<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let visual_mode_style = Style::default().fg(Color::Black).bg(Color::LightRed);
        let mode_style = Style::default().fg(Color::Cyan);
        let file_name_style = Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD);
        let mtime_style = Style::default().add_modifier(Modifier::BOLD);
        let size_style = Style::default()
            .fg(Color::LightYellow)
            .add_modifier(Modifier::BOLD);

        // flat and filter commands indicator style
        let indicator_style = Style::default()
            .fg(Color::LightBlue)
            .add_modifier(THEME_T.selection.modifier);

        let selection_style = permanent_selected_style();
        let mark_cut_style = mark_selected_style("cut");
        let mark_copy_style = mark_selected_style("copy");
        let mark_sym_style = mark_selected_style("symlink");
        let selected_count = self.dirlist.selected_count();
        let marked_cut_count = self.dirlist.marked_cut_count();
        let marked_copy_count = self.dirlist.marked_copy_count();
        let marked_sym_count = self.dirlist.marked_sym_count();

        match self.dirlist.get_index() {
            Some(i) if i < self.dirlist.len() => {
                let entry = &self.dirlist.contents[i];

                let mode_str: String =
                    unix::mode_to_char_array(entry.metadata.mode, entry.metadata.file_type)
                        .iter()
                        .collect();

                let user_str = unix::uid_to_string(entry.metadata.uid).unwrap_or("unknown".into());
                let group_str = unix::gid_to_string(entry.metadata.gid).unwrap_or("unknown".into());

                let mtime_str = format::time_to_string(entry.metadata.modified());
                let size_str = format::file_size_to_string(entry.metadata.len());
                let path_str = entry.file_name();

                let path = self.dirlist.file_path();

                let mut text = vec![
                    Span::styled(
                        if self.dirlist.get_visual_mode_anchor_index().is_none() {
                            ""
                        } else {
                            "VIS"
                        },
                        visual_mode_style,
                    ),
                    Span::raw(if self.dirlist.get_visual_mode_anchor_index().is_none() {
                        ""
                    } else {
                        " "
                    }),
                    Span::styled(mode_str, mode_style),
                    Span::raw("  "),
                    Span::raw(user_str),
                    Span::raw(":"),
                    Span::raw(group_str),
                    Span::raw("  "),
                    Span::raw(format!("{}/{}", i + 1, self.dirlist.len())),
                    Span::raw("  "),
                    Span::styled(mtime_str, mtime_style),
                    Span::styled(TIMEZONE_STR.as_str(), mtime_style),
                    Span::styled(size_str, size_style),
                    Span::raw("  "),
                    Span::styled(path_str, file_name_style),
                    Span::raw("  "),
                    Span::styled(
                        match self.tab_options.dirlist_options_ref(&path.to_path_buf()) {
                            Some(opt) if opt.depth() > 0 => format!("flat:{} ", opt.depth()),
                            _ => "".to_owned(),
                        },
                        indicator_style,
                    ),
                    Span::styled(
                        match self.tab_options.dirlist_options_ref(&path.to_path_buf()) {
                            Some(opt) if !opt.filter_state_ref().is_none() => {
                                format!("filter:{} ", opt.filter_state_ref())
                            }
                            _ => "".to_owned(),
                        },
                        indicator_style,
                    ),
                    Span::styled(
                        if selected_count > 0 {
                            format!("{} selected", selected_count)
                        } else {
                            "".to_string()
                        },
                        selection_style,
                    ),
                    Span::raw(if marked_cut_count > 0 {
                        " | ".to_string()
                    } else {
                        "".to_string()
                    }),
                    Span::styled(
                        if marked_cut_count > 0 {
                            format!("{} marked -> cut", marked_cut_count)
                        } else {
                            "".to_string()
                        },
                        mark_cut_style,
                    ),
                    Span::raw(if marked_copy_count > 0 {
                        " | ".to_string()
                    } else {
                        "".to_string()
                    }),
                    Span::styled(
                        if marked_copy_count > 0 {
                            format!("{} marked -> copy", marked_copy_count)
                        } else {
                            "".to_string()
                        },
                        mark_copy_style,
                    ),
                    Span::raw(if marked_sym_count > 0 {
                        " | ".to_string()
                    } else {
                        "".to_string()
                    }),
                    Span::styled(
                        if marked_sym_count > 0 {
                            format!("{} marked -> symlink", marked_sym_count)
                        } else {
                            "".to_string()
                        },
                        mark_sym_style,
                    ),
                ];

                if let LinkType::Symlink { target, valid } = entry.metadata.link_type() {
                    let link_style = if *valid {
                        symlink_valid_style()
                    } else {
                        symlink_invalid_style()
                    };
                    text.push(Span::styled(" -> ", link_style));
                    text.push(Span::styled(target, link_style));
                }

                Paragraph::new(Line::from(text)).render(area, buf);
            }
            _ => {}
        }
    }
}
