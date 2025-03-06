use std::{any::TypeId, marker::PhantomData};

use bevy::app::{App, Plugin};
use bevy::math::Vec2;
use bevy::prelude::Resource;

pub struct ActionPlugin<A: Action>(PhantomData<A>);

impl<A: Action> Default for ActionPlugin<A> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<A: Action> Plugin for ActionPlugin<A> {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActionList>()
            .init_resource::<ActionState<A>>();
        app.world_mut().resource_mut::<ActionList>().0.push(A::info());
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum ActionType {
    Bool,
    Float,
    Vector,
}

pub trait ActionTy: Send + Sync + Default + Clone + Copy {
    const TYPE: ActionType;
}

impl ActionTy for bool {
    const TYPE: ActionType = ActionType::Bool;
}

impl ActionTy for f32 {
    const TYPE: ActionType = ActionType::Float;
}

impl ActionTy for Vec2 {
    const TYPE: ActionType = ActionType::Float;
}

pub trait Action: Send + Sync + 'static {
    type ActionType: ActionTy;

    fn info() -> ActionInfo;
}

pub struct ActionInfo {
    pub pretty_name: &'static str,
    pub name: &'static str,
    pub action_type: ActionType,
    pub type_id: TypeId,
}

#[derive(Resource, Default)]
pub struct ActionList(pub Vec<ActionInfo>);

#[derive(Resource)]
pub struct ActionState<A: Action> {
    previous_state: A::ActionType,
    current_state: A::ActionType,
}

impl<A: Action> Default for ActionState<A> {
    fn default() -> Self {
        Self {
            previous_state: Default::default(),
            current_state: Default::default(),
        }
    }
}

impl<A: Action> ActionState<A> {
    pub fn current_state(&self) -> A::ActionType {
        self.current_state
    }

    pub fn previous_state(&self) -> A::ActionType {
        self.previous_state
    }

    pub fn set(&mut self, state: A::ActionType) {
        self.previous_state = std::mem::replace(&mut self.current_state, state);
    }
}

impl<A: Action<ActionType = bool>> ActionState<A> {
    pub fn pressed(&self) -> bool {
        self.current_state
    }

    pub fn just_pressed(&self) -> bool {
        !self.previous_state && self.current_state
    }

    pub fn just_released(&self) -> bool {
        self.previous_state && !self.current_state
    }

    pub fn press(&mut self) {
        self.current_state = true
    }
}
