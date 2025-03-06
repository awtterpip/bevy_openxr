use std::borrow::Cow;
use std::ptr;

use bevy::ecs::schedule::ScheduleLabel;
use bevy::ecs::system::RunSystemOnce;
use bevy::platform_support::collections::HashMap;
use bevy::prelude::*;
use bevy_mod_xr::session::XrSessionCreatedEvent;
use openxr::sys::ActionSuggestedBinding;

use crate::resources::OxrInstance;

impl Plugin for OxrActionBindingPlugin {
    fn build(&self, app: &mut App) {
        app.add_schedule(Schedule::new(OxrSendActionBindings));
        app.add_event::<OxrSuggestActionBinding>();
        app.add_systems(
            Update,
            run_action_binding_sugestion.run_if(on_event::<XrSessionCreatedEvent>),
        );
    }
}

// This could for now be handled better with a SystemSet, but in the future we might want to add an
// Event to allow requesting binding suggestion for new actions
pub(crate) fn run_action_binding_sugestion(world: &mut World) {
    world.run_schedule(OxrSendActionBindings);
    _ = world.run_system_once(bind_actions);
}

fn bind_actions(instance: Res<OxrInstance>, mut actions: EventReader<OxrSuggestActionBinding>) {
    let mut bindings: HashMap<&str, Vec<ActionSuggestedBinding>, _> = HashMap::new();
    for e in actions.read() {
        bindings.entry(&e.interaction_profile).or_default().extend(
            e.bindings
                .clone()
                .into_iter()
                .filter_map(|b| match instance.string_to_path(&b) {
                    Ok(p) => Some(p),
                    Err(err) => {
                        error!(
                            "Unable to convert path: \"{}\"; error: {}",
                            b,
                            err.to_string()
                        );
                        None
                    }
                })
                .map(|p| ActionSuggestedBinding {
                    action: e.action,
                    binding: p,
                })
                .collect::<Vec<_>>(),
        );
    }
    use openxr::sys;
    for (profile, bindings) in bindings.iter() {
        let interaction_profile = match instance.string_to_path(profile) {
            Ok(v) => v,
            Err(err) => {
                error!(
                    "Unable to convert interaction profile path: \"{}\"; error: \"{}\" Skipping all suggestions for this interaction profile",
                    profile,
                    err.to_string()
                );
                continue;
            }
        };
        // Using the raw way since we want all actions through one event and we can't use the
        // Bindings from the openxr crate since they can't be created from raw actions
        let info = sys::InteractionProfileSuggestedBinding {
            ty: sys::InteractionProfileSuggestedBinding::TYPE,
            next: ptr::null(),
            interaction_profile,
            count_suggested_bindings: bindings.len() as u32,
            suggested_bindings: bindings.as_ptr() as *const _ as _,
        };
        match unsafe {
            (instance.fp().suggest_interaction_profile_bindings)(instance.as_raw(), &info)
        } {
            openxr::sys::Result::ERROR_ACTIONSETS_ALREADY_ATTACHED => error!(
                "Binding Suggested for an Action whith an ActionSet that was already attached!"
            ),
            openxr::sys::Result::ERROR_PATH_INVALID => error!("Invalid Path Suggested!"),
            openxr::sys::Result::ERROR_PATH_UNSUPPORTED => error!("Suggested Path Unsupported!"),
            _ => {}
        }
    }
}

#[derive(Event, Clone)]
/// Only Send this for Actions that were not attached yet!
pub struct OxrSuggestActionBinding {
    pub action: openxr::sys::Action,
    pub interaction_profile: Cow<'static, str>,
    pub bindings: Vec<Cow<'static, str>>,
}

pub struct OxrActionBindingPlugin;
// Maybe use a SystemSet in an XrStartup Schedule?
#[derive(ScheduleLabel, Hash, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OxrSendActionBindings;
