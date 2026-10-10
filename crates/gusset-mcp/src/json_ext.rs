//! Non-panicking JSON access (the workspace lints forbid indexing).

use serde_json::Value;

static NULL: Value = Value::Null;

/// `v.at("key")` / `v.idx(i)`: the member or element, or `null` when absent.
pub(crate) trait JsonExt {
    /// Object member `k`, or `null`.
    fn at(&self, k: &str) -> &Value;
    /// Array element `i`, or `null`.
    fn idx(&self, i: usize) -> &Value;
}

impl JsonExt for Value {
    fn at(&self, k: &str) -> &Value {
        self.get(k).unwrap_or(&NULL)
    }
    fn idx(&self, i: usize) -> &Value {
        self.get(i).unwrap_or(&NULL)
    }
}
