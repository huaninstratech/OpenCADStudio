//! The contextual visibility controls share the ribbon's buttons, tips and collapse layout.
use iced::widget::{button, column, container, pick_list, row, text, tooltip};
use iced::{Element, Fill, Theme};

use super::collapse::Panel;
use super::widgets::{make_icon, tool_btn_style, LARGE_ICON, LARGE_W, SMALL_ICON, TOOL_BAR_H};
use crate::app::Message;
use crate::modules::IconKind;
use crate::ui::window::block_visibility::{Action, RibbonContext};
use crate::ui::ROW_H;

fn tool(
    id: &'static str,
    label: &'static str,
    icon: &'static [u8],
    large: bool,
    active: bool,
    message: Option<Message>,
) -> Element<'static, Message> {
    let enabled = message.is_some();
    let size = if large { LARGE_ICON } else { SMALL_ICON };
    let image = if enabled {
        crate::ui::icons::semantic(icon, size)
    } else {
        crate::ui::icons::semantic_disabled(icon, size)
    };
    let face: Element<'static, Message> = if large {
        column![
            image,
            text(crate::t!(label).into_owned())
                .size(10)
                .align_x(iced::Center)
        ]
        .align_x(iced::Center)
        .spacing(3)
        .into()
    } else {
        image
    };
    let mut control = button(face)
        .on_press_maybe(message)
        .style(move |theme: &Theme, status| {
            let mut style = tool_btn_style(theme, active, status);
            if !enabled {
                style.text_color = style.text_color.scale_alpha(0.42);
                style.background = None;
            }
            style
        })
        .padding([3, 5]);
    control = if large {
        control.width(LARGE_W + 8.0).height(3.0 * ROW_H)
    } else {
        control.width(ROW_H + 4.0).height(ROW_H)
    };
    tooltip(
        super::keytips::anchor(id, container(control).id(id)),
        super::tooltip_content(crate::t!(label).into_owned()),
        iced::widget::tooltip::Position::Right,
    )
    .gap(6)
    .delay(std::time::Duration::from_millis(400))
    .style(super::tooltip_style)
    .into()
}

fn view(context: Option<RibbonContext>, compact: bool) -> Element<'static, Message> {
    let target = context.as_ref().map(|context| context.target);
    let mode = context.as_ref().is_some_and(|context| context.mode);
    let manager = tool(
        "BVSTATE_MANAGER",
        "Visibility States",
        include_bytes!("../../../assets/icons/ui/list.svg"),
        !compact,
        false,
        target.map(|target| Message::BlockVisibility(Action::Open(target))),
    );
    let toggles = row![
        tool(
            "BVMODE_TOGGLE",
            "Visibility Mode",
            include_bytes!("../../../assets/icons/constrain/show_all.svg"),
            false,
            mode,
            target.map(|target| Message::BlockVisibility(Action::Mode(target)))
        ),
        tool(
            "BVSHOW",
            "Make Visible",
            include_bytes!("../../../assets/icons/constrain/show.svg"),
            false,
            false,
            target.map(|_| Message::Command("BVSHOW".into()))
        ),
        tool(
            "BVHIDE",
            "Make Invisible",
            include_bytes!("../../../assets/icons/constrain/hide_all.svg"),
            false,
            false,
            target.map(|_| Message::Command("BVHIDE".into()))
        ),
    ]
    .spacing(2);
    let names = context
        .as_ref()
        .map(|context| context.names.clone())
        .unwrap_or_default();
    let current = context.as_ref().and_then(|context| context.current.clone());
    let mut selector = pick_list(current, names, |name: &String| name.clone())
        .placeholder(crate::t!("Visibility state").into_owned())
        .text_size(11)
        .padding([3, 6])
        .width(Fill);
    if let Some(target) = target {
        selector =
            selector.on_select(move |name| Message::BlockVisibility(Action::Current(target, name)));
    }
    let selector = container(selector).id("BVSTATE_CURRENT").width(Fill);
    let controls: Element<'static, Message> = if compact {
        column![row![manager, toggles].spacing(3), selector]
            .spacing(4)
            .width(160)
            .height(Fill)
            .into()
    } else {
        row![manager, column![toggles, selector].spacing(6).width(170)]
            .spacing(4)
            .height(Fill)
            .into()
    };
    column![controls, text(crate::t!("Visibility").into_owned()).size(9)]
        .align_x(iced::Center)
        .padding([3, 4])
        .height(TOOL_BAR_H)
        .into()
}

fn collapsed(tight: bool) -> Element<'static, Message> {
    let icon = make_icon(
        IconKind::Svg(include_bytes!("../../../assets/icons/ui/list.svg")),
        if tight { SMALL_ICON } else { LARGE_ICON },
    );
    button(
        column![icon, text(crate::t!("Visibility ▾").into_owned()).size(10)]
            .align_x(iced::Center)
            .spacing(4),
    )
    .on_press(Message::ToggleRibbonPanel("Visibility".into()))
    .style(|theme, status| tool_btn_style(theme, false, status))
    .padding([4, 6])
    .into()
}

pub(super) fn panel(context: Option<RibbonContext>) -> Panel<'static> {
    Panel {
        id: "Visibility".into(),
        elements: [
            view(context.clone(), false),
            view(context, true),
            collapsed(false),
            collapsed(true),
        ],
    }
}
