//! `TopLoc_SListOfItemLocation` and `TopLoc_SListNodeOfItemLocation`.
use super::ItemLocation;
use crate::mathlin::MathError;
use std::rc::Rc;

#[derive(Debug)]
struct Node {
    tail: SListOfItemLocation,
    value: ItemLocation,
}

/// `TopLoc_SListOfItemLocation`: a persistent singly linked list (`Clone` shares the nodes).
#[derive(Debug, Clone, Default)]
pub struct SListOfItemLocation {
    node: Option<Rc<Node>>,
}

impl SListOfItemLocation {
    /// `TopLoc_SListOfItemLocation()`.
    pub fn new() -> Self {
        Self::default()
    }
    /// `TopLoc_SListOfItemLocation(anItem, aTail)`: the node's transformation is the item's, pre-multiplied by the
    /// transformation of the tail's head when the tail is not empty.
    pub fn from_item_and_tail(item: ItemLocation, tail: &SListOfItemLocation) -> Self {
        let mut value = item;
        if let Some(n) = &tail.node {
            value.trsf.pre_multiply(&n.value.trsf);
        }
        Self { node: Some(Rc::new(Node { tail: tail.clone(), value })) }
    }
    /// `Construct(anItem)`.
    pub fn construct(&mut self, item: ItemLocation) -> Result<(), MathError> {
        *self = Self::from_item_and_tail(item, self);
        Ok(())
    }
    /// `IsEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.node.is_none()
    }
    /// `More()`.
    pub fn more(&self) -> bool {
        self.node.is_some()
    }
    /// `Clear()`.
    pub fn clear(&mut self) {
        self.node = None;
    }
    /// `Value()` (an empty list is `Err`).
    pub fn value(&self) -> Result<&ItemLocation, MathError> {
        self.node.as_ref().map(|n| &n.value).ok_or(MathError::Domain)
    }
    /// `Tail()` (the tail of the empty list is the empty list).
    pub fn tail(&self) -> SListOfItemLocation {
        match &self.node {
            Some(n) => n.tail.clone(),
            None => self.clone(),
        }
    }
    /// `ToTail()`.
    pub fn to_tail(&mut self) {
        *self = self.tail();
    }
    /// `Next()`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) {
        self.to_tail();
    }
}
