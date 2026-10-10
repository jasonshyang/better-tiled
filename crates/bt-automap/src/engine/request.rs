//! What a run is asked to do: which part of the map, with which seed, and which map layers stand
//! for the names the rules use.

use bt_core::LayerId;
use bt_utils::IRect;

/// Which part of the map a run covers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Area {
    /// The whole map: Tiled's AutoMap with nothing selected.
    #[default]
    Whole,
    /// Around the cells of a selection, as Tiled runs on a selection: rules are tried where
    /// their input box overlaps a selected cell (only the selection's cells, not everything
    /// inside their bounds), and the region grows by what earlier rule maps wrote.
    Selection(Vec<IRect>),
    /// The cells an edit touched. An exact region run recomputes only what the edit can
    /// change; until those exist this runs on the whole map and says so in the report.
    Edit(IRect),
}

/// A name the rules use, bound to a map layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameBinding {
    /// The name, as the rules write it after `input_`, `output_` and the like.
    pub name: String,
    /// The map layer standing for it.
    pub layer: LayerId,
}

/// What happens to a name no binding gives a layer.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Unbound {
    /// Tiled's lookup: the bottom-most tile layer (or object layer, for objects) of that name;
    /// an output layer the map lacks is created on top.
    #[default]
    LookUpByName,
    /// Outputs go to temporary layers that later rule maps can read and that are dropped after
    /// the run; inputs read them, or nothing.
    Temporary,
}

/// Which map layers stand for the names the rules use. The default is Tiled's: every name looked
/// up by name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayerNames {
    pub(crate) tiles: Vec<NameBinding>,
    pub(crate) objects: Vec<NameBinding>,
    pub(crate) unbound: Unbound,
}

impl LayerNames {
    /// Tiled's lookup by name for every name.
    pub fn by_name() -> LayerNames {
        LayerNames::default()
    }

    /// Sets up bindings.
    pub fn builder() -> LayerNamesBuilder {
        LayerNamesBuilder::default()
    }

    /// The tile layer bound to `name`.
    pub fn tile_layer(&self, name: &str) -> Option<LayerId> {
        self.tiles.iter().find(|b| b.name == name).map(|b| b.layer)
    }

    /// The object layer bound to `name`.
    pub fn object_layer(&self, name: &str) -> Option<LayerId> {
        self.objects
            .iter()
            .find(|b| b.name == name)
            .map(|b| b.layer)
    }

    /// What happens to names without a binding.
    pub fn unbound(&self) -> Unbound {
        self.unbound
    }
}

/// Sets up [`LayerNames`].
#[derive(Clone, Debug, Default)]
pub struct LayerNamesBuilder {
    names: LayerNames,
}

impl LayerNamesBuilder {
    /// The tile layer `layer` stands for the name `name`, read and written.
    pub fn tiles(mut self, name: impl Into<String>, layer: LayerId) -> LayerNamesBuilder {
        self.names.tiles.push(NameBinding {
            name: name.into(),
            layer,
        });
        self
    }

    /// The object layer `layer` stands for the name `name` of object outputs.
    pub fn objects(mut self, name: impl Into<String>, layer: LayerId) -> LayerNamesBuilder {
        self.names.objects.push(NameBinding {
            name: name.into(),
            layer,
        });
        self
    }

    /// What happens to names without a binding ([`Unbound::LookUpByName`] by default).
    pub fn unbound(mut self, unbound: Unbound) -> LayerNamesBuilder {
        self.names.unbound = unbound;
        self
    }

    /// The bindings.
    pub fn build(self) -> LayerNames {
        self.names
    }
}

/// A run: its area, its seed and its layer names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRequest {
    pub(crate) area: Area,
    pub(crate) seed: u64,
    pub(crate) layer_names: LayerNames,
}

impl RunRequest {
    /// Sets up a run with `seed`, the whole map and Tiled's lookup of layers by name.
    pub fn builder(seed: u64) -> RunRequestBuilder {
        RunRequestBuilder {
            request: RunRequest {
                area: Area::Whole,
                seed,
                layer_names: LayerNames::by_name(),
            },
        }
    }

    /// The part of the map the run covers.
    pub fn area(&self) -> &Area {
        &self.area
    }

    /// The seed every random choice comes from.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The layers standing for the names the rules use.
    pub fn layer_names(&self) -> &LayerNames {
        &self.layer_names
    }
}

/// Sets up a [`RunRequest`].
#[derive(Clone, Debug)]
pub struct RunRequestBuilder {
    request: RunRequest,
}

impl RunRequestBuilder {
    /// The part of the map the run covers (the whole map by default).
    pub fn area(mut self, area: Area) -> RunRequestBuilder {
        self.request.area = area;
        self
    }

    /// The layers standing for the names the rules use (Tiled's lookup by name by default).
    pub fn layer_names(mut self, layer_names: LayerNames) -> RunRequestBuilder {
        self.request.layer_names = layer_names;
        self
    }

    /// The request.
    pub fn build(self) -> RunRequest {
        self.request
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_default_to_the_whole_map_and_names() {
        let request = RunRequest::builder(7).build();
        assert_eq!(request.area(), &Area::Whole);
        assert_eq!(request.seed(), 7);
        assert_eq!(request.layer_names(), &LayerNames::by_name());
        assert_eq!(request.layer_names().unbound(), Unbound::LookUpByName);
    }

    #[test]
    fn names_are_bound_to_layers() {
        let names = LayerNames::builder()
            .tiles("walls", LayerId(3))
            .objects("things", LayerId(4))
            .unbound(Unbound::Temporary)
            .build();
        let request = RunRequest::builder(1)
            .area(Area::Selection(vec![IRect::default()]))
            .layer_names(names.clone())
            .build();
        assert_eq!(request.layer_names().tile_layer("walls"), Some(LayerId(3)));
        assert_eq!(request.layer_names().tile_layer("things"), None);
        assert_eq!(names.object_layer("things"), Some(LayerId(4)));
        assert_eq!(names.unbound(), Unbound::Temporary);
    }
}
