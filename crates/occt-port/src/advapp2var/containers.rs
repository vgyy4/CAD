//! Storage with the OCCT access rules (checks enabled): `NCollection_Array1/2` index a flat
//! storage and raise `Standard_OutOfRange` when the position is outside it;
//! `NCollection_Sequence` is 1-based and raises when an index is out of range.

use super::{Error, Result};

/// `NCollection_Array1<T>` with bounds `[lower, upper]`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Arr1<T> {
    lower: i32,
    data: Vec<T>,
}

impl<T: Clone> Arr1<T> {
    fn pos(&self, i: i32) -> Result<usize> {
        usize::try_from(i64::from(i) - i64::from(self.lower))
            .ok()
            .filter(|&p| p < self.data.len())
            .ok_or(Error::OutOfRange)
    }

    /// `Value(i)`
    pub(crate) fn get(&self, i: i32) -> Result<&T> {
        let p = self.pos(i)?;
        self.data.get(p).ok_or(Error::OutOfRange)
    }

    /// `ChangeValue(i)`
    pub(crate) fn get_mut(&mut self, i: i32) -> Result<&mut T> {
        let p = self.pos(i)?;
        self.data.get_mut(p).ok_or(Error::OutOfRange)
    }

    /// `SetValue(i, v)`
    pub(crate) fn set(&mut self, i: i32, v: T) -> Result<()> {
        *self.get_mut(i)? = v;
        Ok(())
    }
}

/// `NCollection_Array2<T>` with row bounds `[row_lower, row_upper]` and column bounds
/// `[col_lower, col_upper]`. The position is `(row - row_lower) * cols + (col - col_lower)`
/// and only that flat position is checked, as in `NCollection_Array2::Value`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Arr2<T> {
    row_lower: i32,
    col_lower: i32,
    cols: i64,
    data: Vec<T>,
}

impl<T: Clone> Arr2<T> {
    /// `NCollection_Array2(r0, r1, c0, c1)` with every item set to `init`.
    pub(crate) fn new(r0: i32, r1: i32, c0: i32, c1: i32, init: T) -> Self {
        let rows = i64::from(r1) - i64::from(r0) + 1;
        let cols = i64::from(c1) - i64::from(c0) + 1;
        let n = usize::try_from(rows.saturating_mul(cols)).unwrap_or(0);
        Self { row_lower: r0, col_lower: c0, cols, data: vec![init; n] }
    }

    fn pos(&self, r: i32, c: i32) -> Result<usize> {
        let p = (i64::from(r) - i64::from(self.row_lower)) * self.cols + (i64::from(c) - i64::from(self.col_lower));
        usize::try_from(p).ok().filter(|&p| p < self.data.len()).ok_or(Error::OutOfRange)
    }

    /// `Value(r, c)`
    pub(crate) fn get(&self, r: i32, c: i32) -> Result<&T> {
        let p = self.pos(r, c)?;
        self.data.get(p).ok_or(Error::OutOfRange)
    }

    /// `ChangeValue(r, c)`
    pub(crate) fn get_mut(&mut self, r: i32, c: i32) -> Result<&mut T> {
        let p = self.pos(r, c)?;
        self.data.get_mut(p).ok_or(Error::OutOfRange)
    }

    /// `SetValue(r, c, v)`
    pub(crate) fn set(&mut self, r: i32, c: i32, v: T) -> Result<()> {
        *self.get_mut(r, c)? = v;
        Ok(())
    }
}

/// `NCollection_Sequence<T>`: 1-based items.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Seq<T> {
    items: Vec<T>,
}

impl<T> Default for Seq<T> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl<T> Seq<T> {
    /// `Length()`
    pub(crate) fn len(&self) -> i32 {
        i32::try_from(self.items.len()).unwrap_or(i32::MAX)
    }

    fn pos(&self, i: i32) -> Result<usize> {
        usize::try_from(i).ok().filter(|&i| i >= 1 && i <= self.items.len()).map(|i| i - 1).ok_or(Error::OutOfRange)
    }

    /// `Value(i)`
    pub(crate) fn value(&self, i: i32) -> Result<&T> {
        let p = self.pos(i)?;
        self.items.get(p).ok_or(Error::OutOfRange)
    }

    /// `ChangeValue(i)`
    pub(crate) fn value_mut(&mut self, i: i32) -> Result<&mut T> {
        let p = self.pos(i)?;
        self.items.get_mut(p).ok_or(Error::OutOfRange)
    }

    /// `First()`
    pub(crate) fn first(&self) -> Result<&T> {
        self.items.first().ok_or(Error::NoSuchObject)
    }

    /// `Last()`
    pub(crate) fn last(&self) -> Result<&T> {
        self.items.last().ok_or(Error::NoSuchObject)
    }

    /// `Append(v)`
    pub(crate) fn append(&mut self, v: T) {
        self.items.push(v);
    }

    /// `InsertAfter(i, items)`: the items go after position `i` (`0 <= i <= Length()`).
    pub(crate) fn insert_all_after(&mut self, i: i32, items: Vec<T>) -> Result<()> {
        let at = usize::try_from(i).ok().filter(|&a| a <= self.items.len()).ok_or(Error::OutOfRange)?;
        self.items.splice(at..at, items);
        Ok(())
    }

    /// `InsertAfter(i, v)`
    pub(crate) fn insert_after(&mut self, i: i32, v: T) -> Result<()> {
        self.insert_all_after(i, vec![v])
    }

    /// `InsertBefore(i, v)`: `1 <= i <= Length() + 1`.
    pub(crate) fn insert_before(&mut self, i: i32, v: T) -> Result<()> {
        if i < 1 {
            return Err(Error::OutOfRange);
        }
        self.insert_after(i - 1, v)
    }

    /// Items in order (`Iterator`).
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, T> {
        self.items.iter()
    }

    /// Items in order, mutable.
    pub(crate) fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.items.iter_mut()
    }
}
