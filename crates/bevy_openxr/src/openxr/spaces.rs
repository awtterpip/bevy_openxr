use std::{mem::MaybeUninit, ptr, sync::Mutex};

use bevy::{prelude::*};
use bevy::platform_support::*;
use bevy::platform_support::hash::FixedHasher;
use bevy_mod_xr::{
    session::{XrFirst, XrHandleEvents},
    spaces::{
        XrDestroySpace, XrPrimaryReferenceSpace, XrReferenceSpace, XrSpace, XrSpaceLocationFlags,
        XrSpaceVelocityFlags, XrVelocity,
    },
};
use openxr::{
    sys, HandJointLocation, HandJointLocations, HandJointVelocities, HandJointVelocity,
    ReferenceSpaceType, SpaceLocationFlags, SpaceVelocityFlags, HAND_JOINT_COUNT,
};

use crate::{
    helper_traits::{ToPosef, ToQuat, ToVec3},
    openxr_session_available, openxr_session_running,
    resources::{OxrFrameState, OxrInstance, Pipelined},
    session::OxrSession,
};

#[derive(SystemSet, Hash, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OxrSpaceSyncSet;

/// VERY IMPORTANT!! only disable when you know what you are doing
pub struct OxrSpacePatchingPlugin;
impl Plugin for OxrSpacePatchingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Startup,
            patch_destroy_space.run_if(openxr_session_available),
        );
    }
}

pub struct OxrSpatialPlugin;
impl Plugin for OxrSpatialPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<XrDestroySpace>()
            .add_systems(
                XrFirst,
                destroy_space_event
                    .before(XrHandleEvents::Poll)
                    .run_if(openxr_session_available),
            )
            .add_systems(
                PreUpdate,
                update_space_transforms
                    .in_set(OxrSpaceSyncSet)
                    .run_if(openxr_session_running),
            )
            .register_required_components::<XrSpaceLocationFlags, OxrSpaceLocationFlags>()
            .register_required_components::<XrSpaceVelocityFlags, OxrSpaceVelocityFlags>();
    }
}

fn destroy_space_event(instance: Res<OxrInstance>, mut events: EventReader<XrDestroySpace>) {
    for space in events.read() {
        match instance.destroy_space(space.0) {
            Ok(_) => (),
            Err(err) => warn!("error while destroying space: {}", err),
        }
    }
}

pub static OXR_DO_NOT_CALL_DESTOY_SPACE_FOR_SPACES: Mutex<Option<bevy::platform_support::collections::hash_set::HashSet<u64, bevy::platform_support::hash::RandomState>>> = Mutex::new(None);
pub static OXR_ORIGINAL_DESTOY_SPACE: Mutex<Option<openxr::sys::pfn::DestroySpace>> =
    Mutex::new(None);

fn patch_destroy_space(instance: ResMut<OxrInstance>) {
    OXR_DO_NOT_CALL_DESTOY_SPACE_FOR_SPACES
        .lock()
        .unwrap()
        .replace(bevy::platform_support::collections::hash_set::HashSet::new());
    let raw_instance_ptr = instance.fp() as *const _ as *mut openxr::raw::Instance;
    unsafe {
        OXR_ORIGINAL_DESTOY_SPACE
            .lock()
            .unwrap()
            .replace((*raw_instance_ptr).destroy_space);

        (*raw_instance_ptr).destroy_space = patched_destroy_space;
    }
}
unsafe extern "system" fn patched_destroy_space(space: openxr::sys::Space) -> openxr::sys::Result {
    if !OXR_DO_NOT_CALL_DESTOY_SPACE_FOR_SPACES
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .contains(&space.into_raw())
    {
        OXR_ORIGINAL_DESTOY_SPACE
            .lock()
            .unwrap()
            .expect("has to be initialized")(space)
    } else {
        info!("Inject Worked, not destroying space");
        openxr::sys::Result::SUCCESS
    }
}

#[derive(Clone, Copy, Component, Default)]
pub struct OxrSpaceLocationFlags(pub openxr::SpaceLocationFlags);
impl OxrSpaceLocationFlags {
    pub fn pos_valid(&self) -> bool {
        self.0.contains(SpaceLocationFlags::POSITION_VALID)
    }
    pub fn pos_tracked(&self) -> bool {
        self.0.contains(SpaceLocationFlags::POSITION_TRACKED)
    }
    pub fn rot_valid(&self) -> bool {
        self.0.contains(SpaceLocationFlags::ORIENTATION_VALID)
    }
    pub fn rot_tracked(&self) -> bool {
        self.0.contains(SpaceLocationFlags::ORIENTATION_TRACKED)
    }
}
#[derive(Clone, Copy, Component, Default)]
pub struct OxrSpaceVelocityFlags(pub openxr::SpaceVelocityFlags);
impl OxrSpaceVelocityFlags {
    pub fn linear_valid(&self) -> bool {
        self.0.contains(SpaceVelocityFlags::LINEAR_VALID)
    }
    pub fn angular_valid(&self) -> bool {
        self.0.contains(SpaceVelocityFlags::ANGULAR_VALID)
    }
}

#[allow(clippy::type_complexity)]
fn update_space_transforms(
    session: Res<OxrSession>,
    default_ref_space: Res<XrPrimaryReferenceSpace>,
    pipelined: Option<Res<Pipelined>>,
    frame_state: Res<OxrFrameState>,
    mut query: Query<(
        &mut Transform,
        &XrSpace,
        Option<&mut XrVelocity>,
        Option<&XrReferenceSpace>,
        &mut OxrSpaceLocationFlags,
        &mut XrSpaceLocationFlags,
        Option<&mut OxrSpaceVelocityFlags>,
        Option<&mut XrSpaceVelocityFlags>,
    )>,
) {
    for (
        mut transform,
        space,
        velocity,
        ref_space,
        mut oxr_space_location_flags,
        mut xr_space_location_flags,
        oxr_space_velocity_flags,
        xr_space_velocity_flags,
    ) in &mut query
    {
        let ref_space = ref_space.unwrap_or(&default_ref_space);
        let time = if pipelined.is_some() {
            openxr::Time::from_nanos(
                frame_state.predicted_display_time.as_nanos()
                    + frame_state.predicted_display_period.as_nanos(),
            )
        } else {
            frame_state.predicted_display_time
        };
        let space_location = if let Some(mut velocity) = velocity {
            match session.locate_space_with_velocity(space, ref_space, time) {
                Ok((location, space_velocity)) => {
                    let flags = OxrSpaceVelocityFlags(space_velocity.velocity_flags);
                    if flags.linear_valid() {
                        velocity.linear = space_velocity.linear_velocity.to_vec3();
                    }
                    if flags.linear_valid() {
                        velocity.linear = space_velocity.linear_velocity.to_vec3();
                    }
                    let Some(mut vel_flags) = oxr_space_velocity_flags else {
                        error!("XrVelocity without OxrSpaceVelocityFlags");
                        return;
                    };
                    let Some(mut xr_vel_flags) = xr_space_velocity_flags else {
                        error!("XrVelocity without XrSpaceVelocityFlags");
                        return;
                    };
                    *vel_flags = flags;
                    xr_vel_flags.linear_valid = vel_flags.linear_valid();
                    xr_vel_flags.angular_valid = vel_flags.angular_valid();
                    Ok(location)
                }
                Err(err) => Err(err),
            }
        } else {
            session.locate_space(space, ref_space, time)
        };
        if let Ok(space_location) = space_location {
            let flags = OxrSpaceLocationFlags(space_location.location_flags);
            if flags.pos_valid() {
                transform.translation = space_location.pose.position.to_vec3();
            }
            if flags.rot_valid() {
                transform.rotation = space_location.pose.orientation.to_quat();
            }
            *oxr_space_location_flags = flags;
            xr_space_location_flags.position_tracked = flags.pos_valid() && flags.pos_tracked();
            xr_space_location_flags.rotation_tracked = flags.rot_valid() && flags.rot_tracked();
        }
    }
}

impl OxrSession {
    pub fn create_action_space<T: openxr::ActionTy>(
        &self,
        action: &openxr::Action<T>,
        subaction_path: openxr::Path,
        pose_in_space: Isometry3d,
    ) -> openxr::Result<XrSpace> {
        let info = sys::ActionSpaceCreateInfo {
            ty: sys::ActionSpaceCreateInfo::TYPE,
            next: ptr::null(),
            action: action.as_raw(),
            subaction_path,
            pose_in_action_space: pose_in_space.to_posef(),
        };
        let mut out = sys::Space::NULL;
        unsafe {
            cvt((self.instance().fp().create_action_space)(
                self.as_raw(),
                &info,
                &mut out,
            ))?;
            Ok(XrSpace::from_raw(out.into_raw()))
        }
    }
    pub fn create_reference_space(
        &self,
        ref_space_type: ReferenceSpaceType,
        pose_in_ref_space: Transform,
    ) -> openxr::Result<XrReferenceSpace> {
        let info = sys::ReferenceSpaceCreateInfo {
            ty: sys::ReferenceSpaceCreateInfo::TYPE,
            next: ptr::null(),
            reference_space_type: ref_space_type,
            pose_in_reference_space: pose_in_ref_space.to_posef(),
        };
        let mut out = sys::Space::NULL;
        unsafe {
            cvt((self.instance().fp().create_reference_space)(
                self.as_raw(),
                &info,
                &mut out,
            ))?;
            Ok(XrReferenceSpace(XrSpace::from_raw(out.into_raw())))
        }
    }
}
fn locate_space(
    instance: &openxr::Instance,
    space: &XrSpace,
    base: &XrSpace,
    time: openxr::Time,
) -> openxr::Result<openxr::SpaceLocation> {
    unsafe {
        let mut x = sys::SpaceLocation::out(ptr::null_mut());
        cvt((instance.fp().locate_space)(
            space.as_raw_openxr_space(),
            base.as_raw_openxr_space(),
            time,
            x.as_mut_ptr(),
        ))?;
        Ok(create_space_location(&x))
    }
}
fn locate_space_with_velocity(
    instance: &openxr::Instance,
    space: &XrSpace,
    base: &XrSpace,
    time: openxr::Time,
) -> openxr::Result<(openxr::SpaceLocation, openxr::SpaceVelocity)> {
    unsafe {
        let mut velocity = sys::SpaceVelocity::out(ptr::null_mut());
        let mut location = sys::SpaceLocation::out(&mut velocity as *mut _ as _);
        cvt((instance.fp().locate_space)(
            space.as_raw_openxr_space(),
            base.as_raw_openxr_space(),
            time,
            location.as_mut_ptr(),
        ))?;
        Ok((
            create_space_location(&location),
            create_space_velocity(&velocity),
        ))
    }
}
pub fn locate_hand_joints(
    instance: &openxr::Instance,
    tracker: &openxr::HandTracker,
    base: &XrSpace,
    time: openxr::Time,
) -> openxr::Result<Option<HandJointLocations>> {
    unsafe {
        let locate_info = sys::HandJointsLocateInfoEXT {
            ty: sys::HandJointsLocateInfoEXT::TYPE,
            next: ptr::null(),
            base_space: base.as_raw_openxr_space(),
            time,
        };
        let mut locations =
            MaybeUninit::<[openxr::HandJointLocation; openxr::HAND_JOINT_COUNT]>::uninit();
        let mut location_info = sys::HandJointLocationsEXT {
            ty: sys::HandJointLocationsEXT::TYPE,
            next: ptr::null_mut(),
            is_active: false.into(),
            joint_count: openxr::HAND_JOINT_COUNT as u32,
            joint_locations: locations.as_mut_ptr() as _,
        };
        cvt((instance
            .exts()
            .ext_hand_tracking
            .as_ref()
            .expect("Somehow created HandTracker without XR_EXT_hand_tracking being enabled")
            .locate_hand_joints)(
            tracker.as_raw(),
            &locate_info,
            &mut location_info,
        ))?;
        Ok(if location_info.is_active.into() {
            Some(locations.assume_init())
        } else {
            None
        })
    }
}
pub fn locate_hand_joints_with_velocities(
    instance: &openxr::Instance,
    tracker: &openxr::HandTracker,
    base: &XrSpace,
    time: openxr::Time,
) -> openxr::Result<Option<(HandJointLocations, HandJointVelocities)>> {
    unsafe {
        let locate_info = sys::HandJointsLocateInfoEXT {
            ty: sys::HandJointsLocateInfoEXT::TYPE,
            next: ptr::null(),
            base_space: base.as_raw_openxr_space(),
            time,
        };
        let mut velocities = MaybeUninit::<[HandJointVelocity; HAND_JOINT_COUNT]>::uninit();
        let mut velocity_info = sys::HandJointVelocitiesEXT {
            ty: sys::HandJointVelocitiesEXT::TYPE,
            next: ptr::null_mut(),
            joint_count: HAND_JOINT_COUNT as u32,
            joint_velocities: velocities.as_mut_ptr() as _,
        };
        let mut locations = MaybeUninit::<[HandJointLocation; HAND_JOINT_COUNT]>::uninit();
        let mut location_info = sys::HandJointLocationsEXT {
            ty: sys::HandJointLocationsEXT::TYPE,
            next: &mut velocity_info as *mut _ as _,
            is_active: false.into(),
            joint_count: HAND_JOINT_COUNT as u32,
            joint_locations: locations.as_mut_ptr() as _,
        };
        cvt((instance
            .exts()
            .ext_hand_tracking
            .as_ref()
            .expect("Somehow created HandTracker without XR_EXT_hand_tracking being enabled")
            .locate_hand_joints)(
            tracker.as_raw(),
            &locate_info,
            &mut location_info,
        ))?;
        Ok(if location_info.is_active.into() {
            Some((locations.assume_init(), velocities.assume_init()))
        } else {
            None
        })
    }
}
pub fn destroy_space(
    instance: &openxr::Instance,
    space: sys::Space,
) -> openxr::Result<sys::Result> {
    OXR_DO_NOT_CALL_DESTOY_SPACE_FOR_SPACES
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .remove(&space.into_raw());
    let result = unsafe { (instance.fp().destroy_space)(space) };
    cvt(result)
}
impl OxrSession {
    pub fn allow_auto_destruct_of_openxr_space(&self, space: &openxr::Space) {
        OXR_DO_NOT_CALL_DESTOY_SPACE_FOR_SPACES
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .remove(&space.as_raw().into_raw());
    }
    pub fn destroy_space(&self, space: XrSpace) -> openxr::Result<sys::Result> {
        destroy_space(self.instance(), space.as_raw_openxr_space())
    }
    pub fn destroy_openxr_space(&self, space: openxr::Space) -> openxr::Result<sys::Result> {
        destroy_space(self.instance(), space.as_raw())
    }
    pub fn locate_views(
        &self,
        view_configuration_type: openxr::ViewConfigurationType,
        display_time: openxr::Time,
        ref_space: &XrReferenceSpace,
    ) -> openxr::Result<(openxr::ViewStateFlags, Vec<openxr::View>)> {
        let info = sys::ViewLocateInfo {
            ty: sys::ViewLocateInfo::TYPE,
            next: ptr::null(),
            view_configuration_type,
            display_time,
            space: ref_space.as_raw_openxr_space(),
        };
        let (flags, raw) = unsafe {
            let mut out = sys::ViewState::out(ptr::null_mut());
            let raw = get_arr_init(sys::View::out(ptr::null_mut()), |cap, count, buf| {
                (self.instance().fp().locate_views)(
                    self.as_raw(),
                    &info,
                    out.as_mut_ptr(),
                    cap,
                    count,
                    buf as _,
                )
            })?;
            (out.assume_init().view_state_flags, raw)
        };
        Ok((
            flags,
            raw.into_iter()
                .map(|x| unsafe { create_view(flags, &x) })
                .collect(),
        ))
    }
    pub fn locate_space(
        &self,
        space: &XrSpace,
        base: &XrSpace,
        time: openxr::Time,
    ) -> openxr::Result<openxr::SpaceLocation> {
        locate_space(self.instance(), space, base, time)
    }
    pub fn locate_space_with_velocity(
        &self,
        space: &XrSpace,
        base: &XrSpace,
        time: openxr::Time,
    ) -> openxr::Result<(openxr::SpaceLocation, openxr::SpaceVelocity)> {
        locate_space_with_velocity(self.instance(), space, base, time)
    }
    pub fn locate_hand_joints(
        &self,
        tracker: &openxr::HandTracker,
        base: &XrSpace,
        time: openxr::Time,
    ) -> openxr::Result<Option<openxr::HandJointLocations>> {
        locate_hand_joints(self.instance(), tracker, base, time)
    }
    pub fn locate_hand_joints_with_velocities(
        &self,
        tracker: &openxr::HandTracker,
        base: &XrSpace,
        time: openxr::Time,
    ) -> openxr::Result<Option<(HandJointLocations, HandJointVelocities)>> {
        locate_hand_joints_with_velocities(self.instance(), tracker, base, time)
    }
}
impl OxrInstance {
    pub fn allow_auto_destruct_of_openxr_space(&self, space: &openxr::Space) {
        OXR_DO_NOT_CALL_DESTOY_SPACE_FOR_SPACES
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .remove(&space.as_raw().into_raw());
    }
    pub fn destroy_space(&self, space: XrSpace) -> openxr::Result<sys::Result> {
        destroy_space(self, space.as_raw_openxr_space())
    }
    pub fn destroy_openxr_space(&self, space: openxr::Space) -> openxr::Result<sys::Result> {
        destroy_space(self, space.as_raw())
    }
    pub fn locate_space(
        &self,
        space: &XrSpace,
        base: &XrSpace,
        time: openxr::Time,
    ) -> openxr::Result<openxr::SpaceLocation> {
        locate_space(self, space, base, time)
    }
    pub fn locate_space_with_velocity(
        &self,
        space: &XrSpace,
        base: &XrSpace,
        time: openxr::Time,
    ) -> openxr::Result<(openxr::SpaceLocation, openxr::SpaceVelocity)> {
        locate_space_with_velocity(self, space, base, time)
    }
    pub fn locate_hand_joints(
        &self,
        tracker: &openxr::HandTracker,
        base: &XrSpace,
        time: openxr::Time,
    ) -> openxr::Result<Option<openxr::HandJointLocations>> {
        locate_hand_joints(self, tracker, base, time)
    }
    pub fn locate_hand_joints_with_velocities(
        &self,
        tracker: &openxr::HandTracker,
        base: &XrSpace,
        time: openxr::Time,
    ) -> openxr::Result<Option<(HandJointLocations, HandJointVelocities)>> {
        locate_hand_joints_with_velocities(self, tracker, base, time)
    }
}

/// # Safety
/// This is an Extension trait. DO NOT IMPLEMENT IT!
pub unsafe trait OxrSpaceExt {
    /// get an openxr::sys::Space as a reference to the XrSpace
    /// does not remove the space from the space managment system!
    fn as_raw_openxr_space(&self) -> sys::Space;
    /// Adds the openxr::sys::Space into the the space managment system
    fn from_raw_openxr_space(space: sys::Space) -> Self;
    /// Adds the openxr::Space into the the space manegment system
    fn from_openxr_space(space: openxr::Space) -> Self;
    /// get an openxr::Space as a reference to the XrSpace
    /// does not remove the space from the space managment system!
    /// # Safety
    /// Session has to be the session from which the space is from
    unsafe fn as_openxr_space<T>(&self, session: &openxr::Session<T>) -> openxr::Space;
    /// get an openxr::Space as an onwned version of the XrSpace
    /// removes the space from the space managment system!
    /// # Safety
    /// Session has to be the session from which the space is from
    unsafe fn into_openxr_space<T>(self, session: &openxr::Session<T>) -> openxr::Space;
}

unsafe impl OxrSpaceExt for XrSpace {
    fn as_raw_openxr_space(&self) -> sys::Space {
        sys::Space::from_raw(self.as_raw())
    }

    fn from_raw_openxr_space(space: sys::Space) -> Self {
        let raw = space.into_raw();
        OXR_DO_NOT_CALL_DESTOY_SPACE_FOR_SPACES
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .insert(raw);
        unsafe { XrSpace::from_raw(raw) }
    }

    fn from_openxr_space(space: openxr::Space) -> Self {
        Self::from_raw_openxr_space(space.as_raw())
    }

    unsafe fn as_openxr_space<T>(&self, session: &openxr::Session<T>) -> openxr::Space {
        unsafe { openxr::Space::reference_from_raw(session.clone(), self.as_raw_openxr_space()) }
    }
    unsafe fn into_openxr_space<T>(self, session: &openxr::Session<T>) -> openxr::Space {
        OXR_DO_NOT_CALL_DESTOY_SPACE_FOR_SPACES
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .remove(&self.as_raw());
        unsafe { openxr::Space::reference_from_raw(session.clone(), self.as_raw_openxr_space()) }
    }
}

fn cvt(x: sys::Result) -> openxr::Result<sys::Result> {
    if x.into_raw() >= 0 {
        Ok(x)
    } else {
        Err(x)
    }
}
unsafe fn create_view(flags: openxr::ViewStateFlags, raw: &MaybeUninit<sys::View>) -> openxr::View {
    // Applications *must* not read invalid parts of a poses, i.e. they may be uninitialized
    let ptr = raw.as_ptr();
    openxr::View {
        pose: openxr::Posef {
            orientation: flags
                .contains(sys::ViewStateFlags::ORIENTATION_VALID)
                .then(|| *ptr::addr_of!((*ptr).pose.orientation))
                .unwrap_or_default(),
            position: flags
                .contains(sys::ViewStateFlags::POSITION_VALID)
                .then(|| *ptr::addr_of!((*ptr).pose.position))
                .unwrap_or_default(),
        },
        fov: *ptr::addr_of!((*ptr).fov),
    }
}
unsafe fn create_space_location(raw: &MaybeUninit<sys::SpaceLocation>) -> openxr::SpaceLocation {
    // Applications *must* not read invalid parts of a pose, i.e. they may be uninitialized
    let ptr = raw.as_ptr();
    let flags = *ptr::addr_of!((*ptr).location_flags);
    openxr::SpaceLocation {
        location_flags: flags,
        pose: openxr::Posef {
            orientation: flags
                .contains(sys::SpaceLocationFlags::ORIENTATION_VALID)
                .then(|| *ptr::addr_of!((*ptr).pose.orientation))
                .unwrap_or_default(),
            position: flags
                .contains(sys::SpaceLocationFlags::POSITION_VALID)
                .then(|| *ptr::addr_of!((*ptr).pose.position))
                .unwrap_or_default(),
        },
    }
}
unsafe fn create_space_velocity(raw: &MaybeUninit<sys::SpaceVelocity>) -> openxr::SpaceVelocity {
    // Applications *must* not read invalid velocities, i.e. they may be uninitialized
    let ptr = raw.as_ptr();
    let flags = *ptr::addr_of!((*ptr).velocity_flags);
    openxr::SpaceVelocity {
        velocity_flags: flags,
        linear_velocity: flags
            .contains(sys::SpaceVelocityFlags::LINEAR_VALID)
            .then(|| *ptr::addr_of!((*ptr).linear_velocity))
            .unwrap_or_default(),
        angular_velocity: flags
            .contains(sys::SpaceVelocityFlags::ANGULAR_VALID)
            .then(|| *ptr::addr_of!((*ptr).angular_velocity))
            .unwrap_or_default(),
    }
}
fn get_arr_init<T: Copy>(
    init: T,
    mut getter: impl FnMut(u32, &mut u32, *mut T) -> sys::Result,
) -> openxr::Result<Vec<T>> {
    let mut output = 0;
    cvt(getter(0, &mut output, std::ptr::null_mut()))?;
    let mut buffer = vec![init; output as usize];
    loop {
        match cvt(getter(output, &mut output, buffer.as_mut_ptr() as _)) {
            Ok(_) => {
                buffer.truncate(output as usize);
                return Ok(buffer);
            }
            Err(sys::Result::ERROR_SIZE_INSUFFICIENT) => {
                buffer.resize(output as usize, init);
            }
            Err(e) => {
                return Err(e);
            }
        }
    }
}
