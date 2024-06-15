use std::{mem::MaybeUninit, ptr, sync::Mutex};

use bevy::{prelude::*, utils::hashbrown::HashSet};
use bevy_xr::{
    session::{session_available, session_running},
    spaces::{
        XrDestroySpace, XrPrimaryReferenceSpace, XrReferenceSpace, XrSpace, XrSpatialTransform,
    },
    types::XrPose,
};
use openxr::{
    sys, HandJointLocation, HandJointLocations, HandJointVelocities, HandJointVelocity,
    ReferenceSpaceType, SpaceLocationFlags, HAND_JOINT_COUNT,
};

use crate::{
    helper_traits::{ToPosef, ToQuat, ToVec3},
    init::{OxrHandleEvents, OxrLast},
    resources::{OxrFrameState, OxrInstance, Pipelined},
    session::OxrSession,
};

pub struct OxrSpatialPlugin;
impl Plugin for OxrSpatialPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<XrDestroySpace>();
        app.add_systems(PreUpdate, update_spatial_transforms.run_if(session_running));
        app.add_systems(Startup, patch_destroy_space.run_if(session_available));
        app.add_systems(OxrLast, destroy_space_event.before(OxrHandleEvents));
    }
}

fn destroy_space_event(instance: Res<OxrInstance>, mut events: EventReader<XrDestroySpace>) {
    for space in events.read() {
        unsafe {
            (instance.fp().destroy_space)(space.as_raw_openxr_space());
        }
    }
}

pub static OXR_DO_NOT_CALL_DESTOY_SPACE_FOR: Mutex<Option<HashSet<u64>>> = Mutex::new(None);
pub static OXR_ORIGINAL_DESTOY_SPACE: Mutex<Option<openxr::sys::pfn::DestroySpace>> =
    Mutex::new(None);

fn patch_destroy_space(instance: ResMut<OxrInstance>) {
    OXR_DO_NOT_CALL_DESTOY_SPACE_FOR
        .lock()
        .unwrap()
        .replace(HashSet::new());
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
    if !OXR_DO_NOT_CALL_DESTOY_SPACE_FOR
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

fn update_spatial_transforms(
    session: Res<OxrSession>,
    default_ref_space: Res<XrPrimaryReferenceSpace>,
    pipelined: Option<Res<Pipelined>>,
    frame_state: Res<OxrFrameState>,
    mut query: Query<(
        &mut Transform,
        &XrSpatialTransform,
        Option<&XrReferenceSpace>,
    )>,
) {
    for (mut transform, spatial, ref_space) in &mut query {
        let ref_space = ref_space.unwrap_or(&default_ref_space);
        if let Ok(space_location) = session.locate_space(
            &spatial.space,
            ref_space,
            if pipelined.is_some() {
                openxr::Time::from_nanos(
                    frame_state.predicted_display_time.as_nanos()
                        + frame_state.predicted_display_period.as_nanos(),
                )
            } else {
                frame_state.predicted_display_time
            },
        ) {
            if space_location
                .location_flags
                .contains(SpaceLocationFlags::POSITION_VALID)
            {
                transform.translation = spatial
                    .offset
                    .transform_point(space_location.pose.position.to_vec3())
            }
            if space_location
                .location_flags
                .contains(SpaceLocationFlags::ORIENTATION_VALID)
            {
                transform.rotation =
                    spatial.offset.rotation * space_location.pose.orientation.to_quat();
            }
        }
    }
}

impl OxrSession {
    pub fn create_action_space<T: openxr::ActionTy>(
        &self,
        action: &openxr::Action<T>,
        subaction_path: openxr::Path,
        pose_in_space: XrPose,
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
pub fn destroy_space(instance: &openxr::Instance, space: sys::Space) -> openxr::Result<sys::Result> {
    let result = unsafe { (instance.fp().destroy_space)(space) };
    cvt(result)
}
impl OxrSession {
    pub fn destroy_space(&self, space: XrSpace) {
        let _ = destroy_space(self.instance(), space.as_raw_openxr_space());
    }
    pub fn destroy_openxr_space(&self, space: openxr::Space) {
        let _ = destroy_space(self.instance(), space.as_raw());
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
    fn as_raw_openxr_space(&self) -> sys::Space;
    fn from_raw_openxr_space(space: sys::Space) -> Self;
    fn from_openxr_space(space: openxr::Space) -> Self;
    /// # Safety
    /// Session has to be the session from which the space is from
    unsafe fn to_openxr_space<T>(self, session: &openxr::Session<T>) -> openxr::Space;
}

unsafe impl OxrSpaceExt for XrSpace {
    fn as_raw_openxr_space(&self) -> sys::Space {
        sys::Space::from_raw(self.as_raw())
    }

    fn from_raw_openxr_space(space: sys::Space) -> Self {
        let raw = space.into_raw();
        OXR_DO_NOT_CALL_DESTOY_SPACE_FOR
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

    unsafe fn to_openxr_space<T>(self, session: &openxr::Session<T>) -> openxr::Space {
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
