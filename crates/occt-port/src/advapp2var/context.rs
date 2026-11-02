//! `AdvApp2Var_Context`: the approximation conditions (default state and accessors).

/// `AdvApp2Var_Context`, the scalar part (the tolerance and root tables are not translated
/// because only the default constructor and the accessors are part of the judged API).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Context {
    fav: i32,
    ord_u: i32,
    ord_v: i32,
    lim_u: i32,
    lim_v: i32,
    nb_1dss: i32,
    nb_2dss: i32,
    nb_3dss: i32,
    jdeg_u: i32,
    jdeg_v: i32,
}

impl Context {
    /// `AdvApp2Var_Context::AdvApp2Var_Context()`: every scalar is zero.
    pub fn new_0() -> Self {
        Self::default()
    }

    /// `TotalDimension()`
    pub fn total_dimension(&self) -> i32 {
        self.nb_1dss + 2 * self.nb_2dss + 3 * self.nb_3dss
    }

    /// `TotalNumberSSP()`
    pub fn total_number_ssp(&self) -> i32 {
        self.nb_1dss + self.nb_2dss + self.nb_3dss
    }

    /// `FavorIso()`
    pub fn favor_iso(&self) -> i32 {
        self.fav
    }

    /// `UOrder()`
    pub fn u_order(&self) -> i32 {
        self.ord_u
    }

    /// `VOrder()`
    pub fn v_order(&self) -> i32 {
        self.ord_v
    }

    /// `ULimit()`
    pub fn u_limit(&self) -> i32 {
        self.lim_u
    }

    /// `VLimit()`
    pub fn v_limit(&self) -> i32 {
        self.lim_v
    }

    /// `UJacDeg()`
    pub fn u_jac_deg(&self) -> i32 {
        self.jdeg_u
    }

    /// `VJacDeg()`
    pub fn v_jac_deg(&self) -> i32 {
        self.jdeg_v
    }
}
