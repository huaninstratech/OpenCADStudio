use crate::app::{
    settings::{BlockEditorPreferences, BLOCK_DISPLAY_VARIABLES},
    Message,
};
use iced::{
    widget::{button, checkbox, column, container, row, text, text_input, Space},
    Element, Fill,
};

#[derive(Clone, Debug)]
pub(crate) struct State {
    pub values: Vec<String>,
    pub error: Option<String>,
}

impl State {
    pub(crate) fn new(prefs: &BlockEditorPreferences) -> Self {
        Self {
            values: BLOCK_DISPLAY_VARIABLES
                .iter()
                .map(|(name, _)| prefs.display_variable(name).unwrap())
                .collect(),
            error: None,
        }
    }
}

pub(crate) fn view_window(state: &State) -> Element<'_, Message> {
    let mut fields = column![].spacing(12);
    for (index, ((name, label), value)) in BLOCK_DISPLAY_VARIABLES
        .iter()
        .zip(&state.values)
        .enumerate()
    {
        if matches!(
            *name,
            "BPTEXTHORIZONTAL"
                | "BDEPENDENCYHIGHLIGHT"
                | "BTMARKDISPLAY"
                | "GRIPTIPS"
                | "BACTIONBARMODE"
        ) {
            fields = fields.push(checkbox(value == "1").label(crate::t!(*label)).on_toggle(
                move |value| Message::BlockEditorSettingsInput(index, u8::from(value).to_string()),
            ));
        } else {
            fields = fields.push(
                row![
                    text(crate::t!(*label)).width(240),
                    text_input(name, value)
                        .on_input(move |value| Message::BlockEditorSettingsInput(index, value))
                ]
                .spacing(12)
                .align_y(iced::Center),
            );
        }
    }
    fields = fields.push(text(crate::t!("Colors: BYLAYER, BYBLOCK, 1–255, or RGB:r,g,b. Fonts: family, file, or primary,bigfont.")).size(12));
    if let Some(error) = &state.error {
        fields = fields.push(text(error).style(iced::widget::text::danger));
    }
    fields = fields.push(
        row![
            button(text(crate::t!("Reset to Defaults")))
                .on_press(Message::BlockEditorSettingsReset),
            Space::new().width(Fill),
            button(text(crate::t!("Cancel"))).on_press(Message::CloseModal),
            button(text(crate::t!("Apply"))).on_press(Message::BlockEditorSettingsApply),
        ]
        .spacing(8),
    );
    container(fields).padding(18).width(670).into()
}
