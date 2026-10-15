//! The custom properties of one or several things of one kind, shown and changed together.

use bt_core::{CustomTypes, Map, Properties, Tileset};

use super::change::{Change, Plan};
use super::error::PropertyError;
use super::lookup::{
    class_members, class_members_mut, map_properties, map_properties_mut, tileset_properties,
    tileset_properties_mut,
};
use super::shown::{Shown, rows};
use super::target::Target;
use super::target_id::TargetId;

/// The custom properties of one or several things of one kind (the map, layers, objects, the
/// tileset, tiles): the rows the Properties panel shows for them, and changes that edit each
/// one's own properties. Its targets borrow what they were read from; a change applies to a
/// copy of that map or tileset, the one a transaction edits.
#[derive(Clone, Debug)]
pub struct PropertySet<'a> {
    targets: Vec<Target<'a>>,
    types: &'a CustomTypes,
}

/// A target's properties after a change.
struct Update {
    target: TargetId,
    properties: Properties,
}

impl<'a> PropertySet<'a> {
    /// The targets in the order given, a target given twice once; `types` are the project's.
    pub fn of(targets: Vec<Target<'a>>, types: &'a CustomTypes) -> PropertySet<'a> {
        let mut unique: Vec<Target<'a>> = Vec::with_capacity(targets.len());
        for target in targets {
            if !unique.iter().any(|kept| kept.id() == target.id()) {
                unique.push(target);
            }
        }
        PropertySet {
            targets: unique,
            types,
        }
    }

    /// The targets; rows name them by their index here.
    pub fn targets(&self) -> &[Target<'a>] {
        &self.targets
    }

    /// The rows: every property any target has, its own or inherited, by name, with class
    /// values expanded to their members and lists to their items.
    pub fn rows(&self) -> Vec<Shown> {
        rows(&self.targets, self.types)
    }

    /// Applies a change to a standalone property collection. Refused unless this set contains
    /// exactly the standalone target it was built from; failed plans leave the collection intact.
    pub fn apply_to_properties(
        &self,
        properties: &mut Properties,
        change: &Change,
    ) -> Result<bool, PropertyError> {
        let Some(target) = self
            .targets
            .first()
            .filter(|target| self.targets.len() == 1 && target.id() == TargetId::Properties)
        else {
            return Err(PropertyError::StandaloneRequired);
        };
        let plan = Plan::new(change, &self.targets, self.types)?;
        if let Some(changed) = plan.applied_to(target, properties) {
            *properties = changed;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Applies `change` to the targets in `map` (the map, its layers, its objects), each
    /// target's own properties on their own. Returns whether any changed; nothing changes when
    /// the change is refused.
    pub fn apply_to_map(&self, map: &mut Map, change: &Change) -> Result<bool, PropertyError> {
        let plan = Plan::new(change, &self.targets, self.types)?;
        let mut updates = Vec::new();
        for target in &self.targets {
            let stored = map_properties(map, target.id())?;
            if let Some(properties) = plan.applied_to(target, stored) {
                updates.push(Update {
                    target: target.id(),
                    properties,
                });
            }
        }
        let changed = !updates.is_empty();
        for update in updates {
            *map_properties_mut(map, update.target)? = update.properties;
        }
        Ok(changed)
    }

    /// Applies `change` to the targets in `tileset` (the tileset, its tiles); a tile without
    /// data gets some when it gets properties. Returns whether any changed; nothing changes
    /// when the change is refused.
    pub fn apply_to_tileset(
        &self,
        tileset: &mut Tileset,
        change: &Change,
    ) -> Result<bool, PropertyError> {
        let plan = Plan::new(change, &self.targets, self.types)?;
        let mut updates = Vec::new();
        for target in &self.targets {
            let stored = tileset_properties(tileset, target.id())?;
            if let Some(properties) = plan.applied_to(target, stored) {
                updates.push(Update {
                    target: target.id(),
                    properties,
                });
            }
        }
        let changed = !updates.is_empty();
        for update in updates {
            *tileset_properties_mut(tileset, update.target)? = update.properties;
        }
        Ok(changed)
    }

    /// Applies `change` to the targets in `types` (classes' members), a copy of the types the
    /// targets were read from. Returns whether any changed; nothing changes when the change is
    /// refused.
    pub fn apply_to_types(
        &self,
        types: &mut CustomTypes,
        change: &Change,
    ) -> Result<bool, PropertyError> {
        let plan = Plan::new(change, &self.targets, self.types)?;
        let mut updates = Vec::new();
        for target in &self.targets {
            let TargetId::Members(class) = target.id() else {
                return Err(PropertyError::NotInTypes(target.id()));
            };
            let stored = class_members(types, class)?;
            if let Some(properties) = plan.applied_to(target, stored) {
                updates.push(Update {
                    target: target.id(),
                    properties,
                });
            }
        }
        let changed = !updates.is_empty();
        for update in updates {
            if let TargetId::Members(class) = update.target {
                *class_members_mut(types, class)? = update.properties;
            }
        }
        Ok(changed)
    }
}
