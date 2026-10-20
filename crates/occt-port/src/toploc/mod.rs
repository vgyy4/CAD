//! `TopLoc` of OCCT 8.0.1: `TopLoc_Datum3D`, `TopLoc_ItemLocation`, `TopLoc_SListOfItemLocation` and
//! `TopLoc_Location` (a persistent list of datum powers; datums are compared by identity, `Rc::ptr_eq`).
mod datum3d;
mod item;
mod location;
mod slist;

pub use datum3d::Datum3D;
pub use item::ItemLocation;
pub use location::Location;
pub use slist::SListOfItemLocation;
