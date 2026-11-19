//! The tyre: the Magic Formula of Pacejka for the force along the wheel and across it, with
//! combined slip, the load's effect on grip and stiffness, camber thrust and rolling resistance.
//!
//! Sources (formulas only; every number of a tyre is in its car's file):
//! - H. B. Pacejka, *Tire and Vehicle Dynamics*, 3rd ed., Butterworth-Heinemann, 2012: ch. 4, the
//!   Magic Formula `D sin(C atan(Bx − E(Bx − atan Bx)))`, the cornering stiffness's dependence on load
//!   `pKy1 Fz0 sin(2 atan(Fz / (pKy2 Fz0)))`, the friction coefficient falling with load
//!   `μ (1 + pDy2 dfz)`, and the weighting functions of combined slip `G = cos(C atan(B s))`;
//!   ch. 7, the transient tyre with relaxation lengths and its damping at low speed.
//! - E. Bakker, L. Nyborg, H. B. Pacejka, *Tyre modelling for use in vehicle dynamics studies*, SAE
//!   870421, 1987: the shape factors C = 1.3 for the side force and 1.65 for the brake force that
//!   fit car tyres.
//!
//! Sign conventions: the longitudinal slip `kappa` is (ωR − Vx) / |Vx|, positive when the tyre
//! drives; the lateral slip `tan_alpha` is −Vy / |Vx|, positive when the contact patch slides to the
//! left, so a positive slip gives a force to the right. Camber is the lean of the wheel's top to the
//! right, radians.

use serde::Deserialize;

/// The numbers of a tyre (with its wheel), as its car's file gives them.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TyreFit {
    /// Rolling radius, m.
    pub radius: f64,
    /// Width of the tread, m (for drawing it).
    pub width: f64,
    /// Wheel, tyre, brake disc and hub turning together, kg m².
    pub inertia: f64,
    /// Radial stiffness, N/m.
    pub vertical_stiffness: f64,
    /// The load its numbers are given for (Fz0), N.
    pub nominal_load: f64,
    /// Peak friction across (μy) and along (μx) on dry asphalt at the nominal load.
    pub grip: f64,
    pub grip_longitudinal: f64,
    /// How the peak friction changes with load: μ (1 + this × dfz), dfz = (Fz − Fz0) / Fz0 (pDy2).
    pub load_sensitivity: f64,
    /// Cornering stiffness at its peak load, over the nominal load, per radian (pKy1), and the load
    /// it peaks at, over the nominal load (pKy2).
    pub cornering_stiffness: f64,
    pub cornering_peak: f64,
    /// Longitudinal slip stiffness over the load (∂Fx/∂κ / Fz).
    pub slip_stiffness: f64,
    /// Shape and curvature factors of the Magic Formula, across and along (C, E).
    pub shape_lateral: f64,
    pub shape_longitudinal: f64,
    pub curvature_lateral: f64,
    pub curvature_longitudinal: f64,
    /// Combined slip: how fast the slip angle takes away the force along (rBx1, rBx2), and the
    /// slip along the force across (rBy1, rBy2).
    pub combined_longitudinal: (f64, f64),
    pub combined_lateral: (f64, f64),
    /// Camber thrust over the load, per radian of lean.
    pub camber_stiffness: f64,
    /// Relaxation lengths, m: how far the tyre rolls to build its force, across and along.
    pub relaxation_lateral: f64,
    pub relaxation_longitudinal: f64,
    /// Rolling resistance's growth with speed: added to the surface's coefficient per (m/s)².
    pub rolling_speed: f64,
}

impl TyreFit {
    /// The forces along and across the wheel (N) at a load `fz` (N), slips `kappa` and `tan_alpha`,
    /// a lean of `camber` and a surface giving `grip` of the dry asphalt's friction.
    pub fn forces(&self, fz: f64, kappa: f64, tan_alpha: f64, camber: f64, grip: f64) -> (f64, f64) {
        self.forces_wet(fz, kappa, tan_alpha, camber, grip, 0.0)
    }

    /// `forces` on a road `wet` (0 dry to 1 wet through, `wet::wet_grip`): the curves' curvature
    /// lowered by `wet::WET_CURVATURE` times it, so that the force falls away faster past its peak.
    pub fn forces_wet(&self, fz: f64, kappa: f64, tan_alpha: f64, camber: f64, grip: f64, wet: f64) -> (f64, f64) {
        if fz <= 0.0 {
            return (0.0, 0.0);
        }
        let fz0 = self.nominal_load;
        let dfz = (fz - fz0) / fz0;
        let scale = (1.0 + self.load_sensitivity * dfz).max(0.4) * grip;
        let alpha = tan_alpha.atan();

        // Pure side slip, with the camber's thrust as a shift of the slip angle.
        let dy = self.grip * scale * fz;
        let ky = self.cornering_stiffness * fz0 * (2.0 * (fz / (self.cornering_peak * fz0)).atan()).sin();
        let cy = self.shape_lateral;
        let by = ky / (cy * dy).max(1.0);
        let shift = self.camber_stiffness * fz * camber / ky.max(1.0);
        let fy0 = magic(dy, cy, by, self.curvature_lateral - super::wet::WET_CURVATURE * wet, alpha + shift);

        // Pure longitudinal slip.
        let dx = self.grip_longitudinal * scale * fz;
        let kx = self.slip_stiffness * fz;
        let cx = self.shape_longitudinal;
        let bx = kx / (cx * dx).max(1.0);
        let fx0 = magic(dx, cx, bx, self.curvature_longitudinal - super::wet::WET_CURVATURE * wet, kappa);

        // Combined: each force weighted down by the other slip.
        let (rbx1, rbx2) = self.combined_longitudinal;
        let (rby1, rby2) = self.combined_lateral;
        let bxa = rbx1 * (rbx2 * kappa).atan().cos();
        let gxa = (bxa * alpha).atan().cos();
        let byk = rby1 * (rby2 * alpha).atan().cos();
        let gyk = (byk * kappa).atan().cos();
        (fx0 * gxa, fy0 * gyk)
    }

    /// The highest force along the wheel at a load on a surface: its friction there.
    pub fn peak_longitudinal(&self, fz: f64, grip: f64) -> f64 {
        let dfz = (fz - self.nominal_load) / self.nominal_load;
        self.grip_longitudinal * (1.0 + self.load_sensitivity * dfz).max(0.4) * grip * fz
    }
}

/// The Magic Formula: D sin(C atan(B x − E (B x − atan(B x)))).
fn magic(d: f64, c: f64, b: f64, e: f64, x: f64) -> f64 {
    let bx = b * x;
    d * (c * (bx - e * (bx - bx.atan())).atan()).sin()
}
