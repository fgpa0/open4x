//! Building blocks for procedural low-poly models (plants, units): flat-shaded
//! primitives with per-vertex colour, merged into one mesh.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

/// Raw geometry with per-vertex colour, cheap to copy into merged meshes.
#[derive(Default, Clone)]
pub struct Model {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    colors: Vec<Vec4>,
    indices: Vec<u32>,
}

pub fn lin(r: u8, g: u8, b: u8) -> Vec4 {
    let c = Color::srgb_u8(r, g, b).to_linear();
    Vec4::new(c.red, c.green, c.blue, 1.0)
}

impl Model {
    pub fn from_parts(positions: Vec<Vec3>, normals: Vec<Vec3>, colors: Vec<Vec4>, indices: Vec<u32>) -> Self {
        Model { positions, normals, colors, indices }
    }

    /// Appends a primitive mesh, flat shaded, coloured and transformed.
    pub fn add(&mut self, mesh: Mesh, color: Vec4, transform: Transform) {
        let mut mesh = mesh.transformed_by(transform);
        mesh.duplicate_vertices();
        mesh.compute_flat_normals();
        let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return };
        let Some(VertexAttributeValues::Float32x3(nrm)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { return };
        let base = self.positions.len() as u32;
        self.positions.extend(pos.iter().map(|p| Vec3::from(*p)));
        self.normals.extend(nrm.iter().map(|n| Vec3::from(*n)));
        self.colors.extend(std::iter::repeat_n(color, pos.len()));
        self.indices.extend((0..pos.len() as u32).map(|i| base + i));
    }

    /// The highest point.
    pub fn top(&self) -> f32 {
        self.positions.iter().map(|p| p.y).fold(0.0, f32::max)
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Rewrites the alpha of the vertices from `start` on.
    pub fn map_alpha_from(&mut self, start: usize, f: impl Fn(f32) -> f32) {
        for c in &mut self.colors[start..] {
            c.w = f(c.w);
        }
    }

    /// Appends another model placed by `transform` and tinted.
    pub fn append(&mut self, other: &Model, transform: &Transform, tint: Vec4) {
        let base = self.positions.len() as u32;
        let m = transform.to_matrix();
        self.positions.extend(other.positions.iter().map(|p| m.transform_point3(*p)));
        // Normals scale inversely, for stretched models.
        self.normals.extend(other.normals.iter().map(|n| (transform.rotation * (*n / transform.scale)).normalize()));
        self.colors.extend(other.colors.iter().map(|c| *c * tint));
        self.indices.extend(other.indices.iter().map(|i| base + i));
    }

    /// Appends `other` placed by `transform`, triangle by triangle: `f` maps
    /// each vertex colour to the one to use, and a triangle whose first
    /// vertex maps to `None` is left out (props flag their surfaces in alpha).
    pub fn append_with(&mut self, other: &Model, transform: &Transform, f: impl Fn(Vec4) -> Option<Vec4>) {
        let m = transform.to_matrix();
        let normal = transform.rotation;
        for tri in other.indices.as_chunks::<3>().0 {
            if f(other.colors[tri[0] as usize]).is_none() {
                continue;
            }
            for &i in tri {
                let i = i as usize;
                self.indices.push(self.positions.len() as u32);
                self.positions.push(m.transform_point3(other.positions[i]));
                self.normals.push((normal * other.normals[i]).normalize());
                self.colors.push(f(other.colors[i]).unwrap_or(other.colors[i]));
            }
        }
    }

    pub fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.iter().map(|v| v.to_array()).collect::<Vec<_>>())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals.iter().map(|v| v.to_array()).collect::<Vec<_>>())
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors.iter().map(|v| v.to_array()).collect::<Vec<_>>())
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

pub fn at(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_xyz(x, y, z)
}

pub fn cone(r: f32, h: f32, res: u32) -> Mesh {
    Cone { radius: r, height: h }.mesh().resolution(res).build()
}

pub fn cylinder(r: f32, h: f32, res: u32) -> Mesh {
    Cylinder::new(r, h).mesh().resolution(res).build()
}

pub fn blob(r: f32, detail: u32) -> Mesh {
    Sphere::new(r).mesh().ico(detail).unwrap()
}
