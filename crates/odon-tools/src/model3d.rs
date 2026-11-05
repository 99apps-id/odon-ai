//! Deterministic low-poly glTF models built from primitives.
//!
//! The output is a valid glTF 2.0 file with one flat-shaded mesh and vertex
//! colours: no textures, no external files, no third-party service. It is meant
//! for game blockouts and props, and the format is deliberately boring so the
//! result can be loaded by Three.js, Blender, or any glTF viewer.

use async_trait::async_trait;
use base64::Engine as _;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};

use crate::registry::{Tool, ToolContext};

type Vec3 = [f32; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Box,
    Sphere,
    Cylinder,
    Cone,
    Torus,
    Plane,
}

impl Shape {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "box" | "cube" => Some(Self::Box),
            "sphere" | "ball" => Some(Self::Sphere),
            "cylinder" | "tube" => Some(Self::Cylinder),
            "cone" => Some(Self::Cone),
            "torus" | "ring" => Some(Self::Torus),
            "plane" | "quad" => Some(Self::Plane),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
struct Part {
    shape: Shape,
    position: Vec3,
    rotation: Vec3,
    scale: Vec3,
    color: [f32; 4],
    segments: usize,
}

impl Part {
    fn new(shape: Shape, position: Vec3, scale: Vec3, color: &str) -> Self {
        Self {
            shape,
            position,
            rotation: [0.0; 3],
            scale,
            color: parse_color(color).unwrap_or([0.7, 0.65, 0.55, 1.0]),
            segments: 16,
        }
    }
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul(a: Vec3, s: f32) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: Vec3) -> f32 {
    dot(a, a).sqrt()
}

fn normalize(a: Vec3) -> Vec3 {
    let len = length(a);
    if len <= f32::EPSILON {
        [0.0, 1.0, 0.0]
    } else {
        mul(a, 1.0 / len)
    }
}

fn rotate_euler(v: Vec3, rotation: Vec3) -> Vec3 {
    let (sx, cx) = rotation[0].sin_cos();
    let (sy, cy) = rotation[1].sin_cos();
    let (sz, cz) = rotation[2].sin_cos();

    // X, then Y, then Z. Documented in the skill and the tool description.
    let (y, z) = (v[1] * cx - v[2] * sx, v[1] * sx + v[2] * cx);
    let (x, z) = (v[0] * cy + z * sy, -v[0] * sy + z * cy);
    let (x, y) = (x * cz - y * sz, x * sz + y * cz);
    [x, y, z]
}

fn transform_point(part: &Part, point: Vec3) -> Vec3 {
    let scaled = [
        point[0] * part.scale[0],
        point[1] * part.scale[1],
        point[2] * part.scale[2],
    ];
    add(rotate_euler(scaled, part.rotation), part.position)
}

fn transform_hint(part: &Part, hint: Vec3) -> Vec3 {
    normalize(rotate_euler(hint, part.rotation))
}

fn parse_color(raw: &str) -> Option<[f32; 4]> {
    let hex = raw.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let channel = |range: std::ops::Range<usize>| -> Option<f32> {
        u8::from_str_radix(&hex[range], 16).ok().map(|value| {
            let s = value as f32 / 255.0;
            // glTF stores linear colours; convert from the sRGB picker value.
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        })
    };
    Some([channel(0..2)?, channel(2..4)?, channel(4..6)?, 1.0])
}

fn parse_vec3(value: Option<&Value>, default: Vec3, field: &str) -> Result<Vec3> {
    let Some(value) = value else {
        return Ok(default);
    };
    let items = value
        .as_array()
        .filter(|items| items.len() == 3)
        .ok_or_else(|| OdonError::Tool {
            tool: "model3d_create".to_string(),
            message: format!("'{field}' must be an array of three numbers"),
        })?;
    let mut out = default;
    for (index, item) in items.iter().enumerate() {
        out[index] = item.as_f64().ok_or_else(|| OdonError::Tool {
            tool: "model3d_create".to_string(),
            message: format!("'{field}' must contain numbers"),
        })? as f32;
        if !out[index].is_finite() {
            return Err(OdonError::Tool {
                tool: "model3d_create".to_string(),
                message: format!("'{field}' contains a non-finite number"),
            });
        }
    }
    Ok(out)
}

struct Mesh {
    positions: Vec<f32>,
    normals: Vec<f32>,
    colors: Vec<f32>,
    triangles: usize,
}

impl Mesh {
    fn new() -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            colors: Vec::new(),
            triangles: 0,
        }
    }

    fn vertices(&self) -> usize {
        self.positions.len() / 3
    }

    fn push_triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, hint: Vec3, color: [f32; 4]) {
        let mut normal = cross(sub(b, a), sub(c, a));
        // A degenerate triangle (the poles of a sphere) has no normal and would
        // inject NaN into the buffer. Skip it instead of emitting it.
        if length(normal) <= 1e-9 {
            return;
        }
        normal = normalize(normal);
        if dot(normal, hint) < 0.0 {
            normal = mul(normal, -1.0);
        }
        for point in [a, b, c] {
            self.positions.extend_from_slice(&point);
            self.normals.extend_from_slice(&normal);
            self.colors.extend_from_slice(&color);
        }
        self.triangles += 1;
    }

    fn push_quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, hint: Vec3, color: [f32; 4]) {
        self.push_triangle(a, b, c, hint, color);
        self.push_triangle(a, c, d, hint, color);
    }
}

fn emit_box(mesh: &mut Mesh, part: &Part) {
    // Each face is a quad whose tangent cross product is its outward normal.
    const FACES: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
    ];
    for (normal, u, v) in FACES {
        let center = mul(normal, 0.5);
        let a = sub(sub(center, mul(u, 0.5)), mul(v, 0.5));
        let b = sub(add(center, mul(u, 0.5)), mul(v, 0.5));
        let c = add(add(center, mul(u, 0.5)), mul(v, 0.5));
        let d = add(sub(center, mul(u, 0.5)), mul(v, 0.5));
        let hint = transform_hint(part, normal);
        mesh.push_quad(
            transform_point(part, a),
            transform_point(part, b),
            transform_point(part, c),
            transform_point(part, d),
            hint,
            part.color,
        );
    }
}

fn emit_sphere(mesh: &mut Mesh, part: &Part) {
    let segments = part.segments.clamp(6, 64);
    let rings = (segments / 2).max(3);
    let point = |v: f32, u: f32| -> Vec3 {
        [
            0.5 * v.sin() * u.cos(),
            0.5 * v.cos(),
            0.5 * v.sin() * u.sin(),
        ]
    };
    for ring in 0..rings {
        let v0 = std::f32::consts::PI * ring as f32 / rings as f32;
        let v1 = std::f32::consts::PI * (ring + 1) as f32 / rings as f32;
        for segment in 0..segments {
            let u0 = std::f32::consts::TAU * segment as f32 / segments as f32;
            let u1 = std::f32::consts::TAU * (segment + 1) as f32 / segments as f32;
            let a = point(v0, u0);
            let b = point(v1, u0);
            let c = point(v1, u1);
            let d = point(v0, u1);
            for triangle in [[a, b, c], [a, c, d]] {
                let hint = normalize(add(add(triangle[0], triangle[1]), triangle[2]));
                mesh.push_triangle(
                    transform_point(part, triangle[0]),
                    transform_point(part, triangle[1]),
                    transform_point(part, triangle[2]),
                    transform_hint(part, hint),
                    part.color,
                );
            }
        }
    }
}

fn emit_cylinder(mesh: &mut Mesh, part: &Part) {
    let segments = part.segments.clamp(6, 64);
    let ring = |index: usize, y: f32| -> Vec3 {
        let angle = std::f32::consts::TAU * index as f32 / segments as f32;
        [0.5 * angle.cos(), y, 0.5 * angle.sin()]
    };
    for segment in 0..segments {
        let a = ring(segment, -0.5);
        let b = ring(segment, 0.5);
        let c = ring(segment + 1, 0.5);
        let d = ring(segment + 1, -0.5);
        let hint = normalize([a[0] + c[0], 0.0, a[2] + c[2]]);
        mesh.push_quad(
            transform_point(part, a),
            transform_point(part, b),
            transform_point(part, c),
            transform_point(part, d),
            transform_hint(part, hint),
            part.color,
        );
        let bottom_center = [0.0, -0.5, 0.0];
        let top_center = [0.0, 0.5, 0.0];
        mesh.push_triangle(
            transform_point(part, bottom_center),
            transform_point(part, d),
            transform_point(part, a),
            transform_hint(part, [0.0, -1.0, 0.0]),
            part.color,
        );
        mesh.push_triangle(
            transform_point(part, top_center),
            transform_point(part, b),
            transform_point(part, c),
            transform_hint(part, [0.0, 1.0, 0.0]),
            part.color,
        );
    }
}

fn emit_cone(mesh: &mut Mesh, part: &Part) {
    let segments = part.segments.clamp(6, 64);
    let base = |index: usize| -> Vec3 {
        let angle = std::f32::consts::TAU * index as f32 / segments as f32;
        [0.5 * angle.cos(), -0.5, 0.5 * angle.sin()]
    };
    let apex = [0.0, 0.5, 0.0];
    for segment in 0..segments {
        let a = base(segment);
        let b = base(segment + 1);
        let center = normalize(add(add(a, b), apex));
        mesh.push_triangle(
            transform_point(part, a),
            transform_point(part, b),
            transform_point(part, apex),
            transform_hint(part, center),
            part.color,
        );
        mesh.push_triangle(
            transform_point(part, [0.0, -0.5, 0.0]),
            transform_point(part, b),
            transform_point(part, a),
            transform_hint(part, [0.0, -1.0, 0.0]),
            part.color,
        );
    }
}

fn emit_torus(mesh: &mut Mesh, part: &Part) {
    let segments = part.segments.clamp(8, 64);
    let tube_segments = (part.segments / 2).clamp(6, 32);
    let point = |u: f32, v: f32| -> Vec3 {
        let (sin_u, cos_u) = u.sin_cos();
        let (sin_v, cos_v) = v.sin_cos();
        [
            (0.35 + 0.15 * cos_v) * cos_u,
            0.15 * sin_v,
            (0.35 + 0.15 * cos_v) * sin_u,
        ]
    };
    let normal = |u: f32, v: f32| -> Vec3 {
        let (sin_u, cos_u) = u.sin_cos();
        let (sin_v, cos_v) = v.sin_cos();
        [cos_v * cos_u, sin_v, cos_v * sin_u]
    };
    for segment in 0..segments {
        let u0 = std::f32::consts::TAU * segment as f32 / segments as f32;
        let u1 = std::f32::consts::TAU * (segment + 1) as f32 / segments as f32;
        for tube in 0..tube_segments {
            let v0 = std::f32::consts::TAU * tube as f32 / tube_segments as f32;
            let v1 = std::f32::consts::TAU * (tube + 1) as f32 / tube_segments as f32;
            let hint = normalize(add(
                add(normal(u0, v0), normal(u1, v0)),
                add(normal(u1, v1), normal(u0, v1)),
            ));
            mesh.push_quad(
                transform_point(part, point(u0, v0)),
                transform_point(part, point(u1, v0)),
                transform_point(part, point(u1, v1)),
                transform_point(part, point(u0, v1)),
                transform_hint(part, hint),
                part.color,
            );
        }
    }
}

fn emit_plane(mesh: &mut Mesh, part: &Part) {
    let hint = transform_hint(part, [0.0, 1.0, 0.0]);
    mesh.push_quad(
        transform_point(part, [-0.5, 0.0, -0.5]),
        transform_point(part, [0.5, 0.0, -0.5]),
        transform_point(part, [0.5, 0.0, 0.5]),
        transform_point(part, [-0.5, 0.0, 0.5]),
        hint,
        part.color,
    );
}

fn emit_part(mesh: &mut Mesh, part: &Part) {
    match part.shape {
        Shape::Box => emit_box(mesh, part),
        Shape::Sphere => emit_sphere(mesh, part),
        Shape::Cylinder => emit_cylinder(mesh, part),
        Shape::Cone => emit_cone(mesh, part),
        Shape::Torus => emit_torus(mesh, part),
        Shape::Plane => emit_plane(mesh, part),
    }
}

fn build_gltf(name: &str, parts: &[Part]) -> Result<String> {
    let mut mesh = Mesh::new();
    for part in parts {
        emit_part(&mut mesh, part);
    }
    if mesh.triangles == 0 || mesh.vertices() == 0 {
        return Err(OdonError::Tool {
            tool: "model3d_create".to_string(),
            message: "the parts produced no geometry".to_string(),
        });
    }

    let vertex_count = mesh.vertices();
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for vertex in mesh.positions.as_chunks::<3>().0 {
        for axis in 0..3 {
            min[axis] = min[axis].min(vertex[axis]);
            max[axis] = max[axis].max(vertex[axis]);
        }
    }

    let mut buffer = Vec::with_capacity(vertex_count * (12 + 12 + 16));
    for value in &mesh.positions {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    for value in &mesh.normals {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    for value in &mesh.colors {
        buffer.extend_from_slice(&value.to_le_bytes());
    }

    let position_bytes = vertex_count * 12;
    let normal_bytes = vertex_count * 12;
    let color_bytes = vertex_count * 16;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&buffer);

    let gltf = json!({
        "asset": { "version": "2.0", "generator": "ODON model3d_create" },
        "scene": 0,
        "scenes": [{ "name": name, "nodes": [0] }],
        "nodes": [{ "name": name, "mesh": 0 }],
        "meshes": [{
            "name": name,
            "primitives": [{
                "attributes": { "POSITION": 0, "NORMAL": 1, "COLOR_0": 2 },
                "material": 0,
                "mode": 4
            }]
        }],
        "materials": [{
            "name": "vertex-color",
            "doubleSided": true,
            "pbrMetallicRoughness": {
                "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                "metallicFactor": 0.0,
                "roughnessFactor": 0.9
            }
        }],
        "buffers": [{
            "byteLength": buffer.len(),
            "uri": format!("data:application/octet-stream;base64,{encoded}")
        }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": position_bytes, "target": 34962 },
            { "buffer": 0, "byteOffset": position_bytes, "byteLength": normal_bytes, "target": 34962 },
            { "buffer": 0, "byteOffset": position_bytes + normal_bytes, "byteLength": color_bytes, "target": 34962 }
        ],
        "accessors": [
            {
                "bufferView": 0,
                "componentType": 5126,
                "count": vertex_count,
                "type": "VEC3",
                "min": min,
                "max": max
            },
            { "bufferView": 1, "componentType": 5126, "count": vertex_count, "type": "VEC3" },
            { "bufferView": 2, "componentType": 5126, "count": vertex_count, "type": "VEC4" }
        ]
    });

    serde_json::to_string_pretty(&gltf).map_err(|error| OdonError::Tool {
        tool: "model3d_create".to_string(),
        message: format!("cannot serialise glTF: {error}"),
    })
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in value.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            out.push(ch);
            dash = false;
        } else if !out.is_empty() {
            dash = true;
        }
    }
    if out.is_empty() {
        "model".to_string()
    } else {
        out
    }
}

fn parse_parts(raw: &Value) -> Result<Vec<Part>> {
    let items = raw.as_array().ok_or_else(|| OdonError::Tool {
        tool: "model3d_create".to_string(),
        message: "'parts' must be an array".to_string(),
    })?;
    if items.is_empty() {
        return Err(OdonError::Tool {
            tool: "model3d_create".to_string(),
            message: "'parts' must contain at least one primitive".to_string(),
        });
    }
    if items.len() > 64 {
        return Err(OdonError::Tool {
            tool: "model3d_create".to_string(),
            message: "too many parts (max 64); merge decorative detail into bigger shapes"
                .to_string(),
        });
    }
    let mut parts = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let shape_raw =
            item.get("shape")
                .and_then(Value::as_str)
                .ok_or_else(|| OdonError::Tool {
                    tool: "model3d_create".to_string(),
                    message: format!("part {index} is missing 'shape'"),
                })?;
        let shape = Shape::parse(shape_raw).ok_or_else(|| OdonError::Tool {
            tool: "model3d_create".to_string(),
            message: format!(
                "part {index} has unknown shape '{shape_raw}' (box, sphere, cylinder, cone, torus, plane)"
            ),
        })?;
        let position = parse_vec3(item.get("position"), [0.0, 0.0, 0.0], "position")?;
        let rotation_degrees = parse_vec3(item.get("rotation"), [0.0, 0.0, 0.0], "rotation")?;
        let scale = parse_vec3(item.get("scale"), [1.0, 1.0, 1.0], "scale")?;
        if scale.iter().any(|value| *value <= 0.0001) {
            return Err(OdonError::Tool {
                tool: "model3d_create".to_string(),
                message: format!("part {index} scale must be positive on every axis"),
            });
        }
        let color = parse_color(
            item.get("color")
                .and_then(Value::as_str)
                .unwrap_or("#b7a68c"),
        )
        .ok_or_else(|| OdonError::Tool {
            tool: "model3d_create".to_string(),
            message: format!("part {index} has an invalid colour; use #rrggbb"),
        })?;
        let segments = item
            .get("segments")
            .and_then(Value::as_u64)
            .unwrap_or(16)
            .clamp(4, 64) as usize;
        parts.push(Part {
            shape,
            position,
            rotation: rotation_degrees.map(f32::to_radians),
            scale,
            color,
            segments,
        });
    }
    Ok(parts)
}

/// A small low-poly robot used by the game scaffold to demonstrate the asset
/// pipeline. Same builder as the public tool, so the file is guaranteed valid.
pub(crate) fn sample_model_gltf(name: &str) -> Option<String> {
    let parts = vec![
        Part::new(Shape::Box, [-0.12, 0.3, 0.0], [0.18, 0.6, 0.18], "#5d3d30"),
        Part::new(Shape::Box, [0.12, 0.3, 0.0], [0.18, 0.6, 0.18], "#5d3d30"),
        Part::new(Shape::Box, [0.0, 0.95, 0.0], [0.55, 0.7, 0.35], "#a8775f"),
        Part::new(Shape::Box, [0.0, 1.45, 0.0], [0.42, 0.36, 0.36], "#d5b9a3"),
        Part::new(Shape::Box, [-0.3, 1.0, 0.0], [0.14, 0.55, 0.16], "#b7a68c"),
        Part::new(Shape::Box, [0.3, 1.0, 0.0], [0.14, 0.55, 0.16], "#b7a68c"),
        Part::new(Shape::Box, [-0.1, 1.47, 0.19], [0.1, 0.08, 0.04], "#23303a"),
        Part::new(Shape::Box, [0.1, 1.47, 0.19], [0.1, 0.08, 0.04], "#23303a"),
    ];
    build_gltf(name, &parts).ok()
}

pub struct Model3dCreateTool;

#[async_trait]
impl Tool for Model3dCreateTool {
    fn name(&self) -> &str {
        "model3d_create"
    }
    fn description(&self) -> &str {
        "Create a low-poly glTF 3D model from primitives (box, sphere, cylinder, cone, torus, plane) with positions, rotations, scale, and vertex colours. Use for game assets, props, characters, and terrain blockouts."
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "output": {
                    "type": "string",
                    "description": "Workspace-relative .gltf path (default assets/models/<name>.gltf)"
                },
                "name": { "type": "string", "description": "Model name used as the glTF scene/node name" },
                "parts": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 64,
                    "description": "Primitive parts; they are merged into one flat-shaded mesh",
                    "items": {
                        "type": "object",
                        "properties": {
                            "shape": { "type": "string", "enum": ["box", "sphere", "cylinder", "cone", "torus", "plane"] },
                            "position": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 },
                            "rotation": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3, "description": "Euler degrees, applied X then Y then Z" },
                            "scale": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 },
                            "color": { "type": "string", "description": "#rrggbb" },
                            "segments": { "type": "integer", "minimum": 4, "maximum": 64, "description": "Curve resolution for sphere/cylinder/cone/torus" }
                        },
                        "required": ["shape"]
                    }
                }
            },
            "required": ["parts"]
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let parts_value = args.get("parts").cloned().unwrap_or_else(|| json!([]));
        let parts = parse_parts(&parts_value)?;
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("model")
            .to_string();
        let slug = slugify(&name);
        let mut output = args
            .get("output")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("assets/models/{slug}.gltf"));
        if !output.to_ascii_lowercase().ends_with(".gltf") {
            output.push_str(".gltf");
        }

        let gltf = build_gltf(&name, &parts)?;
        let path = crate::workspace_path::resolve_write(&ctx.workspace_root, &output)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| OdonError::Tool {
                tool: self.name().to_string(),
                message: format!("cannot create output folder: {error}"),
            })?;
        }
        std::fs::write(&path, gltf.as_bytes()).map_err(|error| OdonError::Tool {
            tool: self.name().to_string(),
            message: format!("cannot write model: {error}"),
        })?;

        Ok(json!({
            "saved": true,
            "path": output,
            "bytes": gltf.len(),
            "parts": parts.len(),
            "vertices": gltf_vertex_count(&gltf).unwrap_or(0),
            "triangles": gltf_triangle_count(&gltf).unwrap_or(0),
        }))
    }
}

fn gltf_vertex_count(gltf: &str) -> Option<u64> {
    let value: Value = serde_json::from_str(gltf).ok()?;
    value["accessors"][0]["count"].as_u64()
}

fn gltf_triangle_count(gltf: &str) -> Option<u64> {
    gltf_vertex_count(gltf).map(|vertices| vertices / 3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use odon_security::{ApprovalEngine, AuditLogger, SecurityPolicy};
    use std::sync::Arc;

    fn test_context(root: &std::path::Path) -> ToolContext {
        ToolContext {
            session_id: "model3d-test".to_string(),
            workspace_root: root.to_string_lossy().to_string(),
            approval_engine: Arc::new(ApprovalEngine::new(SecurityPolicy::default())),
            audit_logger: Arc::new(AuditLogger::new()),
            storage: None,
        }
    }

    fn parse_gltf(gltf: &str) -> Value {
        serde_json::from_str(gltf).expect("glTF must be valid JSON")
    }

    #[test]
    fn box_has_twelve_triangles_and_outward_normals() {
        let part = Part::new(Shape::Box, [0.0; 3], [1.0; 3], "#ff0000");
        let gltf = build_gltf("cube", &[part]).unwrap();
        let value = parse_gltf(&gltf);
        assert_eq!(value["accessors"][0]["count"], 36);
        assert_eq!(value["accessors"][0]["type"], "VEC3");
        assert_eq!(value["meshes"][0]["primitives"][0]["mode"], 4);

        let buffer = base64_decode(&value);
        let positions = &buffer[0..36 * 12];
        let normals = &buffer[36 * 12..36 * 24];
        let first_normal = [
            f32::from_le_bytes(normals[0..4].try_into().unwrap()),
            f32::from_le_bytes(normals[4..8].try_into().unwrap()),
            f32::from_le_bytes(normals[8..12].try_into().unwrap()),
        ];
        assert!((length(first_normal) - 1.0).abs() < 1e-5);
        assert!(positions.iter().any(|byte| *byte != 0));
    }

    fn base64_decode(value: &Value) -> Vec<u8> {
        let uri = value["buffers"][0]["uri"].as_str().unwrap();
        let encoded = uri.split(',').nth(1).unwrap();
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap()
    }

    #[test]
    fn transform_moves_a_box_so_its_feet_touch_y_zero() {
        let part = Part {
            shape: Shape::Box,
            position: [0.0, 1.0, 0.0],
            rotation: [0.0; 3],
            scale: [1.0, 2.0, 1.0],
            color: [1.0; 4],
            segments: 16,
        };
        let gltf = build_gltf("box", &[part]).unwrap();
        let value = parse_gltf(&gltf);
        let min = value["accessors"][0]["min"].as_array().unwrap();
        let max = value["accessors"][0]["max"].as_array().unwrap();
        assert!((min[1].as_f64().unwrap() - 0.0).abs() < 1e-5);
        assert!((max[1].as_f64().unwrap() - 2.0).abs() < 1e-5);
    }

    #[test]
    fn every_shape_produces_geometry_without_nan() {
        for shape in [
            Shape::Box,
            Shape::Sphere,
            Shape::Cylinder,
            Shape::Cone,
            Shape::Torus,
            Shape::Plane,
        ] {
            let part = Part {
                shape,
                position: [0.0; 3],
                rotation: [0.3, 0.5, 0.7],
                scale: [1.0; 3],
                color: [0.5, 0.5, 0.5, 1.0],
                segments: 8,
            };
            let gltf = build_gltf("shape", &[part]).unwrap_or_else(|error| {
                panic!("{shape:?} failed: {error}");
            });
            let value = parse_gltf(&gltf);
            let count = value["accessors"][0]["count"].as_u64().unwrap();
            assert!(count > 0, "{shape:?} produced no vertices");
            let buffer = base64_decode(&value);
            for window in buffer.as_chunks::<4>().0 {
                let value = f32::from_le_bytes(*window);
                assert!(value.is_finite(), "{shape:?} produced a non-finite value");
            }
        }
    }

    #[test]
    fn srgb_colours_are_converted_to_linear() {
        assert_eq!(parse_color("#000000"), Some([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse_color("#ffffff"), Some([1.0, 1.0, 1.0, 1.0]));
        let red = parse_color("#ff0000").unwrap();
        assert_eq!(red, [1.0, 0.0, 0.0, 1.0]);
        let mid = parse_color("#808080").unwrap();
        assert!(mid[0] > 0.2 && mid[0] < 0.3, "sRGB midpoint: {mid:?}");
        assert_eq!(parse_color("red"), None);
        assert_eq!(parse_color("#12345"), None);
    }

    #[test]
    fn accessors_match_the_embedded_buffer() {
        let parts = vec![
            Part::new(Shape::Box, [0.0; 3], [1.0; 3], "#a8775f"),
            Part::new(Shape::Sphere, [0.0, 1.0, 0.0], [1.0; 3], "#3f5b70"),
        ];
        let gltf = build_gltf("assembly", &parts).unwrap();
        let value = parse_gltf(&gltf);
        let count = value["accessors"][0]["count"].as_u64().unwrap() as usize;
        let buffer = base64_decode(&value);
        assert_eq!(buffer.len(), count * (12 + 12 + 16));
        assert_eq!(value["buffers"][0]["byteLength"], buffer.len());
        assert_eq!(value["bufferViews"][2]["byteLength"], count * 16);
    }

    #[test]
    fn sample_model_is_valid() {
        let gltf = sample_model_gltf("sample").expect("sample model");
        let value = parse_gltf(&gltf);
        assert!(value["accessors"][0]["count"].as_u64().unwrap() > 100);
    }

    #[tokio::test]
    async fn tool_writes_a_gltf_into_the_workspace() {
        let root = std::env::temp_dir().join(format!("odon-model3d-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let ctx = test_context(&root);
        let out = Model3dCreateTool
            .execute(
                &ctx,
                json!({
                    "name": "Test Crate",
                    "output": "assets/models/crate.gltf",
                    "parts": [
                        { "shape": "box", "scale": [0.8, 0.8, 0.8], "position": [0, 0.4, 0], "color": "#a8775f" },
                        { "shape": "box", "scale": [0.84, 0.06, 0.84], "position": [0, 0.82, 0], "color": "#5d3d30" }
                    ]
                }),
            )
            .await
            .expect("model should be written");
        assert_eq!(out["saved"], true);
        let path = root.join("assets/models/crate.gltf");
        assert!(path.is_file());
        let text = std::fs::read_to_string(path).unwrap();
        parse_gltf(&text);

        // Traversal outside the workspace is rejected.
        let err = Model3dCreateTool
            .execute(
                &ctx,
                json!({ "output": "../escape.gltf", "parts": [{ "shape": "box" }] }),
            )
            .await
            .expect_err("traversal must be rejected");
        assert!(format!("{err}").to_lowercase().contains("workspace"));
        std::fs::remove_dir_all(root).ok();
    }
}
