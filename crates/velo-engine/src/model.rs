//! 3D models as clouds of points for a Particles node (shape Model): an .obj (its vertices, or
//! points spread over its faces) or a .ply (its vertices, ASCII or binary little endian). The
//! points come out centered and scaled into -1..1, y flipped to run down as the canvas does. A
//! model file is untrusted: sizes are capped and every count in it is checked against the bytes
//! there are before anything is allocated for it.

use crate::gpu::MAX_POINTS;
use std::path::Path;

/// The files a Particles node takes as models.
pub const EXTENSIONS: [&str; 2] = ["obj", "ply"];

/// The largest file read, and the most vertices or triangles kept from an .obj.
const MAX_BYTES: u64 = 256 << 20;
const MAX_ITEMS: usize = 4_000_000;
/// Points spread over an .obj's faces.
const SURFACE: usize = 65_536;
/// An .ply's header must end within this many bytes.
const MAX_HEADER: usize = 64 << 10;

/// Whether `path` names a model, by its extension.
pub fn is_model(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.iter().any(|m| e.eq_ignore_ascii_case(m)))
}

/// The points of the model in `path`, fitted.
pub fn load(path: &Path) -> Result<Vec<[f32; 3]>, String> {
    let name = path.display();
    let len = std::fs::metadata(path).map_err(|e| format!("{name}: {e}"))?.len();
    if len > MAX_BYTES {
        return Err(format!("{name}: {} MB is too big for a model ({} MB at most)", len >> 20, MAX_BYTES >> 20));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
    let points = if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ply")) { ply(&bytes) } else { obj(&bytes) };
    fit(points.map_err(|e| format!("{name}: {e}"))?).map_err(|e| format!("{name}: {e}"))
}

/// The points centered and scaled so the farthest is at distance 1, y flipped; the ones that
/// aren't numbers dropped.
fn fit(mut points: Vec<[f32; 3]>) -> Result<Vec<[f32; 3]>, String> {
    points.retain(|p| p.iter().all(|c| c.is_finite()));
    if points.is_empty() {
        return Err("no points in it".into());
    }
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &points {
        for i in 0..3 {
            (lo[i], hi[i]) = (lo[i].min(p[i]), hi[i].max(p[i]));
        }
    }
    let middle: [f32; 3] = std::array::from_fn(|i| (lo[i] + hi[i]) / 2.0);
    let far = points.iter().map(|p| (0..3).map(|i| (p[i] - middle[i]).powi(2)).sum::<f32>().sqrt()).fold(0.0, f32::max);
    let scale = if far > 1e-9 { 1.0 / far } else { 1.0 };
    for p in &mut points {
        *p = [(p[0] - middle[0]) * scale, -(p[1] - middle[1]) * scale, (p[2] - middle[2]) * scale];
    }
    Ok(points)
}

/// Every `k`-th of `all`, so that at most [`MAX_POINTS`] stay.
fn thin(all: Vec<[f32; 3]>) -> Vec<[f32; 3]> {
    let k = all.len().div_ceil(MAX_POINTS).max(1);
    all.into_iter().step_by(k).collect()
}

/// An .obj: `v x y z` lines for the points, `f` lines (a face of three or more, indices from 1,
/// negative ones back from the last so far, `a/b/c` as `a`) for the surface. With faces, points
/// are spread over them by area; without, the vertices are the points.
fn obj(bytes: &[u8]) -> Result<Vec<[f32; 3]>, String> {
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
    if triangles.is_empty() {
        return Ok(thin(vertices));
    }
    // points over the faces, by their area: a triangle chosen by where a random number falls in
    // the running total, then a point in it
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
    // a model with no area (all its faces flat lines): its vertices
    if total <= 0.0 {
        return Ok(thin(vertices));
    }
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut random = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    Ok((0..SURFACE)
        .map(|_| {
            let t = &triangles[running.partition_point(|&r| r < random() * total).min(triangles.len() - 1)];
            let (a, b, c) = (corner(t, 0), corner(t, 1), corner(t, 2));
            let (s, r) = (random().sqrt(), random());
            let (wa, wb, wc) = (1.0 - s, s * (1.0 - r), s * r);
            std::array::from_fn(|i| (a[i] * wa + b[i] * wb + c[i] * wc) as f32)
        })
        .collect())
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
    fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "char" | "int8" => Self::I8,
            "uchar" | "uint8" => Self::U8,
            "short" | "int16" => Self::I16,
            "ushort" | "uint16" => Self::U16,
            "int" | "int32" => Self::I32,
            "uint" | "uint32" => Self::U32,
            "float" | "float32" => Self::F32,
            "double" | "float64" => Self::F64,
            _ => return None,
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

/// A .ply: its first element must be the vertices, with plain properties (x, y and z among
/// them); the rest of the file (faces, colors) is left alone. ASCII or binary little endian.
fn ply(bytes: &[u8]) -> Result<Vec<[f32; 3]>, String> {
    let head = &bytes[..bytes.len().min(MAX_HEADER)];
    let end = head.windows(11).position(|w| w == b"end_header\n" || w == b"end_header\r").ok_or("not a .ply, or its header never ends")?;
    let body = bytes.get(end + 11 + usize::from(head[end + 10] == b'\r' && bytes.get(end + 11) == Some(&b'\n'))..).unwrap_or_default();
    let header = String::from_utf8_lossy(&head[..end]);
    let mut lines = header.lines().map(str::trim);
    if lines.next() != Some("ply") {
        return Err("not a .ply".into());
    }
    let (mut binary, mut count, mut properties, mut in_vertices, mut elements) = (None, 0usize, Vec::<(String, Kind)>::new(), false, 0);
    for line in lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words[..] {
            ["format", "ascii", _] => binary = Some(false),
            ["format", "binary_little_endian", _] => binary = Some(true),
            ["format", ..] => return Err("a big endian .ply isn't read: save it as ASCII or binary little endian".into()),
            ["element", name, n] => {
                elements += 1;
                in_vertices = name == "vertex";
                if in_vertices {
                    if elements != 1 {
                        return Err("the vertices must be the first element of a .ply".into());
                    }
                    count = n.parse().map_err(|_| "the vertex count isn't a number")?;
                }
            }
            ["property", "list", ..] if in_vertices => return Err("a list among the vertices' properties isn't read".into()),
            ["property", kind, name] if in_vertices => properties.push((name.to_string(), Kind::parse(kind).ok_or_else(|| format!("a {kind} property isn't read"))?)),
            _ => {}
        }
    }
    let binary = binary.ok_or("the .ply has no format line")?;
    let at = |axis: &str| properties.iter().position(|p| p.0 == axis).ok_or_else(|| format!("the vertices have no {axis}"));
    let (x, y, z) = (at("x")?, at("y")?, at("z")?);
    let k = count.div_ceil(MAX_POINTS).max(1);
    let mut points = Vec::with_capacity(count.min(MAX_POINTS));
    if binary {
        let offsets: Vec<usize> = properties.iter().scan(0, |o, p| Some(std::mem::replace(o, *o + p.1.size()))).collect();
        let stride: usize = properties.iter().map(|p| p.1.size()).sum();
        // the header's count against the bytes there are
        if stride == 0 || count > body.len() / stride {
            return Err(format!("truncated: the header says {count} vertices, the file holds {}", body.len().checked_div(stride).unwrap_or(0)));
        }
        for i in (0..count).step_by(k) {
            let v = &body[i * stride..(i + 1) * stride];
            let read = |p: usize| properties[p].1.read(&v[offsets[p]..]);
            points.push([read(x), read(y), read(z)]);
        }
    } else {
        let text = String::from_utf8_lossy(body);
        let lines: Vec<&str> = text.lines().take(count).collect();
        if lines.len() < count {
            return Err(format!("truncated: the header says {count} vertices, the file holds {}", lines.len()));
        }
        for line in lines.into_iter().step_by(k) {
            let w: Vec<&str> = line.split_whitespace().collect();
            let number = |p: usize| w.get(p).and_then(|v| v.parse::<f32>().ok());
            if let (Some(x), Some(y), Some(z)) = (number(x), number(y), number(z)) {
                points.push([x, y, z]);
            }
        }
    }
    Ok(points)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radius(p: &[f32; 3]) -> f32 {
        p.iter().map(|c| c * c).sum::<f32>().sqrt()
    }

    #[test]
    fn points_come_out_centered_scaled_to_a_unit_and_upside_down() {
        // a box a long way off, 4 wide, 2 high, 2 deep, its top at y = 51
        let out = fit(vec![[98.0, 49.0, -10.0], [102.0, 51.0, -8.0], [100.0, 50.0, -9.0]]).unwrap();
        assert!(out.iter().map(radius).fold(0.0, f32::max) - 1.0 < 1e-5, "the farthest at 1: {out:?}");
        assert_eq!(out[2], [0.0, 0.0, 0.0], "the middle");
        assert!(out[1][1] < 0.0 && out[0][1] > 0.0, "the top (y 51) is up on a canvas: y runs down");
        assert!(fit(vec![[f32::NAN, 0.0, 0.0]]).is_err() && fit(Vec::new()).is_err());
    }

    #[test]
    fn an_obj_gives_its_vertices_or_points_over_its_faces() {
        let cube = "v -1 -1 -1\nv 1 -1 -1\nv 1 1 -1\nv -1 1 -1\nv -1 -1 1\nv 1 -1 1\nv 1 1 1\nv -1 1 1\n";
        // no faces: the eight corners
        assert_eq!(obj(cube.as_bytes()).unwrap().len(), 8);
        // a face each side, as quads, some with the slashes of texture and normal indices
        let faces = "f 1 2 3 4\nf 5/1/1 6/2/1 7/3/1 8/4/1\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";
        let surface = fit(obj(format!("{cube}{faces}").as_bytes()).unwrap()).unwrap();
        assert_eq!(surface.len(), SURFACE);
        // the points are fitted on their own (a sample's far corner isn't quite the cube's): the
        // edge is a cube's 1/√3, and every point has a coordinate there, on a face
        let edge = surface.iter().flatten().fold(0.0f32, |m, c| m.max(c.abs()));
        assert!((edge - 1.0 / 3f32.sqrt()).abs() < 0.01, "{edge}");
        assert!(surface.iter().all(|p| p.iter().any(|c| c.abs() > edge * 0.99)), "on the cube");
        // all six faces get some
        for axis in 0..3 {
            assert!(surface.iter().any(|p| p[axis] > edge * 0.99) && surface.iter().any(|p| p[axis] < -edge * 0.99));
        }
    }

    #[test]
    fn an_obj_with_bad_indices_loses_those_faces_not_the_model() {
        // a face past the end, a zero, a negative one back from the last, and a line of nonsense
        let text = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 99\nf 0 1 2\nf -3 -2 -1\nf x y z\nv 1 2\nv a b c\nfoo\n";
        let points = obj(text.as_bytes()).unwrap();
        assert_eq!(points.len(), SURFACE, "the one good face (-3 -2 -1) is the surface");
        assert!(obj(b"").unwrap().is_empty() && load(Path::new("no such.obj")).is_err());
        assert!(fit(obj(b"just words\n").unwrap()).is_err());
    }

    fn ply_header(format: &str, count: impl std::fmt::Display, properties: &str) -> String {
        format!("ply\nformat {format} 1.0\ncomment made here\nelement vertex {count}\n{properties}element face 1\nproperty list uchar int vertex_indices\nend_header\n")
    }

    #[test]
    fn a_ply_gives_its_vertices_ascii_or_binary() {
        let ascii = format!("{}1 2 3 255\n4 5 6 0\n7 8 9 0\n1 1 1 3 0 1 2\n", ply_header("ascii", 3, "property float x\nproperty float y\nproperty float z\nproperty uchar red\n"));
        assert_eq!(ply(ascii.as_bytes()).unwrap(), [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]);
        // binary: an extra uchar before the doubles, so the stride is 1 + 24
        let mut binary = ply_header("binary_little_endian", 2, "property uchar flag\nproperty double x\nproperty double y\nproperty double z\n").into_bytes();
        for v in [[1.5f64, -2.0, 3.0], [0.25, 0.5, 0.75]] {
            binary.push(7);
            binary.extend(v.iter().flat_map(|c| c.to_le_bytes()));
        }
        assert_eq!(ply(&binary).unwrap(), [[1.5, -2.0, 3.0], [0.25, 0.5, 0.75]]);
        // with CRLF line ends too
        let crlf = ascii.replace('\n', "\r\n");
        assert_eq!(ply(crlf.as_bytes()).unwrap().len(), 3);
    }

    #[test]
    fn a_ply_that_lies_or_is_cut_short_is_an_error_not_a_crash() {
        let floats = "property float x\nproperty float y\nproperty float z\n";
        // ten vertices where the header says a thousand, and where it says 4 billion
        for count in [1000u64, 4_000_000_000] {
            let mut cut = ply_header("binary_little_endian", count, floats).into_bytes();
            cut.extend([0u8; 120]);
            let e = ply(&cut).unwrap_err();
            assert!(e.contains("truncated"), "{e}");
            assert!(ply(ply_header("ascii", count, floats).as_bytes()).unwrap_err().contains("truncated"));
        }
        assert!(ply(b"").is_err() && ply(b"ply\nformat ascii 1.0\n").is_err(), "no header end");
        assert!(ply(ply_header("binary_big_endian", 1, floats).as_bytes()).unwrap_err().contains("big endian"));
        assert!(ply(ply_header("ascii", 1, "property float x\nproperty float y\n").as_bytes()).unwrap_err().contains("no z"));
        assert!(ply(ply_header("ascii", 1, "property list uchar int x\n").as_bytes()).is_err());
        assert!(ply(b"obj\nend_header\n").is_err(), "not a ply");
        // a bigger one than it'll keep is thinned to what it'll keep
        let mut big = ply_header("binary_little_endian", 3 * MAX_POINTS, floats).into_bytes();
        big.extend((0..3 * MAX_POINTS * 3).flat_map(|i| (i as f32).to_le_bytes()));
        assert!(ply(&big).unwrap().len() <= MAX_POINTS);
    }
}
