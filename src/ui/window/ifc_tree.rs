// IFC Model Tree dock panel — the spatial hierarchy of the last imported
// IFC model (root → storeys → elements). Clicking an element leaf selects
// it in the viewport; the panel is empty until an IFC import populates it.

use iced::widget::{button, column, container, text};
use iced::{Background, Border, Element, Fill, Theme};

use crate::app::Message;
use crate::io::ifc::IfcTreeNode;

const INDENT: f32 = 14.0;

pub fn view<'a>(tree: &'a [IfcTreeNode], width: f32, auto_collapse: bool) -> Element<'a, Message> {
    let _ = (width, auto_collapse);
    let mut content = column![].spacing(0).width(Fill);
    if tree.is_empty() {
        content = content.push(
            text("Import an IFC model (IMPORTIFC) to populate the model tree.")
                .size(11)
                .width(Fill),
        );
    } else {
        for node in tree {
            content = push_nodes(content, std::slice::from_ref(node), 6.0);
        }
    }
    let body: Element<'_, Message> = container(content).width(Fill).padding([6, 8]).into();
    column![
        container(text("IFC Model Tree".to_string()).size(12))
            .width(Fill)
            .padding([6, 10]),
        body
    ]
    .width(Fill)
    .into()
}

fn push_nodes<'a>(
    mut out: iced::widget::Column<'a, Message>,
    nodes: &'a [IfcTreeNode],
    indent: f32,
) -> iced::widget::Column<'a, Message> {
    for node in nodes {
        out = out.push(row_for_node(node, indent));
        if !node.children.is_empty() {
            out = push_nodes(out, &node.children, indent + INDENT);
        }
    }
    out
}

fn row_for_node<'a>(node: &'a IfcTreeNode, indent: f32) -> Element<'a, Message> {
    let label = text(node.label.clone()).size(11);
    let padded = |extra: u16, content: iced::widget::Text<'a>| {
        container(content)
            .width(Fill)
            .padding(iced::Padding { top: 2.0, right: extra as f32, bottom: 2.0, left: 4.0 + indent })
    };
    match node.leaf_guid.as_ref() {
        Some(guid) => {
            let guid = guid.clone();
            button(padded(0, label))
                .on_press(Message::IfcTreeSelect(guid))
                .style(|theme: &Theme, status: button::Status| {
                    let palette = theme.palette();
                    button::Style {
                        background: matches!(status, button::Status::Hovered)
                            .then_some(Background::Color(palette.background.weak.color)),
                        ..Default::default()
                    }
                })
                .width(Fill)
                .into()
        }
        None => padded(0, label).into(),
    }
}
