//! Parameter Set shortcuts compose an existing parameter placement with its
//! corresponding action or table editor.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ParameterSet {
    pub parameter: &'static str,
    pub follow_up: &'static str,
}

pub(crate) fn get(label: &str) -> Option<ParameterSet> {
    let value = match label {
        "Point Move" => ("POINT", "BACTION MOVE"),
        "Linear Move" => ("LINEAR", "BACTION MOVE"),
        "Linear Stretch" => ("LINEAR", "BACTION STRETCH"),
        "Linear Array" => ("LINEAR", "BACTION ARRAY"),
        "Polar Move" => ("POLAR", "BACTION MOVE"),
        "Polar Stretch" => ("POLAR", "BACTION POLAR STRETCH"),
        "Polar Array" => ("POLAR", "BACTION ARRAY"),
        "XY Move" => ("XY", "BACTION MOVE"),
        "XY Move Box Set" => ("XY", "XY_MOVE_BOX_SET"),
        "Linear Move Pair" => ("LINEAR", "MOVE_PAIR"),
        "Polar Move Pair" => ("POLAR", "MOVE_PAIR"),
        "XY Move Pair" => ("XY", "MOVE_PAIR"),
        "Linear Stretch Pair" => ("LINEAR", "STRETCH_PAIR"),
        "Polar Stretch Pair" => ("POLAR", "STRETCH_PAIR"),
        "XY Stretch Box Set" => ("XY", "XY_STRETCH_BOX_SET"),
        "XY Array Box Set" => ("XY", "BACTION ARRAY"),
        "Rotation Set" => ("ROTATION", "BACTION ROTATE"),
        "Flip Set" => ("FLIP", "BACTION FLIP"),
        "Visibility Set" => ("VISIBILITY", "BVSTATE DIALOG"),
        "Lookup Set" => ("LOOKUP", "BLOOKUPTABLE"),
        _ => return None,
    };
    Some(ParameterSet {
        parameter: value.0,
        follow_up: value.1,
    })
}
