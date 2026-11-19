//! An aircraft's wreck: its mesh cut into the pieces `sim_core::wreck` breaks it into. The
//! game gives it to the world (`WorldRenderer::add_wreck_shape`), which draws the pieces where they
//! lie and the fire.

use crate::models::{AircraftModel, Moves};
use engine::MeshBuilder;
use sim_core::{PieceKind, Pieces};
use glam::DVec3;
use std::ops::Range;

/// The aircraft's mesh with its triangles sorted into the pieces it breaks into (`shape`): one range of
/// vertices per piece, in the order of `PieceKind::ALL`, and each vertex's surface (`AircraftModel::surface`).
/// Moving parts stay where they are at rest; the propeller blur is left out.
pub fn pieces(model: &AircraftModel, shape: &Pieces) -> (MeshBuilder, Vec<[f32; 3]>, [Range<u32>; 4]) {
    let mut lists: [(Vec<f32>, Vec<[f32; 3]>); 4] = Default::default();
    for part in model.parts.iter().filter(|p| !matches!(p.moves, Moves::Disc | Moves::GearUp)) {
        let data = &model.mesh.data[part.first as usize * 10..(part.first + part.count) as usize * 10];
        let surface = model.surface_of(part.first, part.count);
        for (triangle, corners) in data.as_chunks::<30>().0.iter().zip(surface.as_chunks::<3>().0) {
            let kind = shape.of_point(centre(triangle));
            let i = PieceKind::ALL.iter().position(|k| *k == kind).unwrap_or(0);
            lists[i].0.extend_from_slice(triangle);
            lists[i].1.extend_from_slice(corners);
        }
    }
    let mut mesh = MeshBuilder::new();
    let mut surface = Vec::new();
    let mut ranges: [Range<u32>; 4] = Default::default();
    for ((list, corners), range) in lists.iter().zip(ranges.iter_mut()) {
        let first = mesh.count();
        mesh.data.extend_from_slice(list);
        surface.extend_from_slice(corners);
        *range = first..mesh.count();
    }
    (mesh, surface, ranges)
}

/// Sorts the triangles of each part of the aircraft's mesh by the piece of `shape` they belong to
/// (keeping their order within it), so that each part can be drawn piece by piece: a wing torn
/// short with its cut, the rest whole. The ranges of vertices of each part's pieces, in the order of
/// `PieceKind::ALL`; the parts keep their own ranges, the vertices their surfaces.
pub fn regions(model: &mut AircraftModel, shape: &Pieces) -> Vec<[Range<u32>; 4]> {
    let AircraftModel { mesh, surface, parts, .. } = model;
    parts
        .iter()
        .map(|part| {
            let range = part.first as usize * 10..(part.first + part.count) as usize * 10;
            let corners = part.first as usize..(part.first + part.count) as usize;
            let mut lists: [(Vec<f32>, Vec<[f32; 3]>); 4] = Default::default();
            for (triangle, three) in mesh.data[range.clone()].as_chunks::<30>().0.iter().zip(surface[corners.clone()].as_chunks::<3>().0) {
                let i = PieceKind::ALL.iter().position(|k| *k == shape.of_point(centre(triangle))).unwrap_or(0);
                lists[i].0.extend_from_slice(triangle);
                lists[i].1.extend_from_slice(three);
            }
            let mut ranges: [Range<u32>; 4] = Default::default();
            let mut next = part.first;
            for ((list, _), r) in lists.iter().zip(ranges.iter_mut()) {
                *r = next..next + (list.len() / 10) as u32;
                next = r.end;
            }
            mesh.data[range].copy_from_slice(&lists.iter().flat_map(|l| l.0.iter().copied()).collect::<Vec<f32>>());
            surface[corners].copy_from_slice(&lists.iter().flat_map(|l| l.1.iter().copied()).collect::<Vec<[f32; 3]>>());
            ranges
        })
        .collect()
}

/// The middle of a triangle of 3 vertices of 10 floats (position first).
fn centre(triangle: &[f32; 30]) -> DVec3 {
    (0..3).fold(DVec3::ZERO, |sum, k| sum + DVec3::new(triangle[k * 10] as f64, triangle[k * 10 + 1] as f64, triangle[k * 10 + 2] as f64)) / 3.0
}


#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::AircraftKind;

    #[test]
    fn every_triangle_goes_to_one_piece_and_each_piece_has_some() {
        for (model, shape) in AircraftKind::all().map(|kind| (crate::kinds::model(kind).unwrap(), kind.spec().pieces)) {
            let (mesh, surface, ranges) = pieces(&model, shape);
            assert_eq!(surface.len() as u32, mesh.count());
            let disc: u32 = model.parts.iter().filter(|p| matches!(p.moves, Moves::Disc | Moves::GearUp)).map(|p| p.count).sum();
            assert_eq!(mesh.count(), model.mesh.count() - disc);
            for (range, kind) in ranges.iter().zip(PieceKind::ALL) {
                assert!(range.len() > 300, "{:?} {kind:?} has {} vertices", shape.kind, range.len());
            }
            assert_eq!(ranges[3].end, mesh.count());
        }
    }
    /// Each part, sorted piece by piece, keeps its range and its triangles, and the wings and tail
    /// are where the pieces say.
    #[test]
    fn regions_sort_each_part_without_losing_a_triangle() {
        for (mut model, shape) in AircraftKind::all().map(|kind| (crate::kinds::model(kind).unwrap(), kind.spec().pieces)) {
            let before = (model.mesh.data.clone(), model.surface.clone());
            let regions = regions(&mut model, shape);
            for (part, ranges) in model.parts.iter().zip(&regions) {
                assert_eq!((ranges[0].start, ranges[3].end), (part.first, part.first + part.count));
                for (range, kind) in ranges.iter().zip(PieceKind::ALL) {
                    for triangle in model.mesh.data[range.start as usize * 10..range.end as usize * 10].as_chunks::<30>().0 {
                        assert_eq!(shape.of_point(centre(triangle)), kind);
                    }
                }
            }
            // Each triangle keeps its corners' surfaces (texture coordinates and material).
            let sorted = |data: &[f32], surface: &[[f32; 3]]| {
                let mut list: Vec<Vec<u32>> = data.as_chunks::<30>().0.iter().zip(surface.as_chunks::<3>().0).map(|(t, s)| t.iter().chain(s.iter().flatten()).map(|f| f.to_bits()).collect()).collect();
                list.sort();
                list
            };
            assert_eq!(sorted(&before.0, &before.1), sorted(&model.mesh.data, &model.surface));
            assert!(regions.iter().any(|r| !r[1].is_empty()) && regions.iter().any(|r| !r[2].is_empty()));
        }
    }
}
