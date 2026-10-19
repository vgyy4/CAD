//! `math_FunctionAllRoots`: the null intervals and isolated roots of a function on a sample.
#![allow(clippy::float_cmp)]

use super::{FunctionRoots, FunctionSample, FunctionWithDerivative, seq_get};
use crate::mathlin::MathError;

/// `math_FunctionAllRoots`.
#[derive(Debug, Clone)]
pub struct FunctionAllRoots {
    done: bool,
    pdeb: Vec<f64>,
    pfin: Vec<f64>,
    piso: Vec<f64>,
    ideb: Vec<i32>,
    ifin: Vec<i32>,
    iiso: Vec<i32>,
}

/// `Standard_NumericError_Raise_if` of an inner `FunctionRoots`: not done, all null, or (when `need_solution`) empty.
fn check_roots(r: &FunctionRoots, need_solution: bool) -> Result<(), MathError> {
    if !r.is_done() || r.is_all_null()? || (need_solution && r.nb_solutions()? == 0) {
        return Err(MathError::Domain);
    }
    Ok(())
}

impl FunctionAllRoots {
    /// The constructor. `Err(Domain)` where the C++ `Standard_NumericError_Raise_if` conditions hold
    /// (compiled out in OCCT 8.0.1 as built) and for a sample of fewer than 2 points.
    pub fn new(
        f: &mut dyn FunctionWithDerivative,
        s: &FunctionSample,
        eps_x: f64,
        eps_f: f64,
        eps_nul: f64,
    ) -> Result<Self, MathError> {
        if s.nb_points() < 2 {
            return Err(MathError::Domain);
        }
        let mut r = Self {
            done: false,
            pdeb: Vec::new(),
            pfin: Vec::new(),
            piso: Vec::new(),
            ideb: Vec::new(),
            ifin: Vec::new(),
            iiso: Vec::new(),
        };
        let par = |i: i32| s.parameter(i);

        let mut deb_nul = 0.0_f64;
        let mut fin_nul;
        let mut nuld = false;
        let mut nulf = false;
        let mut indd = 0;
        let mut indf;
        let mut valsav = 0.0_f64;
        let mut val = 0.0_f64;
        let mut valbid = 0.0_f64;
        let nbp = s.nb_points();

        if let Some(v) = f.value(par(1)) {
            val = v;
        }
        let mut pnul = val.abs() <= eps_nul;
        if !pnul {
            valsav = val;
        }
        let mut inter_nul = false;

        let mut i = 2;
        let mut fini = i > nbp;
        while !fini {
            if let Some(v) = f.value(par(i)) {
                val = v;
            }
            let nul = val.abs() <= eps_nul;
            if !nul {
                valsav = val;
            }
            if inter_nul && !nul {
                inter_nul = false;
                r.pdeb.push(deb_nul);
                r.ideb.push(indd);
                let mut cst = if val > 0.0 { eps_nul } else { -eps_nul };
                let res1 = FunctionRoots::new(f, par(i - 1), par(i), 10, eps_x, eps_f, 0.0, cst);
                check_roots(&res1, true)?;

                fin_nul = res1.value(1)?;
                indf = res1.state_number(1)?;

                cst = -cst;
                let res2 = FunctionRoots::new(f, par(i - 1), par(i), 10, eps_x, eps_f, 0.0, cst);
                check_roots(&res2, false)?;

                if res2.nb_solutions()? != 0 && res2.value(1)? < fin_nul {
                    fin_nul = res2.value(1)?;
                    indf = res2.state_number(1)?;
                }
                r.pfin.push(fin_nul);
                r.ifin.push(indf);
            } else if !inter_nul && pnul && nul {
                inter_nul = true;
                if i == 2 {
                    deb_nul = par(1);
                    if let Some(v) = f.value(deb_nul) {
                        valbid = v;
                    }
                    indd = f.state_number();
                    nuld = true;
                } else {
                    let mut cst = if valsav > 0.0 { eps_nul } else { -eps_nul };
                    let res1 = FunctionRoots::new(f, par(i - 2), par(i - 1), 10, eps_x, eps_f, 0.0, cst);
                    check_roots(&res1, true)?;
                    let n1 = res1.nb_solutions()?;
                    deb_nul = res1.value(n1)?;
                    indd = res1.state_number(n1)?;

                    cst = -cst;
                    let res3 = FunctionRoots::new(f, par(i - 2), par(i - 1), 10, eps_x, eps_f, 0.0, cst);
                    check_roots(&res3, false)?;

                    let n3 = res3.nb_solutions()?;
                    if n3 != 0 && res3.value(n3)? > deb_nul {
                        deb_nul = res3.value(n3)?;
                        indd = res3.state_number(n3)?;
                    }
                }
            }
            i += 1;
            pnul = nul;
            fini = i > nbp;
        }

        if inter_nul {
            // add the interval that ends at the last point
            r.pdeb.push(deb_nul);
            r.ideb.push(indd);
            fin_nul = par(nbp);
            if let Some(v) = f.value(fin_nul) {
                valbid = v;
            }
            indf = f.state_number();
            r.pfin.push(fin_nul);
            r.ifin.push(indf);
            nulf = true;
        }
        let _ = valbid;

        if r.pdeb.is_empty() {
            // no null interval
            let res = FunctionRoots::new(f, par(1), par(nbp), nbp, eps_x, eps_f, 0.0, 0.0);
            check_roots(&res, false)?;
            r.append_roots(&res)?;
        } else {
            let nbp_min = 3;
            let total = par(nbp) - par(1);
            if !nuld {
                // roots between the first parameter and the beginning of the first null interval
                let first = seq_get(&r.pdeb, 1)?;
                let nbrpt = (((first - par(1)) / total).abs() * f64::from(nbp)).trunc() as i32;
                let res = FunctionRoots::new(f, par(1), first, nbrpt.max(nbp_min), eps_x, eps_f, 0.0, 0.0);
                check_roots(&res, false)?;
                r.append_roots(&res)?;
            }
            let nb_int = i32::try_from(r.pdeb.len()).unwrap_or(i32::MAX);
            for k in 2..=nb_int {
                let deb_k = seq_get(&r.pdeb, k)?;
                let fin_prev = seq_get(&r.pfin, k - 1)?;
                let nbrpt = (((deb_k - fin_prev) / total).abs() * f64::from(nbp)).trunc() as i32;
                let res = FunctionRoots::new(f, fin_prev, deb_k, nbrpt.max(nbp_min), eps_x, eps_f, 0.0, 0.0);
                check_roots(&res, false)?;
                r.append_roots(&res)?;
            }
            if !nulf {
                // roots between the end of the last null interval and the last parameter
                let last_fin = seq_get(&r.pfin, nb_int)?;
                let nbrpt = (((par(nbp) - last_fin) / total).abs() * f64::from(nbp)).trunc() as i32;
                let res = FunctionRoots::new(f, last_fin, par(nbp), nbrpt.max(nbp_min), eps_x, eps_f, 0.0, 0.0);
                check_roots(&res, false)?;
                r.append_roots(&res)?;
            }
        }
        r.done = true;
        Ok(r)
    }

    fn append_roots(&mut self, res: &FunctionRoots) -> Result<(), MathError> {
        for j in 1..=res.nb_solutions()? {
            self.piso.push(res.value(j)?);
            self.iiso.push(res.state_number(j)?);
        }
        Ok(())
    }

    /// `IsDone`.
    pub fn is_done(&self) -> bool {
        self.done
    }
    fn check(&self) -> Result<(), MathError> {
        if self.done { Ok(()) } else { Err(MathError::NotDone) }
    }
    /// `NbIntervals`.
    pub fn nb_intervals(&self) -> Result<i32, MathError> {
        self.check()?;
        Ok(i32::try_from(self.pdeb.len()).unwrap_or(i32::MAX))
    }
    /// `GetInterval`.
    pub fn get_interval(&self, index: i32) -> Result<(f64, f64), MathError> {
        self.check()?;
        Ok((seq_get(&self.pdeb, index)?, seq_get(&self.pfin, index)?))
    }
    /// `GetIntervalState`: `(IFirst, ILast)`.
    pub fn get_interval_state(&self, index: i32) -> Result<(i32, i32), MathError> {
        self.check()?;
        Ok((seq_get(&self.ideb, index)?, seq_get(&self.ifin, index)?))
    }
    /// `NbPoints`.
    pub fn nb_points(&self) -> Result<i32, MathError> {
        self.check()?;
        Ok(i32::try_from(self.piso.len()).unwrap_or(i32::MAX))
    }
    /// `GetPoint`.
    pub fn get_point(&self, index: i32) -> Result<f64, MathError> {
        self.check()?;
        seq_get(&self.piso, index)
    }
    /// `GetPointState`.
    pub fn get_point_state(&self, index: i32) -> Result<i32, MathError> {
        self.check()?;
        seq_get(&self.iiso, index)
    }
    /// `Dump`.
    pub fn dump(&self) -> String {
        let mut s = String::from("math_FunctionAllRoots ");
        if self.done {
            s.push_str(" Status = Done \n");
            s.push_str(&format!(" Number of null intervals = {}\n", self.pdeb.len()));
            s.push_str(&format!(" Number of points where the function is null: {}\n", self.piso.len()));
        } else {
            s.push_str(" Status = not Done \n");
        }
        s
    }
}
