use std::{cell::RefCell, mem, ops::Deref, rc::Rc};

use bevy::{ecs::system::SystemId, prelude::*};
use bevy_mod_xr::session::{XrFirst, XrHandleEvents};
use openxr::{Event, EventDataBuffer};

pub struct OxrEventsPlugin;

impl Plugin for OxrEventsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OxrEventHandlers>();
        app.add_systems(
            XrFirst,
            poll_events
                .in_set(XrHandleEvents::Poll)
                .run_if(openxr_session_available),
        );
    }
}
/// Polls any OpenXR events and handles them accordingly
pub fn poll_events(world: &mut World) {
    let _span = debug_span!("xr_poll_events").entered();
    let instance = world.resource::<OxrInstance>().clone();
    let handlers = world.remove_resource::<OxrEventHandlers>().unwrap();
    let mut buffer = EventDataBuffer::default();
    while let Some(event) = instance
        .poll_event(&mut buffer)
        .expect("Failed to poll event")
    {
        let event = Rc::new(RefCell::new(Some(event)));
        for handler in handlers.handlers.iter() {
            if let Err(err) =
                world.run_system_with::<_, ()>(*handler, OxrEvent::new(event.clone()))
            {
                error!("error when running oxr event handler: {err}");
            };
        }
        event.deref().take();
    }
    world.insert_resource(handlers);
}

use super::{openxr_session_available, resources::OxrInstance};
#[derive(Resource, Debug, Default)]
pub struct OxrEventHandlers {
    pub handlers: Vec<OxrEventHandler>,
}
pub type OxrEventHandler = SystemId<In<OxrEvent>, ()>;

pub struct OxrEvent {
    event: Rc<RefCell<Option<Event<'static>>>>,
}

impl OxrEvent {
    pub(crate) fn new(event: Rc<RefCell<Option<Event<'_>>>>) -> Self {
        Self {
            #[allow(clippy::missing_transmute_annotations)]
            event: unsafe { mem::transmute(event) },
        }
    }
    /// always returns [Some] if called in a valid scope
    /// # Safety
    /// The event is only valid for the duration of the poll event callback,
    /// don't Store the [Event] anywhere!!
    #[allow(clippy::needless_lifetimes)]
    pub unsafe fn get<'a>(&'a self) -> Option<Event<'a>> {
        *self.event.borrow()
    }
}
pub trait OxrEventHandlerExt {
    fn add_oxr_event_handler<M>(
        &mut self,
        system: impl IntoSystem<In<OxrEvent>, (), M> + 'static,
    ) -> &mut Self;
}
impl OxrEventHandlerExt for App {
    fn add_oxr_event_handler<M>(
        &mut self,
        system: impl IntoSystem<In<OxrEvent>, (), M> + 'static,
    ) -> &mut Self {
        self.init_resource::<OxrEventHandlers>();
        let id = self.register_system(system);
        self.world_mut()
            .resource_mut::<OxrEventHandlers>()
            .handlers
            .push(id);
        self
    }
}
