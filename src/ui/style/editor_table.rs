use iced::widget::{text, text_input};
use iced::{Background, Theme};

/// Right-hand lane reserved for the scrollbar in editable command tables.
pub(crate) const EDITOR_TABLE_GUTTER: f32 = 16.0;

/// Text style for a value flagged as invalid in an editable command table.
pub(crate) fn danger_text_style(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(theme.palette().danger.base.color),
    }
}

/// Invalid-value field style shared by the Shortcuts and Aliases editors.
pub(crate) fn danger_input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let danger = theme.palette().danger.base;
    text_input::Style {
        background: Background::Color(theme.palette().danger.weak.color),
        border: iced::border::rounded(4).color(danger.color).width(
            if matches!(status, text_input::Status::Focused { .. }) {
                1.5
            } else {
                1.0
            },
        ),
        icon: danger.color,
        placeholder: danger.color.scale_alpha(0.7),
        value: danger.text,
        selection: danger.color,
    }
}

#[cfg(test)]
mod tests {
    use super::{danger_input_style, danger_text_style, EDITOR_TABLE_GUTTER};
    use iced::widget::text_input;
    use iced::{Background, Theme};

    #[test]
    fn invalid_editor_styles_keep_danger_palette_and_focus_border() {
        let theme = Theme::Dark;
        let palette = theme.palette();
        let text_style = danger_text_style(&theme);
        assert_eq!(text_style.color, Some(palette.danger.base.color));

        let active = danger_input_style(&theme, text_input::Status::Active);
        assert_eq!(
            active.background,
            Background::Color(palette.danger.weak.color)
        );
        assert_eq!(active.border.color, palette.danger.base.color);
        assert_eq!(active.border.width, 1.0);
        assert_eq!(active.border.radius, 4.0.into());
        assert_eq!(active.icon, palette.danger.base.color);
        assert_eq!(
            active.placeholder,
            palette.danger.base.color.scale_alpha(0.7)
        );
        assert_eq!(active.value, palette.danger.base.text);
        assert_eq!(active.selection, palette.danger.base.color);

        let focused = danger_input_style(&theme, text_input::Status::Focused { is_hovered: false });
        assert_eq!(focused.border.width, 1.5);
    }

    #[test]
    fn editor_table_gutter_is_sixteen_pixels() {
        assert_eq!(EDITOR_TABLE_GUTTER, 16.0);
    }
}
