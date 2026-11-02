//! `AdvApp2Var_Network`: the grid of patches with its cutting parameters.

use super::{Patch, Result, Seq};

/// `AdvApp2Var_Network`
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Network {
    net: Seq<Patch>,
    u_parameters: Seq<f64>,
    v_parameters: Seq<f64>,
}

impl Network {
    /// `AdvApp2Var_Network::AdvApp2Var_Network()`
    pub fn new_0() -> Self {
        Self::default()
    }

    /// `FirstNotApprox(theIndex)`: the 1-based index of the first patch not approximated.
    pub fn first_not_approx(&self) -> Option<i32> {
        for (index, patch) in (1..).zip(self.net.iter()) {
            if !patch.is_approximated() {
                return Some(index);
            }
        }
        None
    }

    /// `UpdateInU(CuttingValue)`: splits every patch column at a U cutting value.
    pub fn update_in_u(&mut self, cutting_value: f64) -> Result<()> {
        let mut i: i32 = 1;
        while *self.u_parameters.value(i)? < cutting_value {
            i += 1;
        }
        self.u_parameters.insert_before(i, cutting_value)?;
        let nb_patch_in_u = self.u_parameters.len() - 1;
        for j in 1..self.v_parameters.len() {
            let patch_index = nb_patch_in_u * (j - 1) + i - 1;
            let pat = self.net.value_mut(patch_index)?;
            let (uo, vo) = (pat.u_order(), pat.v_order());
            let (v0, v1) = (pat.v0(), pat.v1());
            let u0 = pat.u0();
            pat.change_domain(u0, cutting_value, v0, v1);
            pat.reset_approx();
            let u_next = *self.u_parameters.value(i + 1)?;
            let v_j = *self.v_parameters.value(j)?;
            let v_next = *self.v_parameters.value(j + 1)?;
            let mut new_pat = Patch::new_6(cutting_value, u_next, v_j, v_next, uo, vo);
            new_pat.reset_approx();
            self.net.insert_after(patch_index, new_pat)?;
        }
        Ok(())
    }

    /// `UpdateInV(CuttingValue)`: splits every patch row at a V cutting value.
    pub fn update_in_v(&mut self, cutting_value: f64) -> Result<()> {
        let mut j: i32 = 1;
        while *self.v_parameters.value(j)? < cutting_value {
            j += 1;
        }
        self.v_parameters.insert_before(j, cutting_value)?;
        let nb_patch_in_u = self.u_parameters.len() - 1;
        for i in 1..=nb_patch_in_u {
            let patch_index = nb_patch_in_u * (j - 2) + i;
            let pat = self.net.value_mut(patch_index)?;
            let (u0, u1, v0) = (pat.u0(), pat.u1(), pat.v0());
            pat.change_domain(u0, u1, v0, cutting_value);
            pat.reset_approx();
        }
        for i in 1..=nb_patch_in_u {
            let patch_index = nb_patch_in_u * (j - 1) + i - 1;
            let (uo, vo) = {
                let source = self.net.value(nb_patch_in_u * (j - 2) + i)?;
                (source.u_order(), source.v_order())
            };
            let u_i = *self.u_parameters.value(i)?;
            let u_next = *self.u_parameters.value(i + 1)?;
            let v_next = *self.v_parameters.value(j + 1)?;
            let mut new_pat = Patch::new_6(u_i, u_next, cutting_value, v_next, uo, vo);
            new_pat.reset_approx();
            self.net.insert_after(patch_index, new_pat)?;
        }
        Ok(())
    }

    /// `SameDegree(iu, iv, ncfu, ncfv)`: the common coefficient counts, applied to all patches.
    pub fn same_degree(&mut self, iu: i32, iv: i32) -> (i32, i32) {
        let mut ncfu = 2 * iu + 2;
        let mut ncfv = 2 * iv + 2;
        for patch in self.net.iter() {
            ncfu = ncfu.max(patch.nb_coeff_in_u());
            ncfv = ncfv.max(patch.nb_coeff_in_v());
        }
        for patch in self.net.iter_mut() {
            patch.change_nb_coeff(ncfu, ncfv);
        }
        (ncfu, ncfv)
    }

    /// `NbPatch()`
    pub fn nb_patch(&self) -> i32 {
        self.net.len()
    }

    /// `NbPatchInU()`
    pub fn nb_patch_in_u(&self) -> i32 {
        self.u_parameters.len() - 1
    }

    /// `NbPatchInV()`
    pub fn nb_patch_in_v(&self) -> i32 {
        self.v_parameters.len() - 1
    }
}
