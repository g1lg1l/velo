//! 3D models: an .obj (its vertices and faces) or a .ply (its vertices and, when they follow,
//! its faces; ASCII or binary little endian). A model gives a Particles node (shape Model)
//! points to be born on, and a Mesh node its triangles with their normals. Everything comes out
//! centered and scaled into a unit ball, y flipped to run down as the canvas does. A model file
//! is untrusted: sizes are capped and every count in it is checked against the bytes there are
//! before anything is allocated for it.

use crate::gpu::MAX_POINTS;
use std::path::Path;

/// The files a Particles or Mesh node takes as models.
pub const EXTENSIONS: [&str; 2] = ["obj", "ply"];

/// The largest file read, and the most vertices or triangles kept from one.
const MAX_BYTES: u64 = 256 << 20;
const MAX_ITEMS: usize = 2_000_000;
/// The most triangles a mesh draws.
pub const MAX_TRIANGLES: usize = 600_000;
/// Points spread over a model's faces.
const SURFACE: usize = 65_536;
/// An .ply's header must end within this many bytes.
const MAX_HEADER: usize = 64 << 10;

/// A model as the nodes use it.
#[derive(Debug, Default)]
pub struct Model {
    /// Where a particle is born, fitted: the vertices (thinned to [`MAX_POINTS`]), or with faces
    /// points spread over them by area.
    pub points: Vec<[f32; 3]>,
    /// The surface: a triangle list, each corner a position and a normal (six floats), the
    /// normals smooth where triangles share a vertex. Empty with no faces, or too many.
    pub mesh: Vec<[f32; 6]>,
    /// How many triangles the file has (as many as are read).
    pub faces: usize,
}

/// A file read: its vertices, and its triangles by their indices.
#[derive(Debug)]
struct Raw {
    vertices: Vec<[f32; 3]>,
    triangles: Vec<[usize; 3]>,
}

/// Whether `path` names a model, by its extension.
pub fn is_model(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.iter().any(|m| e.eq_ignore_ascii_case(m)))
}

/// The model in `path`.
pub fn load(path: &Path) -> Result<Model, String> {
    let name = path.display();
    let len = std::fs::metadata(path).map_err(|e| format!("{name}: {e}"))?.len();
    if len > MAX_BYTES {
        return Err(format!("{name}: {} MB is too big for a model ({} MB at most)", len >> 20, MAX_BYTES >> 20));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
    let raw = if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ply")) { ply(&bytes) } else { obj(&bytes) };
    raw.and_then(build).map_err(|e| format!("{name}: {e}"))
}

/// A model from what was read: fitted, with its normals, its points and its mesh.
fn build(raw: Raw) -> Result<Model, String> {
    let Raw { vertices, triangles } = raw;
    let finite: Vec<bool> = vertices.iter().map(|p| p.iter().all(|c| c.is_finite())).collect();
    if !finite.contains(&true) {
        return Err("no points in it".into());
    }
    // a face on a vertex that isn't a number goes, the rest stay (the indices stay as they are)
    let triangles: Vec<[usize; 3]> = triangles.into_iter().filter(|t| t.iter().all(|&i| finite.get(i) == Some(&true))).collect();
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in vertices.iter().zip(&finite).filter(|v| *v.1).map(|v| v.0) {
        for i in 0..3 {
            (lo[i], hi[i]) = (lo[i].min(p[i]), hi[i].max(p[i]));
        }
    }
    let middle: [f32; 3] = std::array::from_fn(|i| (lo[i] + hi[i]) / 2.0);
    let far = vertices.iter().zip(&finite).filter(|v| *v.1).map(|v| (0..3).map(|i| (v.0[i] - middle[i]).powi(2)).sum::<f32>().sqrt()).fold(0.0, f32::max);
    let scale = if far > 1e-9 { 1.0 / far } else { 1.0 };
    // the normals while the model is as it was made, so a face keeps its side; then both flipped
    let mut sums = vec![[0.0f32; 3]; if triangles.is_empty() { 0 } else { vertices.len() }];
    for t in &triangles {
        let [a, b, c] = t.map(|i| vertices[i]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        // its length is twice the area: the bigger faces weigh more
        let cross = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for &i in t {
            (0..3).for_each(|k| sums[i][k] += cross[k]);
        }
    }
    let place = |p: &[f32; 3]| [(p[0] - middle[0]) * scale, -(p[1] - middle[1]) * scale, (p[2] - middle[2]) * scale];
    let normal = |i: usize| {
        let s = sums[i];
        let len = (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt();
        // a vertex of flat or no faces looks up
        if len > 1e-20 { [s[0] / len, -s[1] / len, s[2] / len] } else { [0.0, -1.0, 0.0] }
    };
    let fitted: Vec<[f32; 3]> = vertices.iter().map(place).collect();
    let mesh = if triangles.is_empty() || triangles.len() > MAX_TRIANGLES {
        Vec::new()
    } else {
        triangles
            .iter()
            .flatten()
            .map(|&i| {
                let (p, n) = (fitted[i], normal(i));
                [p[0], p[1], p[2], n[0], n[1], n[2]]
            })
            .collect()
    };
    let points = if triangles.is_empty() { thin(fitted.into_iter().zip(&finite).filter(|v| *v.1).map(|v| v.0).collect()) } else { surface(&fitted, &triangles) };
    Ok(Model { points, mesh, faces: triangles.len() })
}

/// Every `k`-th of `all`, so that at most [`MAX_POINTS`] stay.
fn thin(all: Vec<[f32; 3]>) -> Vec<[f32; 3]> {
    let k = all.len().div_ceil(MAX_POINTS).max(1);
    all.into_iter().step_by(k).collect()
}

/// [`SURFACE`] points over the triangles, by their area: a triangle chosen by where a random
/// number falls in the running total, then a point in it. A model with no area gives its vertices.
fn surface(vertices: &[[f32; 3]], triangles: &[[usize; 3]]) -> Vec<[f32; 3]> {
    let corner = |t: &[usize; 3], i: usize| vertices[t[i]].map(f64::from);
    let mut total = 0.0;
    let running: Vec<f64> = triangles
        .iter()
        .map(|t| {
            let (a, b, c) = (corner(t, 0), corner(t, 1), corner(t, 2));
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let cross = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            total += 0.5 * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
            total
        })
        .collect();
    if total <= 0.0 {
        return thin(vertices.to_vec());
    }
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut random = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    (0..SURFACE)
        .map(|_| {
            let t = &triangles[running.partition_point(|&r| r < random() * total).min(triangles.len() - 1)];
            let (a, b, c) = (corner(t, 0), corner(t, 1), corner(t, 2));
            let (s, r) = (random().sqrt(), random());
            let (wa, wb, wc) = (1.0 - s, s * (1.0 - r), s * r);
            std::array::from_fn(|i| (a[i] * wa + b[i] * wb + c[i] * wc) as f32)
        })
        .collect()
}

/// An .obj: `v x y z` lines for the vertices, `f` lines (a face of three or more, indices from
/// 1, negative ones back from the last so far, `a/b/c` as `a`) for the triangles.
fn obj(bytes: &[u8]) -> Result<Raw, String> {
    let text = String::from_utf8_lossy(bytes);
    let (mut vertices, mut triangles): (Vec<[f32; 3]>, Vec<[usize; 3]>) = (Vec::new(), Vec::new());
    for line in text.lines() {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("v") if vertices.len() < MAX_ITEMS => {
                let c: Vec<f32> = words.take(3).filter_map(|w| w.parse().ok()).collect();
                if let [x, y, z] = c[..] {
                    vertices.push([x, y, z]);
                }
            }
            Some("f") if triangles.len() < MAX_ITEMS => {
                // the face's corners that point at a vertex there is; a fan of triangles from the first
                let corners: Vec<usize> = words
                    .filter_map(|w| w.split('/').next()?.parse::<i64>().ok())
                    .filter_map(|i| match i {
                        1.. => usize::try_from(i - 1).ok(),
                        ..=-1 => vertices.len().checked_sub(usize::try_from(-i).ok()?),
                        0 => None,
                    })
                    .filter(|&i| i < vertices.len())
                    .collect();
                for pair in corners.windows(2).skip(1) {
                    triangles.push([corners[0], pair[0], pair[1]]);
                }
            }
            _ => {}
        }
    }
    Ok(Raw { vertices, triangles })
}

/// What a .ply property holds.
#[derive(Clone, Copy)]
enum Kind {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Kind {
    fn parse(word: &str) -> Result<Self, String> {
        Ok(match word {
            "char" | "int8" => Self::I8,
            "uchar" | "uint8" => Self::U8,
            "short" | "int16" => Self::I16,
            "ushort" | "uint16" => Self::U16,
            "int" | "int32" => Self::I32,
            "uint" | "uint32" => Self::U32,
            "float" | "float32" => Self::F32,
            "double" | "float64" => Self::F64,
            _ => return Err(format!("a {word} property isn't read")),
        })
    }

    fn size(self) -> usize {
        match self {
            Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 | Self::F32 => 4,
            Self::F64 => 8,
        }
    }

    /// The value in the first `size` bytes of `b`, little endian.
    fn read(self, b: &[u8]) -> f32 {
        match self {
            Self::I8 => f32::from(b[0] as i8),
            Self::U8 => f32::from(b[0]),
            Self::I16 => f32::from(i16::from_le_bytes([b[0], b[1]])),
            Self::U16 => f32::from(u16::from_le_bytes([b[0], b[1]])),
            Self::I32 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
            Self::U32 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
            Self::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            Self::F64 => f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32,
        }
    }
}

/// A property of an element of a .ply: one value, or a list (the kind of its length, of its items).
enum Prop {
    One(String, Kind),
    List(Kind, Kind),
}

struct Element {
    name: String,
    count: usize,
    props: Vec<Prop>,
}

/// A .ply: its first element must be the vertices, with plain properties (x, y and z among
/// them); a face element right after them gives the triangles (its first list is the vertex
/// indices); the rest of the file is left alone. ASCII or binary little endian.
fn ply(bytes: &[u8]) -> Result<Raw, String> {
    let head = &bytes[..bytes.len().min(MAX_HEADER)];
    let end = head.windows(11).position(|w| w == b"end_header\n" || w == b"end_header\r").ok_or("not a .ply, or its header never ends")?;
    let body = bytes.get(end + 11 + usize::from(head[end + 10] == b'\r' && bytes.get(end + 11) == Some(&b'\n'))..).unwrap_or_default();
    let header = String::from_utf8_lossy(&head[..end]);
    let mut lines = header.lines().map(str::trim);
    if lines.next() != Some("ply") {
        return Err("not a .ply".into());
    }
    let (mut binary, mut elements) = (None, Vec::<Element>::new());
    for line in lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words[..] {
            ["format", "ascii", _] => binary = Some(false),
            ["format", "binary_little_endian", _] => binary = Some(true),
            ["format", ..] => return Err("a big endian .ply isn't read: save it as ASCII or binary little endian".into()),
            ["element", name, n] => elements.push(Element { name: name.to_string(), count: n.parse().map_err(|_| format!("the {name} count isn't a number"))?, props: Vec::new() }),
            ["property", "list", count, item, _] => elements.last_mut().ok_or("a property before any element")?.props.push(Prop::List(Kind::parse(count)?, Kind::parse(item)?)),
            ["property", kind, name] => elements.last_mut().ok_or("a property before any element")?.props.push(Prop::One(name.to_string(), Kind::parse(kind)?)),
            _ => {}
        }
    }
    let binary = binary.ok_or("the .ply has no format line")?;
    let vertex = elements.first().filter(|e| e.name == "vertex").ok_or("the vertices must be the first element of a .ply")?;
    let scalars: Vec<(&str, Kind)> = vertex
        .props
        .iter()
        .map(|p| match p {
            Prop::One(name, kind) => Ok((name.as_str(), *kind)),
            Prop::List(..) => Err("a list among the vertices' properties isn't read"),
        })
        .collect::<Result<_, _>>()?;
    let at = |axis: &str| scalars.iter().position(|p| p.0 == axis).ok_or_else(|| format!("the vertices have no {axis}"));
    let (x, y, z) = (at("x")?, at("y")?, at("z")?);
    let count = vertex.count;
    if count > MAX_ITEMS {
        return Err(format!("{count} vertices: {MAX_ITEMS} at most"));
    }
    let face = elements.get(1).filter(|e| e.name == "face");
    let mut vertices = Vec::with_capacity(count.min(MAX_ITEMS));
    let mut triangles = Vec::new();
    if binary {
        let offsets: Vec<usize> = scalars.iter().scan(0, |o, p| Some(std::mem::replace(o, *o + p.1.size()))).collect();
        let stride: usize = scalars.iter().map(|p| p.1.size()).sum();
        // the header's count against the bytes there are
        if stride == 0 || count > body.len() / stride {
            return Err(format!("truncated: the header says {count} vertices, the file holds {}", body.len().checked_div(stride).unwrap_or(0)));
        }
        for i in 0..count {
            let v = &body[i * stride..(i + 1) * stride];
            let read = |p: usize| scalars[p].1.read(&v[offsets[p]..]);
            vertices.push([read(x), read(y), read(z)]);
        }
        if let Some(face) = face {
            let rest = &body[count * stride..];
            let least: usize = face.props.iter().map(|p| match p { Prop::One(_, k) => k.size(), Prop::List(c, _) => c.size() }).sum();
            if least == 0 || face.count > rest.len() / least {
                return Err(format!("truncated: the header says {} faces, the file holds {}", face.count, rest.len().checked_div(least).unwrap_or(0)));
            }
            let truncated = || "truncated: a face runs past the end of the file".to_string();
            let mut pos = 0usize;
            for _ in 0..face.count {
                let mut corners: Option<Vec<f32>> = None;
                for prop in &face.props {
                    match prop {
                        Prop::One(_, kind) => pos += kind.size(),
                        Prop::List(length, item) => {
                            let n = length.read(rest.get(pos..pos + length.size()).ok_or_else(truncated)?);
                            pos += length.size();
                            // a length that isn't a count, or that runs past the file, is a cut file
                            let bytes = (n >= 0.0).then(|| (n as usize).checked_mul(item.size())).flatten().and_then(|b| rest.get(pos..pos.checked_add(b)?)).ok_or_else(truncated)?;
                            pos += bytes.len();
                            if corners.is_none() {
                                corners = Some(bytes.chunks(item.size()).map(|c| item.read(c)).collect());
                            }
                        }
                    }
                }
                if let Some(corners) = corners {
                    fan(&corners, count, &mut triangles);
                }
                if triangles.len() >= MAX_ITEMS {
                    break;
                }
            }
        }
    } else {
        let text = String::from_utf8_lossy(body);
        let mut lines = text.lines();
        for _ in 0..count {
            let line = lines.next().ok_or_else(|| format!("truncated: the header says {count} vertices, the file has fewer"))?;
            let w: Vec<&str> = line.split_whitespace().collect();
            let number = |p: usize| w.get(p).and_then(|v| v.parse::<f32>().ok()).unwrap_or(f32::NAN);
            // a line that isn't a vertex stays as one that isn't a number: the faces' indices stay right
            vertices.push([number(x), number(y), number(z)]);
        }
        if let Some(face) = face {
            for _ in 0..face.count {
                let line = lines.next().ok_or_else(|| format!("truncated: the header says {} faces, the file has fewer", face.count))?;
                let mut words = line.split_whitespace();
                let mut corners: Option<Vec<f32>> = None;
                for prop in &face.props {
                    match prop {
                        Prop::One(..) => {
                            words.next();
                        }
                        Prop::List(..) => {
                            let n: usize = words.next().and_then(|w| w.parse().ok()).unwrap_or(0);
                            let items: Vec<f32> = words.by_ref().take(n).map(|w| w.parse().unwrap_or(-1.0)).collect();
                            if corners.is_none() {
                                corners = Some(items);
                            }
                        }
                    }
                }
                if let Some(corners) = corners {
                    fan(&corners, count, &mut triangles);
                }
                if triangles.len() >= MAX_ITEMS {
                    break;
                }
            }
        }
    }
    Ok(Raw { vertices, triangles })
}

/// A face's corners (vertex indices as read) as a fan of triangles from the first; a face with
/// a corner outside the `vertices` there are is left out.
fn fan(corners: &[f32], vertices: usize, into: &mut Vec<[usize; 3]>) {
    let ok: Vec<usize> = corners.iter().filter_map(|&c| (c >= 0.0 && (c as usize) < vertices).then_some(c as usize)).collect();
    if ok.len() == corners.len() {
        for pair in ok.windows(2).skip(1) {
            into.push([ok[0], pair[0], pair[1]]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radius(p: &[f32; 3]) -> f32 {
        p.iter().map(|c| c * c).sum::<f32>().sqrt()
    }

    const CUBE: &str = "v -1 -1 -1\nv 1 -1 -1\nv 1 1 -1\nv -1 1 -1\nv -1 -1 1\nv 1 -1 1\nv 1 1 1\nv -1 1 1\n";
    /// a face each side, as quads, some with the slashes of texture and normal indices
    const FACES: &str = "f 1 2 3 4\nf 5/1/1 6/2/1 7/3/1 8/4/1\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";

    fn model(text: &str) -> Model {
        build(obj(text.as_bytes()).unwrap()).unwrap()
    }

    #[test]
    fn a_model_is_centered_scaled_to_a_unit_and_upside_down() {
        // a box a long way off, 4 wide, 2 high, 2 deep, its top at y = 51
        let m = model("v 98 49 -10\nv 102 51 -8\nv 100 50 -9\n");
        assert!(m.points.iter().map(radius).fold(0.0, f32::max) - 1.0 < 1e-5, "the farthest at 1: {:?}", m.points);
        assert_eq!(m.points[2], [0.0, 0.0, 0.0], "the middle");
        assert!(m.points[1][1] < 0.0 && m.points[0][1] > 0.0, "the top (y 51) is up on a canvas: y runs down");
        assert!(m.mesh.is_empty() && m.faces == 0, "no faces, no mesh");
        assert!(build(Raw { vertices: vec![[f32::NAN, 0.0, 0.0]], triangles: Vec::new() }).is_err() && build(Raw { vertices: Vec::new(), triangles: Vec::new() }).is_err());
    }

    #[test]
    fn faces_give_a_mesh_with_outward_normals_and_points_on_the_surface() {
        // no faces: the eight corners
        assert_eq!(model(CUBE).points.len(), 8);
        let m = model(&format!("{CUBE}{FACES}"));
        assert_eq!((m.faces, m.mesh.len(), m.points.len()), (12, 36, SURFACE), "twelve triangles, three corners each");
        // fitted from the vertices: the cube's corners are at 1/√3 exactly
        let edge = 1.0 / 3f32.sqrt();
        assert!(m.points.iter().all(|p| p.iter().any(|c| (c.abs() - edge).abs() < 1e-4) && p.iter().all(|c| c.abs() <= edge + 1e-4)), "on the cube");
        for axis in 0..3 {
            assert!(m.points.iter().any(|p| p[axis] > edge - 1e-4) && m.points.iter().any(|p| p[axis] < -edge + 1e-4), "all six faces get some");
        }
        // a corner's normal is unit and points away from the middle (y flipped with the position)
        for v in &m.mesh {
            let (p, n) = ([v[0], v[1], v[2]], [v[3], v[4], v[5]]);
            assert!((radius(&n) - 1.0).abs() < 1e-4 && n.iter().zip(&p).map(|(a, b)| a * b).sum::<f32>() > 0.0, "{v:?}");
        }
    }

    #[test]
    fn what_cant_be_drawn_is_left_out_not_the_model() {
        // a face past the end, a zero, one back from the last, nonsense lines, and a vertex that isn't a number
        let m = model("v 0 0 0\nv 1 0 0\nv 0 1 0\nv nan 0 0\nf 1 2 99\nf 0 1 2\nf -4 -3 -2\nf 1 2 4\nf x y z\nv 1 2\nv a b c\nfoo\n");
        assert_eq!(m.faces, 1, "the one good face, -4 -3 -2; the one on the NaN vertex is out");
        assert_eq!(m.points.len(), SURFACE);
        assert!(m.mesh.iter().all(|c| c.iter().all(|x| x.is_finite())));
        assert!(obj(b"").unwrap().vertices.is_empty() && load(Path::new("no such.obj")).is_err());
        assert!(build(obj(b"just words\n").unwrap()).is_err());
        // a face with no area has normals that point somewhere, not nowhere
        let flat = model("v 0 0 0\nv 1 0 0\nv 2 0 0\nf 1 2 3\n");
        assert!(flat.mesh.iter().all(|c| c.iter().all(|x| x.is_finite())) && flat.points.len() <= SURFACE);
    }

    fn ply_header(format: &str, count: impl std::fmt::Display, properties: &str, faces: &str) -> String {
        format!("ply\nformat {format} 1.0\ncomment made here\nelement vertex {count}\n{properties}{faces}end_header\n")
    }

    const FLOATS: &str = "property float x\nproperty float y\nproperty float z\n";
    const LIST: &str = "element face 1\nproperty list uchar int vertex_indices\n";

    #[test]
    fn a_ply_gives_its_vertices_and_faces_ascii_or_binary() {
        let ascii = format!("{}1 2 3 255\n4 5 6 0\n7 8 9 0\n3 0 1 2\n", ply_header("ascii", 3, "property float x\nproperty float y\nproperty float z\nproperty uchar red\n", LIST));
        let raw = ply(ascii.as_bytes()).unwrap();
        assert_eq!((raw.vertices, raw.triangles), (vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]], vec![[0, 1, 2]]));
        // binary: an extra uchar before the doubles, so the stride is 1 + 24; a quad of ints
        let mut binary = ply_header("binary_little_endian", 4, "property uchar flag\nproperty double x\nproperty double y\nproperty double z\n", "element face 1\nproperty list uchar int vertex_indices\n").into_bytes();
        for v in [[1.5f64, -2.0, 3.0], [0.25, 0.5, 0.75], [0.0, 1.0, 2.0], [5.0, 5.0, 5.0]] {
            binary.push(7);
            binary.extend(v.iter().flat_map(|c| c.to_le_bytes()));
        }
        binary.push(4);
        binary.extend([0i32, 1, 2, 3].iter().flat_map(|c| c.to_le_bytes()));
        let raw = ply(&binary).unwrap();
        assert_eq!((raw.vertices[..2].to_vec(), raw.triangles), (vec![[1.5, -2.0, 3.0], [0.25, 0.5, 0.75]], vec![[0, 1, 2], [0, 2, 3]]), "a quad is two triangles");
        // with CRLF line ends too, and the vertices alone without faces
        assert_eq!(ply(ascii.replace('\n', "\r\n").as_bytes()).unwrap().vertices.len(), 3);
        assert!(ply(ply_header("ascii", 1, FLOATS, "").replace("end_header\n", "end_header\n1 2 3\n").as_bytes()).unwrap().triangles.is_empty());
    }

    #[test]
    fn a_ply_that_lies_or_is_cut_short_is_an_error_not_a_crash() {
        // ten vertices where the header says a thousand, and where it says 4 billion
        for count in [1000u64, 4_000_000_000] {
            let mut cut = ply_header("binary_little_endian", count, FLOATS, "").into_bytes();
            cut.extend([0u8; 120]);
            let e = ply(&cut).unwrap_err();
            assert!(e.contains("truncated") || e.contains("at most"), "{e}");
            assert!(ply(ply_header("ascii", count, FLOATS, "").as_bytes()).is_err());
        }
        assert!(ply(b"").is_err() && ply(b"ply\nformat ascii 1.0\n").is_err(), "no header end");
        assert!(ply(ply_header("binary_big_endian", 1, FLOATS, "").as_bytes()).unwrap_err().contains("big endian"));
        assert!(ply(ply_header("ascii", 1, "property float x\nproperty float y\n", "").as_bytes()).unwrap_err().contains("no z"));
        assert!(ply(ply_header("ascii", 1, "property list uchar int x\n", "").as_bytes()).is_err());
        assert!(ply(b"obj\nend_header\n").is_err(), "not a ply");
        // faces: a count that lies, a face cut short, a length that runs on, an index past the vertices
        let mut faces = ply_header("binary_little_endian", 3, FLOATS, "element face 1000\nproperty list uchar int vertex_indices\n").into_bytes();
        faces.extend([0u8; 36]);
        assert!(ply(&faces).unwrap_err().contains("truncated"));
        let mut cut = ply_header("binary_little_endian", 3, FLOATS, LIST).into_bytes();
        cut.extend([0u8; 36]);
        cut.extend([3, 0, 0, 0, 0, 1, 0, 0, 0]);
        assert!(ply(&cut).unwrap_err().contains("truncated"));
        let mut long = ply_header("binary_little_endian", 3, FLOATS, LIST).into_bytes();
        long.extend([0u8; 36]);
        long.extend([255, 0, 0, 0, 0]);
        assert!(ply(&long).unwrap_err().contains("truncated"));
        let mut past = ply_header("binary_little_endian", 3, FLOATS, LIST).into_bytes();
        past.extend([0u8; 36]);
        past.push(3);
        past.extend([0i32, 1, 77].iter().flat_map(|c| c.to_le_bytes()));
        assert!(ply(&past).unwrap().triangles.is_empty(), "that face is left out");
        let ascii_short = ply_header("ascii", 1, FLOATS, "element face 5\nproperty list uchar int vertex_indices\n") + "0 0 0\n3 0 0 0\n";
        assert!(ply(ascii_short.as_bytes()).unwrap_err().contains("truncated"));
        // a bigger one than it keeps is thinned to what it keeps
        let mut big = ply_header("binary_little_endian", 3 * MAX_POINTS, FLOATS, "").into_bytes();
        big.extend((0..3 * MAX_POINTS * 3).flat_map(|i| (i as f32).to_le_bytes()));
        assert!(build(ply(&big).unwrap()).unwrap().points.len() <= MAX_POINTS);
    }
}
