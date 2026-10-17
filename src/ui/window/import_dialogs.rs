//! Import File (IMPORT: the format first, then the file, routed to the
//! format's importer) and DGN Import (DGNIMPORT: model, target drawing,
//! conversion units and the mapping setup's effect on the file).

use iced::widget::{button, column, container, image, row, scrollable, text, text_input, Space};
use iced::{Background, Border, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button, dialog_button_styled_opt, field_style};
use crate::ui::window::pdf_dialogs::{card, well_style, PageThumb};

/// One edit in the Import File or DGN Import window.
#[derive(Debug, Clone)]
pub enum ImportDialogMsg {
    // Import File
    Path(String),
    Browse,
    /// A recent file chosen from its chip.
    Recent(usize),
    Format(usize),
    Search(String),
    OnlySupported(bool),
    Picked(Option<String>),
    ImportOk,
    // DGN Import
    DgnChange,
    DgnPicked(Option<String>),
    DgnModel(usize),
    DgnCurrent(bool),
    DgnPrefix(bool),
    DgnSub(bool),
    DgnExplode(bool),
    DgnMappingTab(usize),
    DgnOk,
    Help(&'static str),
}

/// The colour of a format that is read in part or only written.
const AMBER: iced::Color = iced::Color::from_rgb8(0xd9, 0xa4, 0x00);

fn msg(m: ImportDialogMsg) -> Message {
    Message::ImportDialog(m)
}

/// Whether OCS imports a format, and why not when it does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    Yes,
    /// Any file: the importer follows the file's extension.
    ByExtension,
    /// SAT: ACIS bodies are read only inside drawings.
    AcisSolverOnly,
    /// STEP: written by STEPOUT, not read.
    ExportOnly,
    No,
}

pub struct ImportFormat {
    pub name: &'static str,
    pub patterns: &'static str,
    pub extensions: &'static [&'static str],
    /// 0 2D / underlay, 1 3D model, 2 other.
    pub group: u8,
    pub support: Support,
}

const fn format(
    name: &'static str,
    patterns: &'static str,
    extensions: &'static [&'static str],
    group: u8,
    support: Support,
) -> ImportFormat {
    ImportFormat {
        name,
        patterns,
        extensions,
        group,
        support,
    }
}

/// The reference's Import File types, in its order of groups.
pub const FORMATS: [ImportFormat; 21] = [
    format("PDF Files", "*.pdf", &["pdf"], 0, Support::Yes),
    format("MicroStation DGN", "*.dgn", &["dgn"], 0, Support::Yes),
    format("All DGN Files", "*.*", &["*"], 0, Support::Yes),
    format("Metafile", "*.wmf", &["wmf"], 0, Support::No),
    format("3D Studio", "*.3ds", &["3ds"], 1, Support::No),
    format("ACIS", "*.sat", &["sat"], 1, Support::AcisSolverOnly),
    format(
        "CATIA V4",
        "*.model; *.session; *.exp; *.dlv3",
        &["model", "session", "exp", "dlv3"],
        1,
        Support::No,
    ),
    format(
        "CATIA V5",
        "*.CATPart; *.CATProduct",
        &["CATPart", "CATProduct"],
        1,
        Support::No,
    ),
    format("IGES", "*.igs; *.iges", &["igs", "iges"], 1, Support::No),
    format("Inventor", "*.ipt; *.iam", &["ipt", "iam"], 1, Support::No),
    format("JT", "*.jt", &["jt"], 1, Support::No),
    format("NX", "*.prt", &["prt"], 1, Support::No),
    format("Parasolid Binary", "*.x_b", &["x_b"], 1, Support::No),
    format("Parasolid Text", "*.x_t", &["x_t"], 1, Support::No),
    format("Pro/ENGINEER", "*.prt*; *.asm*", &["prt", "asm"], 1, Support::No),
    format("Pro/ENGINEER Granite", "*.g", &["g"], 1, Support::No),
    format("Pro/ENGINEER Neutral", "*.neu*", &["neu"], 1, Support::No),
    format("Rhino", "*.3dm", &["3dm"], 1, Support::No),
    format(
        "SolidWorks",
        "*.prt; *.sldprt; *.asm; *.sldasm",
        &["prt", "sldprt", "asm", "sldasm"],
        1,
        Support::No,
    ),
    format("STEP", "*.ste; *.stp; *.step", &["ste", "stp", "step"], 1, Support::ExportOnly),
    format("All Files", "*.*", &["*"], 2, Support::ByExtension),
];

pub struct ImportFileState {
    pub path: String,
    pub format: usize,
    pub search: String,
    pub only_supported: bool,
    pub error: Option<String>,
}

pub struct DgnImportState {
    pub path: String,
    pub version: &'static str,
    pub models: Vec<PageThumb>,
    pub model: usize,
    pub current_drawing: bool,
    pub prefix_duplicates: bool,
    pub sub_units: bool,
    pub explode_text_nodes: bool,
    /// Master and sub unit names.
    pub units: (String, String),
    pub mapping_tab: usize,
    /// The chosen model's mapping rows per tab (layer, linetype,
    /// lineweight, colour).
    pub mapping: [Vec<(String, String)>; 4],
}

/// Both windows' working copies and the files imported this session.
#[derive(Default)]
pub struct ImportDialogs {
    pub file: Option<ImportFileState>,
    pub dgn: Option<DgnImportState>,
    pub recent: Vec<String>,
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// A long path as its drive, an ellipsis and the last three parts.
fn short_path(path: &str) -> String {
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    if parts.len() <= 4 {
        return path.to_string();
    }
    format!("{}\\…\\{}", parts[0], parts[parts.len() - 3..].join("\\"))
}

fn badge<'a>(support: Support) -> Element<'a, Message> {
    let (label, tone) = match support {
        Support::Yes => (t!("Supported"), 0),
        Support::ByExtension => (t!("By extension"), 2),
        Support::AcisSolverOnly => (t!("ACIS solver available"), 1),
        Support::ExportOnly => (t!("Export only"), 1),
        Support::No => (t!("None"), 2),
    };
    let color = move |theme: &Theme| match tone {
        0 => theme.palette().success.base.color,
        1 => AMBER,
        _ => theme.palette().background.base.text.scale_alpha(0.55),
    };
    container(
        text(label)
            .size(10)
            .style(move |theme: &Theme| text::Style {
                color: Some(color(theme)),
            }),
    )
    .padding([1, 7])
    .style(move |theme: &Theme| container::Style {
        border: Border {
            width: 1.0,
            radius: 9.0.into(),
            color: color(theme),
        },
        ..Default::default()
    })
    .into()
}

/// The reason a format cannot be imported.
pub fn unsupported_reason(support: Support) -> Option<std::borrow::Cow<'static, str>> {
    match support {
        Support::Yes | Support::ByExtension => None,
        Support::AcisSolverOnly => Some(t!(
            "ACIS bodies are read only inside drawings; SAT files cannot be imported."
        )),
        Support::ExportOnly => Some(t!("STEP files are only written (STEPOUT), not read.")),
        Support::No => Some(t!("OpenCAD Studio has no importer for this format.")),
    }
}

fn tile_style(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status| {
        let palette = theme.palette();
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(if selected || hovered {
                palette.background.base.color
            } else {
                palette.background.weak.color
            })),
            text_color: palette.background.base.text,
            border: Border {
                width: if selected { 1.5 } else { 0.0 },
                radius: 6.0.into(),
                color: palette.primary.base.color,
            },
            ..Default::default()
        }
    }
}

/// Help, Cancel and the named action (inactive while `ok` is `None`).
fn footer<'a>(help: &'static str, action: std::borrow::Cow<'static, str>, ok: Option<Message>) -> Element<'a, Message> {
    row![
        button(text("?").size(12))
            .on_press(msg(ImportDialogMsg::Help(help)))
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
        dialog_button(t!("Cancel"), Message::CloseModal, false),
        dialog_button_styled_opt(action, ok, button::primary),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

fn chip<'a>(label: String, on: bool, message: Message) -> Element<'a, Message> {
    button(text(label).size(11))
        .on_press(message)
        .style(button_style(on))
        .padding([4, 10])
        .into()
}

/// One choice out of a few, as joined buttons; inactive ones are greyed.
fn segmented<'a>(options: Vec<(String, bool, Option<Message>)>) -> Element<'a, Message> {
    let buttons = options.into_iter().map(|(label, on, message)| {
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press_maybe(message)
            .style(button_style(on))
            .padding([5, 6])
            .width(Fill)
            .into()
    });
    container(iced::widget::Row::with_children(buttons.collect::<Vec<_>>()).spacing(2))
        .padding(2)
        .width(Fill)
        .style(well_style)
        .into()
}

pub fn view_import_file<'a>(
    state: &'a ImportFileState,
    recent: &'a [String],
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let mut chips = row![text(t!("Recent:")).size(11).style(muted_style)]
        .spacing(6)
        .align_y(iced::Center);
    for (n, path) in recent.iter().enumerate() {
        chips = chips.push(chip(
            file_name(path),
            state.path == *path,
            msg(ImportDialogMsg::Recent(n)),
        ));
    }
    let mut file = column![row![
        text_input("", &state.path)
            .on_input(|s| msg(ImportDialogMsg::Path(s)))
            .on_submit(msg(ImportDialogMsg::ImportOk))
            .size(12)
            .padding([6, 8])
            .width(Fill)
            .style(field_style),
        button(text(t!("Browse...")).size(11))
            .on_press(msg(ImportDialogMsg::Browse))
            .style(button_style(false))
            .padding([6, 14]),
    ]
    .spacing(8)
    .align_y(iced::Center)]
    .spacing(8);
    if !recent.is_empty() {
        file = file.push(chips);
    }
    if let Some(error) = &state.error {
        file = file.push(text(error.clone()).size(11).style(|theme: &Theme| text::Style {
            color: Some(theme.palette().danger.base.color),
        }));
    }

    let search = state.search.trim().to_lowercase();
    let shown: Vec<usize> = (0..FORMATS.len())
        .filter(|&k| {
            let f = &FORMATS[k];
            (!state.only_supported || matches!(f.support, Support::Yes | Support::ByExtension))
                && (search.is_empty()
                    || f.name.to_lowercase().contains(&search)
                    || f.patterns.to_lowercase().contains(&search))
        })
        .collect();
    let mut groups = column![].spacing(6);
    for (group, title) in [
        (0u8, t!("2D / Underlay")),
        (1, t!("3D Model")),
        (2, t!("Other")),
    ] {
        let members: Vec<usize> = shown
            .iter()
            .copied()
            .filter(|&k| FORMATS[k].group == group)
            .collect();
        if members.is_empty() {
            continue;
        }
        groups = groups.push(text(title.to_uppercase()).size(10).style(muted_style));
        for line in members.chunks(3) {
            let mut r = row![].spacing(6);
            for &k in line {
                let f = &FORMATS[k];
                r = r.push(
                    button(
                        row![
                            column![
                                text(f.name).size(12),
                                text(f.patterns).size(10).style(muted_style),
                            ]
                            .spacing(1)
                            .width(Fill),
                            badge(f.support),
                        ]
                        .spacing(6)
                        .align_y(iced::Center),
                    )
                    .on_press(msg(ImportDialogMsg::Format(k)))
                    .style(tile_style(state.format == k))
                    .padding([5, 8])
                    .width(Length::FillPortion(1)),
                );
            }
            for _ in line.len()..3 {
                r = r.push(Space::new().width(Length::FillPortion(1)));
            }
            groups = groups.push(r);
        }
    }
    let selected = &FORMATS[state.format];
    let reason = unsupported_reason(selected.support);
    let formats = card(
        format!("{} · {}", t!("Format"), FORMATS.len()),
        column![
            row![
                text_input(&t!("Search formats..."), &state.search)
                    .on_input(|s| msg(ImportDialogMsg::Search(s)))
                    .size(11)
                    .padding([5, 8])
                    .width(Length::Fixed(220.0))
                    .style(field_style),
                Space::new().width(Fill),
                chip(
                    t!("All").into_owned(),
                    !state.only_supported,
                    msg(ImportDialogMsg::OnlySupported(false))
                ),
                chip(
                    t!("Only what OCS can open").into_owned(),
                    state.only_supported,
                    msg(ImportDialogMsg::OnlySupported(true))
                ),
            ]
            .spacing(6)
            .align_y(iced::Center),
            scrollable(groups).height(Length::Fixed(360.0)),
            text(match &reason {
                Some(r) => format!("{}: {r}", selected.name),
                None => String::new(),
            })
            .size(11)
            .style(|_: &Theme| text::Style {
                color: Some(AMBER),
            }),
        ]
        .spacing(8),
    );
    let ok = reason.is_none().then(|| msg(ImportDialogMsg::ImportOk));
    column![
        card(t!("File").into_owned(), file),
        formats,
        footer("import", t!("Import"), ok),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}

pub fn view_dgn_import<'a>(
    state: &'a DgnImportState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let top = container(
        row![
            container(text(file_name(&state.path)).size(12))
                .padding([4, 10])
                .style(|theme: &Theme| container::Style {
                    border: Border {
                        width: 1.0,
                        radius: 12.0.into(),
                        color: theme.palette().primary.base.color,
                    },
                    ..Default::default()
                }),
            text(format!(
                "{} · {} · {}",
                short_path(&state.path),
                state.version,
                crate::tf!("{n} model(s)", n = state.models.len())
            ))
            .size(11)
            .style(muted_style)
            .width(Fill)
            .wrapping(iced::advanced::text::Wrapping::None),
            button(text(t!("Change file...")).size(11))
                .on_press(msg(ImportDialogMsg::DgnChange))
                .style(button::text)
                .padding([4, 6]),
        ]
        .spacing(10)
        .align_y(iced::Center),
    )
    .padding([8, 12])
    .width(Fill)
    .style(crate::ui::window::pdf_dialogs::card_style);

    let tiles = state.models.iter().enumerate().map(|(k, m)| {
        let chosen = k == state.model;
        let picture: Element<'a, Message> = match &m.image {
            Some(handle) => image(handle.clone()).width(Fill).height(Fill).into(),
            None => Space::new().width(Fill).height(Fill).into(),
        };
        button(
            column![
                container(picture)
                    .width(Length::Fixed(132.0))
                    .height(Length::Fixed(80.0))
                    .padding(4)
                    .style(well_style),
                text(m.label.clone())
                    .size(11)
                    .width(Length::Fixed(132.0))
                    .align_x(iced::Center),
            ]
            .spacing(4),
        )
        .on_press(msg(ImportDialogMsg::DgnModel(k)))
        .style(tile_style(chosen))
        .padding(5)
        .into()
    });
    let model = card(
        t!("Model").into_owned(),
        scrollable(
            iced::widget::Row::with_children(tiles.collect::<Vec<_>>())
                .spacing(6)
                .wrap()
                .vertical_spacing(6),
        )
        .height(Length::Fixed(118.0)),
    );
    let current = state.current_drawing;
    let target = card(
        t!("Target").into_owned(),
        column![
            segmented(vec![
                (
                    t!("New Drawing").into_owned(),
                    !current,
                    Some(msg(ImportDialogMsg::DgnCurrent(false)))
                ),
                (
                    t!("Current Drawing").into_owned(),
                    current,
                    Some(msg(ImportDialogMsg::DgnCurrent(true)))
                ),
            ]),
            text(t!("On name conflict")).size(10).style(muted_style),
            segmented(vec![
                (
                    t!("Add prefix to dependent definitions").into_owned(),
                    current && state.prefix_duplicates,
                    current.then(|| msg(ImportDialogMsg::DgnPrefix(true)))
                ),
                (
                    t!("Ignore duplicate names").into_owned(),
                    current && !state.prefix_duplicates,
                    current.then(|| msg(ImportDialogMsg::DgnPrefix(false)))
                ),
            ]),
        ]
        .spacing(6),
    );
    let unit = |name: &str| {
        let word = crate::modules::insert::dgn_import::unit_display(name);
        if word.is_empty() {
            String::new()
        } else {
            format!(" · {}", t!(word.as_str()))
        }
    };
    let units = card(
        t!("Units").into_owned(),
        column![
            segmented(vec![
                (
                    format!("{}{}", t!("Master unit"), unit(&state.units.0)),
                    !state.sub_units,
                    Some(msg(ImportDialogMsg::DgnSub(false)))
                ),
                (
                    format!("{}{}", t!("Sub unit"), unit(&state.units.1)),
                    state.sub_units,
                    Some(msg(ImportDialogMsg::DgnSub(true)))
                ),
            ]),
            row![
                iced::widget::toggler(state.explode_text_nodes)
                    .on_toggle(|on| msg(ImportDialogMsg::DgnExplode(on)))
                    .size(16),
                text(t!("Explode text nodes to text elements")).size(11),
            ]
            .spacing(8)
            .align_y(iced::Center),
        ]
        .spacing(8),
    );
    // The DGN readers do not read reference attachments, so their handling
    // is shown but cannot be chosen.
    let references = card(
        t!("External DGN references").into_owned(),
        column![
            segmented(vec![
                (t!("Bind as block / xref").into_owned(), false, None),
                (t!("Ignore").into_owned(), true, None),
                (t!("Attach as DGN underlay").into_owned(), false, None),
            ]),
            text(t!("Reference attachments of DGN files are not read; they are left out."))
                .size(11)
                .style(muted_style),
        ]
        .spacing(8),
    );
    let tabs = [t!("Layer"), t!("Linetype"), t!("Lineweight"), t!("Color")];
    let mut table = column![row![
        text("DGN").size(11).style(muted_style).width(Fill),
        text("DWG").size(11).style(muted_style).width(Fill),
    ]]
    .spacing(4);
    for (dgn, dwg) in &state.mapping[state.mapping_tab.min(3)] {
        table = table.push(row![
            text(dgn.clone()).size(11).width(Fill),
            text("\u{2192}").size(11).style(muted_style).width(Length::Fixed(24.0)),
            text(dwg.clone()).size(11).width(Fill),
        ]);
    }
    let mapping = card(
        t!("Property mapping").into_owned(),
        column![
            row![chip(
                "Standard".to_string(),
                true,
                msg(ImportDialogMsg::DgnMappingTab(state.mapping_tab))
            )],
            text(t!("Standard mapping setup.")).size(11).style(muted_style),
            segmented(
                tabs.iter()
                    .enumerate()
                    .map(|(k, label)| {
                        (
                            label.to_string(),
                            state.mapping_tab == k,
                            Some(msg(ImportDialogMsg::DgnMappingTab(k))),
                        )
                    })
                    .collect()
            ),
            scrollable(table).height(Length::Fixed(250.0)),
            text(t!("The preview shows the selected setup's effect on this file."))
                .size(11)
                .style(muted_style),
        ]
        .spacing(8),
    );
    column![
        top,
        row![
            column![model, target, units, references]
                .spacing(10)
                .width(Length::FillPortion(3)),
            container(mapping).width(Length::FillPortion(2)),
        ]
        .spacing(10),
        footer("dgn", t!("Import"), Some(msg(ImportDialogMsg::DgnOk))),
    ]
    .spacing(10)
    .padding([10, 12])
    .width(sizing.width)
    .into()
}
