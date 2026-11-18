//! Guest listeners sharing the ordinary hook export and wire arena.
use crate::PointId;
/// The bit that marks a point listener's id.
const POINT_TAG: u64 = mod_surface::call::POINT_TAG;
use mod_api::__private::points::{ApiKind, SlotListener};
use std::cell::RefCell;

thread_local! {
    static LISTENERS: RefCell<Vec<(PointId, String, *const ())>> = const { RefCell::new(Vec::new()) };
}

/// Installs a typed API listener at a game point in this guest and enables the generated export
/// table: the host registers the subscribed places in its ordinary ordered listener list.
pub fn register(listener: &SlotListener) -> Result<(), crate::RegisterError> {
    if let Some(ApiKind::Typed(function)) = listener.api_kind() {
        let place = PointId::from_index(listener.place().index())
            .ok_or(crate::RegisterError::Unsupported)?;
        LISTENERS.with(|slots| {
            slots
                .borrow_mut()
                .push((place, listener.injection().to_owned(), function))
        });
        mod_api::wire::guest::extend(POINT_TAG, POINT_TAG, crate::guest_dispatch);
        Ok(())
    } else {
        Err(crate::RegisterError::Unsupported)
    }
}

/// The places this guest listens at, by name, in registration order.
pub fn subscriptions() -> Vec<(String, String)> {
    LISTENERS.with(|slots| {
        slots
            .borrow()
            .iter()
            .map(|(place, injection, _)| (place.name().to_owned(), injection.clone()))
            .collect()
    })
}

pub(crate) fn fire(
    place: PointId,
    index: u32,
    call: impl FnOnce(*const ()),
) -> Result<(), mod_api::wire::WireError> {
    let function = LISTENERS.with(|slots| {
        slots
            .borrow()
            .get(index as usize)
            .filter(|(at, _, _)| *at == place)
            .map(|(_, _, function)| *function)
    });
    let function = function.ok_or(mod_api::wire::WireError::Invalid("unknown point listener"))?;
    call(function);
    Ok(())
}
