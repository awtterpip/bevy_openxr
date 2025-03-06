use bevy::{
    ecs::{component::Component, entity::Entity},
    log::warn,
    math::bool,
    prelude::{Bundle, Commands, Deref, DerefMut, Resource, Transform, Visibility, World},
};
use crate::{session::XrTracker, spaces::XrSpaceLocationFlags};
pub const HAND_JOINT_COUNT: usize = 26;

pub fn spawn_hand_bones<T: Bundle>(
    cmds: &mut Commands,
    mut get_bundle: impl FnMut(HandBone) -> T,
) -> [Entity; HAND_JOINT_COUNT] {
    let mut bones: [Entity; HAND_JOINT_COUNT] = [Entity::PLACEHOLDER; HAND_JOINT_COUNT];
    for bone in HandBone::get_all_bones().into_iter() {
        bones[bone as usize] = cmds.spawn((bone, (get_bundle)(bone))).id();
    }
    bones
}

#[derive(Clone, Copy, Component, Debug)]
pub enum HandSide {
    Left,
    Right,
}

#[derive(Clone, Copy, Component, Debug)]
pub struct LeftHand;

#[derive(Clone, Copy, Component, Debug)]
pub struct RightHand;

/// Hand Joint Entities orderd
#[derive(Deref, DerefMut, Component, Clone, Copy)]
pub struct XrHandBoneEntities(pub [Entity; HAND_JOINT_COUNT]);

#[repr(transparent)]
#[derive(Clone, Copy, Component, Debug, DerefMut, Deref, Default)]
pub struct XrHandBoneRadius(pub f32);

#[repr(u8)]
#[derive(Clone, Copy, Component, Debug)]
#[require(
    XrSpaceLocationFlags,
    XrHandBoneRadius,
    Transform,
    Visibility,
    XrTracker
)]
pub enum HandBone {
    Palm = 0,
    Wrist = 1,
    ThumbMetacarpal = 2,
    ThumbProximal = 3,
    ThumbDistal = 4,
    ThumbTip = 5,
    IndexMetacarpal = 6,
    IndexProximal = 7,
    IndexIntermediate = 8,
    IndexDistal = 9,
    IndexTip = 10,
    MiddleMetacarpal = 11,
    MiddleProximal = 12,
    MiddleIntermediate = 13,
    MiddleDistal = 14,
    MiddleTip = 15,
    RingMetacarpal = 16,
    RingProximal = 17,
    RingIntermediate = 18,
    RingDistal = 19,
    RingTip = 20,
    LittleMetacarpal = 21,
    LittleProximal = 22,
    LittleIntermediate = 23,
    LittleDistal = 24,
    LittleTip = 25,
}

impl HandBone {
    pub const fn is_metacarpal(&self) -> bool {
        matches!(
            self,
            HandBone::ThumbMetacarpal
                | HandBone::IndexMetacarpal
                | HandBone::MiddleMetacarpal
                | HandBone::RingMetacarpal
                | HandBone::LittleMetacarpal
        )
    }
    pub const fn is_thumb(&self) -> bool {
        matches!(
            self,
            HandBone::ThumbMetacarpal
                | HandBone::ThumbProximal
                | HandBone::ThumbDistal
                | HandBone::ThumbTip
        )
    }
    pub const fn is_index(&self) -> bool {
        matches!(
            self,
            HandBone::IndexMetacarpal
                | HandBone::IndexProximal
                | HandBone::IndexIntermediate
                | HandBone::IndexDistal
                | HandBone::IndexTip
        )
    }
    pub const fn is_middle(&self) -> bool {
        matches!(
            self,
            HandBone::MiddleMetacarpal
                | HandBone::MiddleProximal
                | HandBone::MiddleIntermediate
                | HandBone::MiddleDistal
                | HandBone::MiddleTip
        )
    }
    pub const fn is_ring(&self) -> bool {
        matches!(
            self,
            HandBone::RingMetacarpal
                | HandBone::RingProximal
                | HandBone::RingIntermediate
                | HandBone::RingDistal
                | HandBone::RingTip
        )
    }
    pub const fn is_little(&self) -> bool {
        matches!(
            self,
            HandBone::LittleMetacarpal
                | HandBone::LittleProximal
                | HandBone::LittleIntermediate
                | HandBone::LittleDistal
                | HandBone::LittleTip
        )
    }

    pub const fn get_all_bones() -> [HandBone; 26] {
        [
            HandBone::Palm,
            HandBone::Wrist,
            HandBone::ThumbMetacarpal,
            HandBone::ThumbProximal,
            HandBone::ThumbDistal,
            HandBone::ThumbTip,
            HandBone::IndexMetacarpal,
            HandBone::IndexProximal,
            HandBone::IndexIntermediate,
            HandBone::IndexDistal,
            HandBone::IndexTip,
            HandBone::MiddleMetacarpal,
            HandBone::MiddleProximal,
            HandBone::MiddleIntermediate,
            HandBone::MiddleDistal,
            HandBone::MiddleTip,
            HandBone::RingMetacarpal,
            HandBone::RingProximal,
            HandBone::RingIntermediate,
            HandBone::RingDistal,
            HandBone::RingTip,
            HandBone::LittleMetacarpal,
            HandBone::LittleProximal,
            HandBone::LittleIntermediate,
            HandBone::LittleDistal,
            HandBone::LittleTip,
        ]
    }
}

/// Use by a backend to run custom logic when spawning a hand tracker
#[derive(Resource)]
pub struct SpawnHandTrackerCommandExecutor(pub fn(&mut World, Entity, HandSide));

/// `tracker_bundle` is inserted after the backend specific code is run
pub struct SpawnHandTracker<B: Bundle> {
    pub joints: XrHandBoneEntities,
    pub tracker_bundle: B,
    pub side: HandSide,
}

impl<B: Bundle> bevy::prelude::Command for SpawnHandTracker<B> {
    fn apply(self, world: &mut bevy::prelude::World) {
        let Some(executor) = world.remove_resource::<SpawnHandTrackerCommandExecutor>() else {
            warn!("no SpawnHandTracker executor defined, skipping handtracker creation");
            return;
        };
        let mut tracker = world.spawn(self.joints);
        match &self.side {
            HandSide::Left => tracker.insert((XrTracker, LeftHand)),
            HandSide::Right => tracker.insert((XrTracker, RightHand)),
        };
        let tracker = tracker.id();
        executor.0(world, tracker, self.side);
        if let Ok(mut tracker) = world.get_entity_mut(tracker) {
            tracker.insert(self.side);
            tracker.insert(self.tracker_bundle);
        }
        world.insert_resource(executor);
    }
}
