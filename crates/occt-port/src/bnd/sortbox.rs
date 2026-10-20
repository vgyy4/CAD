//! `Bnd_BoundSortBox`: a voxel grid that finds the boxes of a set intersecting a given box.
use super::box3d::BndBox;
use crate::{gp::Pln, mathlin::MathError};

/// `getBnd_VoxelGridResolution`.
fn voxel_grid_resolution(boxes_count: i32) -> i32 {
    if boxes_count > 40000 {
        128
    } else if boxes_count > 10000 {
        64
    } else if boxes_count > 1000 {
        32
    } else if boxes_count > 100 {
        16
    } else {
        8
    }
}

/// `Bnd_VoxelGrid`: for every axis, the box indices of each slice.
#[derive(Debug, Clone, Default)]
struct VoxelGrid {
    slices_x: Vec<Vec<i32>>,
    slices_y: Vec<Vec<i32>>,
    slices_z: Vec<Vec<i32>>,
}

impl VoxelGrid {
    fn new(resolution: i32) -> Self {
        let n = usize::try_from(resolution).unwrap_or(0);
        Self { slices_x: vec![Vec::new(); n], slices_y: vec![Vec::new(); n], slices_z: vec![Vec::new(); n] }
    }
    fn append(slices: &mut [Vec<i32>], min: i32, max: i32, box_index: i32) -> Result<(), MathError> {
        for i in min..=max {
            let idx = usize::try_from(i).map_err(|_| MathError::Range)?;
            slices.get_mut(idx).ok_or(MathError::Range)?.push(box_index);
        }
        Ok(())
    }
    /// `AddBox`.
    fn add_box(&mut self, box_index: i32, voxel_box: &[i32; 6]) -> Result<(), MathError> {
        Self::append(&mut self.slices_x, voxel_box[0], voxel_box[3], box_index)?;
        Self::append(&mut self.slices_y, voxel_box[1], voxel_box[4], box_index)?;
        Self::append(&mut self.slices_z, voxel_box[2], voxel_box[5], box_index)
    }
    fn slice(slices: &[Vec<i32>], voxel_index: i32) -> Option<&Vec<i32>> {
        let slice = slices.get(usize::try_from(voxel_index).ok()?)?;
        if slice.is_empty() { None } else { Some(slice) }
    }
}

/// `Bnd_BoundSortBox`.
#[derive(Debug, Clone, Default)]
pub struct BndBoundSortBox {
    enclosing_box: BndBox,
    /// The sorted boxes, `None` before an `Initialize`.
    boxes: Option<Vec<BndBox>>,
    /// C++ index of the first box.
    lower: i32,
    coeff_x: f64,
    coeff_y: f64,
    coeff_z: f64,
    resolution: i32,
    large_boxes: Vec<i32>,
    voxel_grid: Option<VoxelGrid>,
}

impl BndBoundSortBox {
    /// `Bnd_BoundSortBox()`.
    pub fn new() -> Self {
        Self::default()
    }

    fn length(&self) -> i32 {
        self.boxes.as_ref().map_or(0, |b| i32::try_from(b.len()).unwrap_or(i32::MAX))
    }

    /// `myBoxes->Value(theIndex)`.
    fn box_at(&self, index: i32) -> Option<&BndBox> {
        let offset = usize::try_from(index.checked_sub(self.lower)?).ok()?;
        self.boxes.as_ref()?.get(offset)
    }

    /// `Initialize(theSetOfBoxes)`: `boxes[k]` has the C++ index `lower + k`.
    pub fn initialize_boxes(&mut self, boxes: &[BndBox], lower: i32) {
        self.boxes = Some(boxes.to_vec());
        self.lower = lower;
        for b in boxes {
            if !b.is_void() {
                self.enclosing_box.add_box(b);
            }
        }
        self.resolution = voxel_grid_resolution(self.length());
        if self.enclosing_box.is_void() {
            return;
        }
        // The grid below cannot fail: every voxel index is clamped to the resolution.
        let _ = self.calculate_coefficients();
        self.reset_voxel_grid();
        let _ = self.sort_boxes();
    }

    /// `Initialize(theEnclosingBox, theSetOfBoxes)`.
    pub fn initialize_enclosing_boxes(&mut self, enclosing: &BndBox, boxes: &[BndBox], lower: i32) {
        self.boxes = Some(boxes.to_vec());
        self.lower = lower;
        self.enclosing_box = enclosing.clone();
        self.resolution = voxel_grid_resolution(self.length());
        if self.enclosing_box.is_void() {
            return;
        }
        let _ = self.calculate_coefficients();
        self.reset_voxel_grid();
        let _ = self.sort_boxes();
    }

    /// `Initialize(theEnclosingBox, theNbBoxes)`.
    pub fn initialize_enclosing_count(&mut self, enclosing: &BndBox, nb_boxes: i32) -> Result<(), MathError> {
        if nb_boxes <= 0 {
            return Err(MathError::Domain);
        }
        let n = usize::try_from(nb_boxes).map_err(|_| MathError::Domain)?;
        self.boxes = Some(vec![BndBox::new(); n]);
        self.lower = 1;
        self.enclosing_box = enclosing.clone();
        self.resolution = voxel_grid_resolution(nb_boxes);
        if self.enclosing_box.is_void() {
            return Ok(());
        }
        self.calculate_coefficients()?;
        self.reset_voxel_grid();
        Ok(())
    }

    /// `Add(theBox, theIndex)`.
    pub fn add(&mut self, b: &BndBox, index: i32) -> Result<(), MathError> {
        let current_void = self.box_at(index).ok_or(MathError::Range)?.is_void();
        if !current_void {
            return Err(MathError::Domain);
        }
        if b.is_void() {
            return Ok(());
        }
        let offset = usize::try_from(index - self.lower).map_err(|_| MathError::Range)?;
        *self.boxes.as_mut().and_then(|v| v.get_mut(offset)).ok_or(MathError::Range)? = b.clone();
        self.add_box(b, index)
    }

    /// `Compare(const Bnd_Box&)`.
    pub fn compare_box(&mut self, b: &BndBox) -> Vec<i32> {
        let mut last_result: Vec<i32> = Vec::new();
        if b.is_void() || b.is_out_box(&self.enclosing_box) {
            return last_result;
        }
        for &box_index in &self.large_boxes {
            if self.box_at(box_index).is_some_and(|a_box| !a_box.is_out_box(b)) {
                last_result.push(box_index);
            }
        }
        let Ok([min_x, min_y, min_z, max_x, max_y, max_z]) = self.bounding_voxels(b) else {
            return last_result;
        };
        let Some(grid) = self.voxel_grid.as_ref() else {
            return last_result;
        };
        let upper = self.lower.saturating_add(self.length()).saturating_sub(1);
        let size = usize::try_from(upper.saturating_add(1)).unwrap_or(0);
        let mut result_indices = vec![0u8; size];
        const OCCUPIED_X: u8 = 0b01;
        const OCCUPIED_Y: u8 = 0b10;
        const OCCUPIED_XY: u8 = 0b11;
        for voxel_x in min_x..=max_x {
            if let Some(indices) = VoxelGrid::slice(&grid.slices_x, voxel_x) {
                for &box_index in indices {
                    if let Some(r) = usize::try_from(box_index).ok().and_then(|i| result_indices.get_mut(i)) {
                        *r |= OCCUPIED_X;
                    }
                }
            }
        }
        for voxel_y in min_y..=max_y {
            if let Some(indices) = VoxelGrid::slice(&grid.slices_y, voxel_y) {
                for &box_index in indices {
                    if let Some(r) = usize::try_from(box_index).ok().and_then(|i| result_indices.get_mut(i)) {
                        *r |= OCCUPIED_Y;
                    }
                }
            }
        }
        for voxel_z in min_z..=max_z {
            if let Some(indices) = VoxelGrid::slice(&grid.slices_z, voxel_z) {
                for &box_index in indices {
                    let Some(r) = usize::try_from(box_index).ok().and_then(|i| result_indices.get_mut(i)) else {
                        continue;
                    };
                    if *r == OCCUPIED_XY {
                        *r = 0;
                        if self.box_at(box_index).is_some_and(|x| !x.is_out_box(b)) {
                            last_result.push(box_index);
                        }
                    }
                }
            }
        }
        last_result
    }

    /// `Compare(const gp_Pln&)`.
    pub fn compare_plane(&mut self, p: &Pln) -> Vec<i32> {
        let mut last_result: Vec<i32> = Vec::new();
        if let Some(boxes) = self.boxes.as_ref() {
            for (k, b) in boxes.iter().enumerate() {
                if !b.is_out_plane(p) {
                    last_result.push(self.lower.wrapping_add(i32::try_from(k).unwrap_or(i32::MAX)));
                }
            }
        }
        last_result
    }

    fn calculate_coefficients(&mut self) -> Result<(), MathError> {
        let [xmin, ymin, zmin, xmax, ymax, zmax] = self.enclosing_box.get()?;
        let res = f64::from(self.resolution);
        self.coeff_x = if xmax - xmin == 0.0 { 0.0 } else { res / (xmax - xmin) };
        self.coeff_y = if ymax - ymin == 0.0 { 0.0 } else { res / (ymax - ymin) };
        self.coeff_z = if zmax - zmin == 0.0 { 0.0 } else { res / (zmax - zmin) };
        Ok(())
    }

    fn reset_voxel_grid(&mut self) {
        self.voxel_grid = Some(VoxelGrid::new(self.resolution));
        self.large_boxes.clear();
    }

    fn sort_boxes(&mut self) -> Result<(), MathError> {
        let boxes = self.boxes.clone().unwrap_or_default();
        for (k, b) in boxes.iter().enumerate() {
            let index = self.lower.wrapping_add(i32::try_from(k).map_err(|_| MathError::Range)?);
            self.add_box(b, index)?;
        }
        Ok(())
    }

    /// `getBoundingVoxels`.
    fn bounding_voxels(&self, b: &BndBox) -> Result<[i32; 6], MathError> {
        let grid_start = self.enclosing_box.corner_min()?;
        let [xmin, ymin, zmin, xmax, ymax, zmax] = b.get()?;
        let last = self.resolution - 1;
        let lo = |v: f64, start: f64, coeff: f64| (((v - start) * coeff) as i32).saturating_sub(1).clamp(0, last);
        let hi = |v: f64, start: f64, coeff: f64| (((v - start) * coeff) as i32).saturating_add(1).clamp(0, last);
        Ok([
            lo(xmin, grid_start.x(), self.coeff_x),
            lo(ymin, grid_start.y(), self.coeff_y),
            lo(zmin, grid_start.z(), self.coeff_z),
            hi(xmax, grid_start.x(), self.coeff_x),
            hi(ymax, grid_start.y(), self.coeff_y),
            hi(zmax, grid_start.z(), self.coeff_z),
        ])
    }

    /// `addBox`.
    fn add_box(&mut self, b: &BndBox, index: i32) -> Result<(), MathError> {
        if b.is_void() {
            return Ok(());
        }
        let voxels = self.bounding_voxels(b)?;
        let [min_x, min_y, min_z, max_x, max_y, max_z] = voxels;
        let box_min_side = (max_x - min_x).min(max_y - min_y).min(max_z - min_z);
        if box_min_side * 4 > self.resolution {
            self.large_boxes.push(index);
        } else {
            self.voxel_grid.as_mut().ok_or(MathError::NotDone)?.add_box(index, &voxels)?;
        }
        Ok(())
    }
}
