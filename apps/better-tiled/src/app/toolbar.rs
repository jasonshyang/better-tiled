//! The toolbar: the tools of the current layer's family, the current one shown pressed, and
//! the current tool's options. Choosing one is a command, so its key and its reason when it
//! cannot be used are the menu's.

use bt_editor::{
    ActionId, Editor, Enabled, FillShape, Intent, SelectionMode, ToolFamily, ToolKind, tool_action,
};

/// The tools, in the order the toolbar shows them.
const TILE_TOOLS: [ToolKind; 8] = [
    ToolKind::Brush,
    ToolKind::Eraser,
    ToolKind::BucketFill,
    ToolKind::ShapeFill,
    ToolKind::Picker,
    ToolKind::RectSelect,
    ToolKind::MagicWand,
    ToolKind::SameTile,
];

const OBJECT_TOOLS: [ToolKind; 9] = [
    ToolKind::SelectObjects,
    ToolKind::EditPolygons,
    ToolKind::InsertRectangle,
    ToolKind::InsertEllipse,
    ToolKind::InsertCapsule,
    ToolKind::InsertPoint,
    ToolKind::InsertPolygon,
    ToolKind::InsertTile,
    ToolKind::InsertText,
];

const SHAPES: [(FillShape, &str); 2] = [
    (FillShape::Rectangle, "Rectangle"),
    (FillShape::Ellipse, "Ellipse"),
];

const MODES: [(SelectionMode, &str); 4] = [
    (SelectionMode::Replace, "Replace"),
    (SelectionMode::Add, "Add"),
    (SelectionMode::Subtract, "Subtract"),
    (SelectionMode::Intersect, "Intersect"),
];

/// Draws the toolbar; a chosen tool or option goes to `out`.
pub(crate) fn toolbar(ui: &mut egui::Ui, editor: &Editor, out: &mut Vec<Intent>) {
    let current = editor.tools().tool();
    let family = current_family(editor);
    ui.horizontal(|ui| {
        let tools: &[ToolKind] = match family {
            Some(ToolFamily::Tile) => &TILE_TOOLS,
            Some(ToolFamily::Object) => &OBJECT_TOOLS,
            Some(ToolFamily::Any) | None => &[],
        };
        for &tool in tools.iter().chain([&ToolKind::OffsetLayers]) {
            tool_button(ui, editor, tool, current == tool, out);
        }
        if current == ToolKind::ShapeFill {
            ui.separator();
            let shape = editor.tools().fill_shape();
            for (option, label) in SHAPES {
                if ui.selectable_label(shape == option, label).clicked() {
                    out.push(Intent::SetFillShape(option));
                }
            }
        }
        if matches!(current, ToolKind::Brush | ToolKind::Eraser) {
            ui.separator();
            if current == ToolKind::Brush {
                let random = editor.checked(ActionId::RandomMode).unwrap_or(false);
                if ui.selectable_label(random, "Random").clicked() {
                    out.push(Intent::Run(ActionId::RandomMode));
                }
            }
            ui.label(format!("Size {}", editor.tools().brush_size()));
            if ui.small_button("−").clicked() {
                out.push(Intent::Run(ActionId::BrushSizeDown));
            }
            if ui.small_button("+").clicked() {
                out.push(Intent::Run(ActionId::BrushSizeUp));
            }
        }
        if matches!(
            current,
            ToolKind::RectSelect | ToolKind::MagicWand | ToolKind::SameTile
        ) {
            ui.separator();
            let mode = editor.tools().selection_mode();
            for (option, label) in MODES {
                if ui.selectable_label(mode == option, label).clicked() {
                    out.push(Intent::SetSelectionMode(option));
                }
            }
        }
    });
}

fn tool_button(
    ui: &mut egui::Ui,
    editor: &Editor,
    tool: ToolKind,
    selected: bool,
    out: &mut Vec<Intent>,
) {
    let id: ActionId = tool_action(tool);
    let spec = id.spec();
    let enabled = editor.enabled(id) == Enabled::Enabled;
    let hint = match editor.keymap().key_text(id) {
        Some(key) => format!("{} ({key})", spec.label),
        None => spec.label.to_owned(),
    };
    let response = ui
        .add_enabled(enabled, egui::Button::selectable(selected, spec.label))
        .on_hover_text(hint);
    if response.clicked() {
        out.push(Intent::Run(id));
    }
}

/// The family of the current layer of the current document.
fn current_family(editor: &Editor) -> Option<ToolFamily> {
    let doc = editor.current()?;
    let id = editor.document(doc)?.selection().current_layer?;
    let layer = editor.map_of(doc)?.layer(id)?;
    Some(match layer.kind {
        bt_core::LayerKind::Tile(_) => ToolFamily::Tile,
        bt_core::LayerKind::Object(_) => ToolFamily::Object,
        bt_core::LayerKind::Image(_) | bt_core::LayerKind::Group(_) => ToolFamily::Any,
    })
}
