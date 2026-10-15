//! The toolbar: the tools of the current layer's family, the current one shown pressed, and
//! the current tool's options. Choosing one is a command, so its key and its reason when it
//! cannot be used are the menu's.

use bt_editor::{
    ActionId, Editor, Enabled, FillShape, Intent, SelectionMode, ToolFamily, ToolKind, tool_action,
};

/// The tools, in the order the toolbar shows them.
const TILE_TOOLS: [ToolKind; 9] = [
    ToolKind::Brush,
    ToolKind::Terrain,
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
    let doc = editor.current();
    let in_template = doc.is_some_and(|doc| editor.template_of(doc).is_some());
    let family = doc.and_then(|doc| editor.tool_family(doc));
    let in_tileset = doc
        .and_then(|doc| editor.document(doc))
        .is_some_and(|d| d.tileset_editing().is_some());
    ui.horizontal(|ui| {
        let tools: &[ToolKind] = match family {
            Some(ToolFamily::Tile) => &TILE_TOOLS,
            Some(ToolFamily::Object) => &OBJECT_TOOLS,
            Some(ToolFamily::Any) | None => &[],
        };
        let map_tools: &[ToolKind] = if in_tileset || in_template {
            &[]
        } else {
            &[ToolKind::OffsetLayers, ToolKind::Explain, ToolKind::World]
        };
        for &tool in tools.iter().chain(map_tools) {
            if in_template && !matches!(tool, ToolKind::SelectObjects | ToolKind::EditPolygons) {
                continue;
            }
            // The collision editor's shapes are never tiles or text, as in Tiled.
            if in_tileset && matches!(tool, ToolKind::InsertTile | ToolKind::InsertText) {
                continue;
            }
            tool_button(ui, editor, tool, current == tool, out);
        }
        // A tool's options show only where the tool does.
        if !tools.contains(&current) {
            return;
        }
        if matches!(current, ToolKind::BucketFill | ToolKind::ShapeFill) {
            ui.separator();
            let terrain = editor.tools().terrain_fill();
            if ui
                .selectable_label(terrain, "Terrain fill")
                .on_hover_text("Fill with the terrain chosen in the Terrains panel, not the brush")
                .clicked()
            {
                out.push(Intent::SetTerrainFill(!terrain));
            }
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
    if let Some(world) = doc.and_then(|doc| editor.world_of(doc))
        && let Some(asset) = editor.store().get(world)
    {
        ui.horizontal(|ui| {
            let title = format!(
                "World: {}{}",
                asset.display_name(),
                if asset.is_dirty() { " *" } else { "" }
            );
            ui.label(title);
            for action in [
                ActionId::WorldProperties,
                ActionId::AddMapToWorld,
                ActionId::UnloadWorld,
            ] {
                if ui.small_button(action.spec().label).clicked() {
                    out.push(Intent::Run(action));
                }
            }
        });
    }
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
