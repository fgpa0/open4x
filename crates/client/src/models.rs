//! Models built in Blender from the scripts in `art/` (`art/build.py`),
//! embedded as .glb files (`build.rs` lists them).
//!
//! Units come with baked textures (colour with ambient occlusion,
//! metallic-roughness), a team mask in the vertex colour's red channel and
//! usually a rig: joints (pivots in the model's rest pose) and three bone
//! weights per vertex, which `units.rs` turns into a skinned mesh and
//! `rigs.rs` animates. Props come in libraries of named nodes with their
//! colour in the vertex colours and a flag in alpha (see `Flag`).

use crate::procmesh::Model;
use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageSamplerDescriptor, ImageType};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::OnceLock;

const MODELS: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/models.rs"));

/// What a prop vertex's alpha says about its surface.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flag {
    Bare,
    Foliage,
    Team,
    Window,
}

impl Flag {
    pub fn of(alpha: f32) -> Flag {
        match alpha {
            a if a < 0.16 => Flag::Foliage,
            a if a < 0.5 => Flag::Window,
            a if a < 0.83 => Flag::Team,
            _ => Flag::Bare,
        }
    }
}

/// One mesh of a model.
#[derive(Default, Clone)]
pub struct Geometry {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    joints: Vec<[u16; 4]>,
    weights: Vec<[f32; 4]>,
}

/// A bone of a rig: where it pivots in the rest pose (model space).
#[derive(Clone, Debug)]
pub struct Joint {
    pub name: String,
    pub parent: Option<usize>,
    pub pivot: Vec3,
}

/// A model's skeleton and how its unit attacks (for the animator).
#[derive(Clone, Debug, Default)]
pub struct Rig {
    pub style: String,
    pub joints: Vec<Joint>,
}

/// A model as exported: its meshes by node name, rig and textures.
pub struct Baked {
    pub nodes: Vec<(String, Geometry)>,
    pub rig: Option<Rig>,
    pub albedo: Option<Image>,
    pub metal_rough: Option<Image>,
}

/// The embedded model called `name`, if there is one.
pub fn baked(name: &str) -> Option<Baked> {
    let (_, bytes) = MODELS.iter().find(|(n, _)| *n == name)?;
    Some(parse(bytes).unwrap_or_else(|e| panic!("model {name}: {e}")))
}

/// A prop from the prop libraries (every untextured model), by node name.
pub fn prop(name: &str) -> &'static Model {
    try_prop(name).unwrap_or_else(|| panic!("no prop called {name}"))
}

pub fn try_prop(name: &str) -> Option<&'static Model> {
    static PROPS: OnceLock<HashMap<String, Model>> = OnceLock::new();
    PROPS
        .get_or_init(|| {
            let mut out = HashMap::new();
            for (file, bytes) in MODELS {
                let b = parse(bytes).unwrap_or_else(|e| panic!("model {file}: {e}"));
                if b.albedo.is_some() {
                    continue;
                }
                for (node, g) in b.nodes {
                    out.insert(node, g.model());
                }
            }
            out
        })
        .get(name)
}

fn parse(bytes: &[u8]) -> Result<Baked, gltf::Error> {
    let gltf = gltf::Gltf::from_slice(bytes)?;
    let blob = gltf.blob.as_deref().unwrap_or_default();
    let mut b = Baked { nodes: vec![], rig: None, albedo: None, metal_rough: None };
    for node in gltf.nodes() {
        let Some(mesh) = node.mesh() else { continue };
        if let Some(rig) = node.extras().as_ref().and_then(|e| parse_rig(e.get())) {
            b.rig = Some(rig);
        }
        let m = Mat4::from_cols_array_2d(&node.transform().matrix());
        let mut g = Geometry::default();
        for prim in mesh.primitives() {
            let reader = prim.reader(|_| Some(blob));
            let base = g.positions.len() as u32;
            let pos: Vec<[f32; 3]> = reader.read_positions().into_iter().flatten().collect();
            let n = pos.len();
            g.positions.extend(pos.iter().map(|p| m.transform_point3(Vec3::from(*p)).to_array()));
            g.normals.extend(reader.read_normals().into_iter().flatten().map(|v| m.transform_vector3(Vec3::from(v)).normalize().to_array()));
            let uvs: Vec<[f32; 2]> = reader.read_tex_coords(0).map(|t| t.into_f32().collect()).unwrap_or_default();
            g.uvs.extend(if uvs.len() == n { uvs } else { vec![[0.0; 2]; n] });
            let colors: Vec<[f32; 4]> = reader.read_colors(0).map(|c| c.into_rgba_f32().collect()).unwrap_or_default();
            g.colors.extend(if colors.len() == n { colors } else { vec![[1.0; 4]; n] });
            let custom = |name: &str| {
                let accessor = prim.attributes().find(|(s, _)| matches!(s, gltf::Semantic::Extras(x) if x.eq_ignore_ascii_case(name)))?.1;
                gltf::accessor::Iter::<[f32; 3]>::new(accessor, |_| Some(blob)).map(|i| i.collect::<Vec<_>>())
            };
            if let (Some(j), Some(w)) = (custom("JOINTS"), custom("WEIGHTS")) {
                g.joints.extend(j.iter().map(|j| [j[0] as u16, j[1] as u16, j[2] as u16, 0]));
                g.weights.extend(w.iter().map(|w| [w[0], w[1], w[2], 0.0]));
            }
            match reader.read_indices() {
                Some(i) => g.indices.extend(i.into_u32().map(|i| base + i)),
                None => g.indices.extend(base..base + n as u32),
            }
            let pbr = prim.material().pbr_metallic_roughness();
            let image = |t: Option<gltf::texture::Texture>, srgb: bool| {
                let gltf::image::Source::View { view, .. } = t?.source().source() else { return None };
                let data = &blob[view.offset()..view.offset() + view.length()];
                let image = Image::from_buffer(data, ImageType::Extension("png"), CompressedImageFormats::NONE, srgb, ImageSampler::Default, RenderAssetUsages::RENDER_WORLD).ok()?;
                Some(mipmapped(image, srgb))
            };
            b.albedo = b.albedo.or_else(|| image(pbr.base_color_texture().map(|t| t.texture()), true));
            b.metal_rough = b.metal_rough.or_else(|| image(pbr.metallic_roughness_texture().map(|t| t.texture()), false));
        }
        b.nodes.push((node.name().unwrap_or_default().to_string(), g));
    }
    Ok(b)
}

/// Reads the rig from a node's extras: `{"rig": "style;name parent x y z;..."}`.
fn parse_rig(json: &str) -> Option<Rig> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let text = value.get("rig")?.as_str()?;
    let mut parts = text.split(';');
    let style = parts.next()?.to_string();
    let joints = parts
        .map(|j| {
            let f: Vec<&str> = j.split_whitespace().collect();
            let num = |i: usize| f.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
            let parent = f.get(1).and_then(|v| v.parse::<i32>().ok()).filter(|p| *p >= 0).map(|p| p as usize);
            Joint { name: f.first().copied().unwrap_or_default().to_string(), parent, pivot: Vec3::new(num(2), num(3), num(4)) }
        })
        .collect();
    Some(Rig { style, joints })
}

/// Adds a box-filtered mip chain (the models are seen small, and textures
/// without mips shimmer), averaging sRGB colour in linear light.
fn mipmapped(mut image: Image, srgb: bool) -> Image {
    let (mut w, mut h) = (image.width() as usize, image.height() as usize);
    let Some(mut level) = image.data.take() else { return image };
    if level.len() != w * h * 4 {
        image.data = Some(level);
        return image;
    }
    let to_lin: Vec<f32> = (0..256).map(|v| if srgb { Color::srgb_u8(v as u8, 0, 0).to_linear().red } else { v as f32 / 255.0 }).collect();
    let from_lin = |v: f32| -> u8 {
        let v = v.clamp(0.0, 1.0);
        let s = if srgb { Color::LinearRgba(LinearRgba::rgb(v, 0.0, 0.0)).to_srgba().red } else { v };
        (s * 255.0).round() as u8
    };
    let mut data = level.clone();
    let mut count = 1;
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0.0;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
                        let v = level[(sy * w + sx) * 4 + c];
                        sum += if c < 3 { to_lin[v as usize] } else { v as f32 / 255.0 };
                    }
                    next[(y * nw + x) * 4 + c] = if c < 3 { from_lin(sum / 4.0) } else { (sum / 4.0 * 255.0).round() as u8 };
                }
            }
        }
        data.extend_from_slice(&next);
        level = next;
        (w, h) = (nw, nh);
        count += 1;
    }
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = count;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor { anisotropy_clamp: 8, ..ImageSamplerDescriptor::linear() });
    image
}

impl Geometry {
    /// The mesh with the owner's colour where the team mask is, skinned if
    /// the model has bone weights.
    pub fn team_mesh(&self, team: Vec4) -> Mesh {
        let colors: Vec<[f32; 4]> = self.colors.iter().map(|c| Vec4::ONE.lerp(team, c[0]).with_w(1.0).to_array()).collect();
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
            .with_inserted_indices(Indices::U32(self.indices.clone()));
        if self.joints.len() == self.positions.len() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, bevy::mesh::VertexAttributeValues::Uint16x4(self.joints.clone()));
            mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, self.weights.clone());
        }
        mesh
    }

    /// The geometry with its vertex colours (alpha holding the `Flag`), to
    /// merge into other models.
    pub fn model(&self) -> Model {
        Model::from_parts(
            self.positions.iter().map(|p| Vec3::from(*p)).collect(),
            self.normals.iter().map(|n| Vec3::from(*n)).collect(),
            self.colors.iter().map(|c| Vec4::from(*c)).collect(),
            self.indices.clone(),
        )
    }

    #[cfg(test)]
    pub fn is_skinned(&self) -> bool {
        !self.joints.is_empty()
    }
}

impl Baked {
    /// A material showing the baked textures.
    pub fn material(&mut self, images: &mut Assets<Image>) -> StandardMaterial {
        StandardMaterial {
            base_color_texture: self.albedo.take().map(|i| images.add(i)),
            metallic_roughness_texture: self.metal_rough.take().map(|i| images.add(i)),
            perceptual_roughness: 1.0,
            metallic: 1.0,
            ..default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_load() {
        for (name, _) in MODELS {
            let b = baked(name).expect("listed");
            for (node, g) in &b.nodes {
                assert!(!g.indices.is_empty(), "{name}/{node} has no triangles");
                assert_eq!(g.positions.len(), g.normals.len(), "{name}/{node}");
                if let Some(rig) = &b.rig {
                    assert!(g.is_skinned(), "{name} has a rig but no weights");
                    assert!(g.joints.iter().flatten().all(|j| (*j as usize) < rig.joints.len()), "{name}: bad joint index");
                }
            }
            if let Some(rig) = &b.rig {
                assert!(rig.joints.iter().enumerate().all(|(i, j)| j.parent.is_none_or(|p| p < i)), "{name}: parents must come first");
            }
        }
    }
}
