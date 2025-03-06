//! This plugin and module are here to ease the creation of actions withing openxr
//! The general idea is any plugin can create entities in startup before XRUtilsActionSystemSet::CreateEvents
//! this plugin will then create the neccessary actions sets, actions, and bindings and get them ready for use.
//!
//! example creating actions
//!
//!          //create a set
//!     let set = commands
//!     .spawn((
//!         XRUtilsActionSet {
//!             name: "flight".into(),
//!             pretty_name: "pretty flight set".into(),
//!             priority: u32::MIN,
//!         },
//!         ActiveSet, //marker to indicate we want this synced
//!     ))
//!     .id();
//!     //create an action
//!     let action = commands
//!     .spawn((
//!         XRUtilsAction {
//!             action_name: "flight_input".into(),
//!             localized_name: "flight_input_localized".into(),
//!             action_type: bevy_mod_xr::actions::ActionType::Vector,
//!         },
//!         FlightActionMarker, //lets try a marker component
//!     ))
//!     .id();
//!     
//!     //create a binding
//!     let binding = commands
//!     .spawn(XRUtilsBinding {
//!         profile: "/interaction_profiles/valve/index_controller".into(),
//!         binding: "/user/hand/right/input/thumbstick".into(),
//!     })
//!     .id();
//!     
//!     //add action to set, this isnt the best
//!     //TODO look into a better system
//!     commands.entity(action).add_child(binding);
//!     commands.entity(set).add_child(action);
//!
//! then you can read the action states after XRUtilsActionSystemSet::SyncActionStates
//! for example
//!
//! fn read_action_with_marker_component(
//!     mut action_query: Query<&XRUtilsActionState, With<FlightActionMarker>>,
//!     ) {
//!         //now for the actual checking
//!         for state in action_query.iter_mut() {
//!             info!("action state is: {:?}", state);
//!         }
//!     }
//!
//!
use bevy::prelude::*;
use bevy_mod_openxr::{
    action_binding::OxrSuggestActionBinding,
    action_set_attaching::OxrAttachActionSet,
    action_set_syncing::{OxrActionSetSyncSet, OxrSyncActionSet},
    openxr_session_available, openxr_session_running,
    resources::OxrInstance,
    session::OxrSession,
};
use openxr::{Path, Vector2f};

use std::borrow::Cow;

pub struct XRUtilsActionsPlugin;
impl Plugin for XRUtilsActionsPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            Startup,
            XRUtilsActionSystemSet::CreateEvents.run_if(openxr_session_available),
        );
        app.configure_sets(
            PreUpdate,
            XRUtilsActionSystemSet::SyncActionStates.run_if(openxr_session_running),
        );
        app.add_systems(
            Startup,
            create_openxr_events
                .in_set(XRUtilsActionSystemSet::CreateEvents)
                .run_if(openxr_session_available),
        );
        app.add_systems(
            Update,
            sync_active_action_sets.run_if(openxr_session_running),
        );
        app.add_systems(
            PreUpdate,
            sync_and_update_action_states_f32
                .run_if(openxr_session_running)
                .in_set(XRUtilsActionSystemSet::SyncActionStates)
                .after(OxrActionSetSyncSet),
        );
        app.add_systems(
            PreUpdate,
            sync_and_update_action_states_bool
                .run_if(openxr_session_running)
                .in_set(XRUtilsActionSystemSet::SyncActionStates)
                .after(OxrActionSetSyncSet),
        );
        app.add_systems(
            PreUpdate,
            sync_and_update_action_states_vector
                .run_if(openxr_session_running)
                .in_set(XRUtilsActionSystemSet::SyncActionStates)
                .after(OxrActionSetSyncSet),
        );
    }
}

fn create_openxr_events(
    action_sets_query: Query<(&XRUtilsActionSet, &Children, Entity)>,
    actions_query: Query<(&XRUtilsAction, &Children)>,
    bindings_query: Query<&XRUtilsBinding>,
    instance: ResMut<OxrInstance>,
    mut binding_writer: EventWriter<OxrSuggestActionBinding>,
    mut attach_writer: EventWriter<OxrAttachActionSet>,
    mut commands: Commands,
) {
    //lets create some sets!
    for (set, children, id) in action_sets_query.iter() {
        //create action set
        let action_set: openxr::ActionSet = instance
            .create_action_set(&set.name, &set.pretty_name, set.priority)
            .unwrap();
        //now that we have the action set we need to put it back onto the entity for later
        let oxr_action_set = XRUtilsActionSetReference(action_set.clone());
        commands.entity(id).insert(oxr_action_set);

        //since the actions are made from the sets lets go
        for child in children.iter() {
            //first get the action entity and stuff
            let (create_action, bindings) = actions_query.get(child).unwrap();
            //lets create dat action
            match create_action.action_type {
                bevy_mod_xr::actions::ActionType::Bool => {
                    let action: openxr::Action<bool> = action_set
                        .create_action::<bool>(
                            &create_action.action_name,
                            &create_action.localized_name,
                            &[],
                        )
                        .unwrap();
                    //please put this in a function so I dont go crazy
                    //insert a reference for later
                    commands.entity(child).insert((
                        ActionBooleference {
                            action: action.clone(),
                        },
                        XRUtilsActionState::Bool(ActionStateBool {
                            current_state: false,
                            changed_since_last_sync: false,
                            last_change_time: i64::MIN,
                            is_active: false,
                        }),
                    ));
                    //since we need actions for bindings lets go!!
                    for bind in bindings.iter() {
                        //interaction profile
                        //get the binding entity and stuff
                        let create_binding = bindings_query.get(bind).unwrap();
                        let profile = create_binding.profile.clone();
                        //bindings
                        let binding = vec![create_binding.binding.clone()];
                        let sugestion = OxrSuggestActionBinding {
                            action: action.as_raw(),
                            interaction_profile: profile,
                            bindings: binding,
                        };
                        //finally send the suggestion
                        binding_writer.send(sugestion);
                    }
                }
                bevy_mod_xr::actions::ActionType::Float => {
                    let action: openxr::Action<f32> = action_set
                        .create_action::<f32>(
                            &create_action.action_name,
                            &create_action.localized_name,
                            &[],
                        )
                        .unwrap();

                    //please put this in a function so I dont go crazy
                    //insert a reference for later
                    commands.entity(child).insert((
                        Actionf32Reference {
                            action: action.clone(),
                        },
                        XRUtilsActionState::Float(ActionStateFloat {
                            current_state: 0.0,
                            changed_since_last_sync: false,
                            last_change_time: i64::MIN,
                            is_active: false,
                        }),
                    ));
                    //since we need actions for bindings lets go!!
                    for bind in bindings.iter() {
                        //interaction profile
                        //get the binding entity and stuff
                        let create_binding = bindings_query.get(bind).unwrap();
                        let profile = create_binding.profile.clone();
                        //bindings
                        let binding = vec![create_binding.binding.clone()];
                        let sugestion = OxrSuggestActionBinding {
                            action: action.as_raw(),
                            interaction_profile: profile,
                            bindings: binding,
                        };
                        //finally send the suggestion
                        binding_writer.send(sugestion);
                    }
                }
                bevy_mod_xr::actions::ActionType::Vector => {
                    let action: openxr::Action<Vector2f> = action_set
                        .create_action::<Vector2f>(
                            &create_action.action_name,
                            &create_action.localized_name,
                            &[],
                        )
                        .unwrap();

                    //please put this in a function so I dont go crazy
                    //insert a reference for later
                    commands.entity(child).insert((
                        ActionVector2fReference {
                            action: action.clone(),
                        },
                        XRUtilsActionState::Vector(ActionStateVector {
                            current_state: [0.0, 0.0],
                            changed_since_last_sync: false,
                            last_change_time: i64::MIN,
                            is_active: false,
                        }),
                    ));
                    //since we need actions for bindings lets go!!
                    for bind in bindings.iter() {
                        //interaction profile
                        //get the binding entity and stuff
                        let create_binding = bindings_query.get(bind).unwrap();
                        let profile = create_binding.profile.clone();
                        //bindings
                        let binding = vec![create_binding.binding.clone()];
                        let sugestion = OxrSuggestActionBinding {
                            action: action.as_raw(),
                            interaction_profile: profile,
                            bindings: binding,
                        };
                        //finally send the suggestion
                        binding_writer.send(sugestion);
                    }
                }
            };
        }

        attach_writer.send(OxrAttachActionSet(action_set));
    }
}

fn sync_active_action_sets(
    mut sync_set: EventWriter<OxrSyncActionSet>,
    active_action_set_query: Query<&XRUtilsActionSetReference, With<ActiveSet>>,
) {
    for set in &active_action_set_query {
        sync_set.send(OxrSyncActionSet(set.0.clone()));
    }
}

fn sync_and_update_action_states_f32(
    session: Res<OxrSession>,
    mut f32_query: Query<(&Actionf32Reference, &mut XRUtilsActionState)>,
) {
    //now we do the action state for f32
    for (reference, mut silly_state) in f32_query.iter_mut() {
        let state = reference.action.state(&session, Path::NULL);
        match state {
            Ok(s) => {
                let new_state = XRUtilsActionState::Float(ActionStateFloat {
                    current_state: s.current_state,
                    changed_since_last_sync: s.changed_since_last_sync,
                    last_change_time: s.last_change_time.as_nanos(),
                    is_active: s.is_active,
                });

                *silly_state = new_state;
            }
            Err(_) => {
                info!("error getting action state");
            }
        }
    }
}

fn sync_and_update_action_states_bool(
    session: Res<OxrSession>,
    mut f32_query: Query<(&ActionBooleference, &mut XRUtilsActionState)>,
) {
    //now we do the action state for f32
    for (reference, mut silly_state) in f32_query.iter_mut() {
        let state = reference.action.state(&session, Path::NULL);
        match state {
            Ok(s) => {
                let new_state = XRUtilsActionState::Bool(ActionStateBool {
                    current_state: s.current_state,
                    changed_since_last_sync: s.changed_since_last_sync,
                    last_change_time: s.last_change_time.as_nanos(),
                    is_active: s.is_active,
                });

                *silly_state = new_state;
            }
            Err(_) => {
                info!("error getting action state");
            }
        }
    }
}

fn sync_and_update_action_states_vector(
    session: Res<OxrSession>,
    mut vector_query: Query<(&ActionVector2fReference, &mut XRUtilsActionState)>,
) {
    //now we do the action state for f32
    for (reference, mut silly_state) in vector_query.iter_mut() {
        let state = reference.action.state(&session, Path::NULL);
        match state {
            Ok(s) => {
                let new_state = XRUtilsActionState::Vector(ActionStateVector {
                    current_state: [s.current_state.x, s.current_state.y],
                    changed_since_last_sync: s.changed_since_last_sync,
                    last_change_time: s.last_change_time.as_nanos(),
                    is_active: s.is_active,
                });

                *silly_state = new_state;
            }
            Err(_) => {
                info!("error getting action state");
            }
        }
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy, SystemSet)]
pub enum XRUtilsActionSystemSet {
    /// Runs in Startup
    CreateEvents,
    /// Runs in PreUpdate
    SyncActionStates,
}

#[derive(Component)]
pub struct XRUtilsActionSet {
    pub name: Cow<'static, str>,
    pub pretty_name: Cow<'static, str>,
    pub priority: u32,
}

#[derive(Component, Clone)]
pub struct XRUtilsActionSetReference(pub openxr::ActionSet);

//I want to use this to indicate when an action set is attached
// #[derive(Component)]
// struct AttachedActionSet;

//this is used to determine if this set should be synced
#[derive(Component)]
pub struct ActiveSet;

#[derive(Component)]
pub struct XRUtilsAction {
    pub action_name: Cow<'static, str>,
    pub localized_name: Cow<'static, str>,
    pub action_type: bevy_mod_xr::actions::ActionType,
}

#[derive(Component)]
pub struct XRUtilsBinding {
    pub profile: Cow<'static, str>,
    pub binding: Cow<'static, str>,
}

//Prototype action states
//TODO refactor this
#[derive(Component, Debug)]
pub enum XRUtilsActionState {
    Bool(ActionStateBool),
    Float(ActionStateFloat),
    Vector(ActionStateVector),
}

#[derive(Debug)]
pub struct ActionStateBool {
    pub current_state: bool,
    pub changed_since_last_sync: bool,
    pub last_change_time: i64,
    pub is_active: bool,
}
#[derive(Debug)]
pub struct ActionStateFloat {
    pub current_state: f32,
    pub changed_since_last_sync: bool,
    pub last_change_time: i64,
    pub is_active: bool,
}
#[derive(Debug)]
pub struct ActionStateVector {
    pub current_state: [f32; 2],
    pub changed_since_last_sync: bool,
    pub last_change_time: i64,
    pub is_active: bool,
}

//prototype action references
//TODO refactor along with action states
#[derive(Component)]
struct Actionf32Reference {
    action: openxr::Action<f32>,
}

#[derive(Component)]
struct ActionBooleference {
    action: openxr::Action<bool>,
}

#[derive(Component)]
struct ActionVector2fReference {
    action: openxr::Action<Vector2f>,
}
