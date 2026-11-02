//! HAP movies (Vidvox's open codec, docs/rendering.md §4): each frame is a GPU texture already
//! (BC1, BC3, BC7), usually Snappy-compressed, in a QuickTime file. Opening one reads the
//! movie's sample tables; a frame is read from the file and unpacked when it's due, and goes
//! to the GPU as it is: no decoding on the CPU, no colors converted (Hap Q's YCoCg turns to RGB
//! in the Image shader).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// How a movie's frames are packed on the GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Hap: RGB, DXT1.
    Bc1,
    /// Hap Alpha: RGBA, DXT5.
    Bc3,
    /// Hap Q: scaled YCoCg in DXT5, sharper than Hap.
    YCoCg,
    /// Hap R: RGBA, BC7.
    Bc7,
}

impl Format {
    pub fn texture(self) -> wgpu::TextureFormat {
        match self {
            Format::Bc1 => wgpu::TextureFormat::Bc1RgbaUnorm,
            Format::Bc3 | Format::YCoCg => wgpu::TextureFormat::Bc3RgbaUnorm,
            Format::Bc7 => wgpu::TextureFormat::Bc7RgbaUnorm,
        }
    }

    /// Bytes per 4×4 block.
    pub fn block(self) -> usize {
        if self == Format::Bc1 { 8 } else { 16 }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Bc1 => "Hap",
            Format::Bc3 => "Hap Alpha",
            Format::YCoCg => "Hap Q",
            Format::Bc7 => "Hap R",
        }
    }

    fn of(kind: u8) -> Result<Self, String> {
        match kind & 0x0F {
            0x0B => Ok(Format::Bc1),
            0x0E => Ok(Format::Bc3),
            0x0F => Ok(Format::YCoCg),
            0x0C => Ok(Format::Bc7),
            0x0D => Err("Hap Q Alpha isn't supported yet: export Hap Alpha or Hap Q".into()),
            0x01 => Err("Hap Alpha-Only isn't supported yet".into()),
            0x02 | 0x03 => Err("HAP HDR isn't supported yet".into()),
            other => Err(format!("unknown HAP texture format {other:#x}")),
        }
    }
}

/// A HAP movie's video track, as its sample tables give it.
#[derive(Debug)]
pub struct Movie {
    pub path: PathBuf,
    pub format: Format,
    /// Each frame in the file: offset, length.
    pub samples: Vec<(u64, u32)>,
}

impl Movie {
    /// The texture's size for a picture of `size`: whole 4×4 blocks.
    pub fn blocks(size: [u32; 2]) -> [u32; 2] {
        size.map(|v| v.div_ceil(4) * 4)
    }

    /// Frame `index` from `file` into `texture` (its blocks, reused), through `packed`.
    pub fn read(&self, file: &mut File, index: usize, packed: &mut Vec<u8>, texture: &mut Vec<u8>) -> Result<Format, String> {
        let &(offset, len) = self.samples.get(index).ok_or("no such frame")?;
        packed.resize(len as usize, 0);
        file.seek(SeekFrom::Start(offset)).and_then(|_| file.read_exact(packed)).map_err(|e| e.to_string())?;
        unpack(packed, texture)
    }
}

/// A QuickTime movie's HAP track: its size, its frames' durations, where each frame is.
pub fn open(path: &Path) -> Result<(Movie, [u32; 2], Vec<f64>), String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    // the movie atom, wherever it is (before or after the media data)
    let moov = loop {
        let mut head = [0u8; 8];
        file.read_exact(&mut head).map_err(|_| "not a QuickTime movie (no moov atom)".to_string())?;
        let mut size = u64::from(u32::from_be_bytes([head[0], head[1], head[2], head[3]]));
        let mut header = 8;
        if size == 1 {
            let mut large = [0u8; 8];
            file.read_exact(&mut large).map_err(|e| e.to_string())?;
            (size, header) = (u64::from_be_bytes(large), 16);
        }
        if size != 0 && size < header {
            return Err("a broken QuickTime atom".into());
        }
        if &head[4..] == b"moov" {
            let len = if size == 0 { file.metadata().map_err(|e| e.to_string())?.len().saturating_sub(header) } else { size - header };
            // ponytail: a movie atom over 64 MB is a broken file, not a long movie
            if len > 64 << 20 {
                return Err("the movie atom is too big".into());
            }
            let mut moov = vec![0; len as usize];
            file.read_exact(&mut moov).map_err(|e| e.to_string())?;
            break moov;
        }
        if size == 0 {
            return Err("not a QuickTime movie (no moov atom)".into());
        }
        file.seek(SeekFrom::Current((size - header) as i64)).map_err(|e| e.to_string())?;
    };
    let mut codecs = Vec::new();
    for trak in children(&moov).into_iter().filter(|a| &a.0 == b"trak").map(|a| a.1) {
        let Some(stbl) = find(trak, &[b"mdia", b"minf", b"stbl"]) else { continue };
        let Some(stsd) = find(stbl, &[b"stsd"]) else { continue };
        // the first sample description: its codec, then (a visual one) its width and height
        let codec = stsd.get(12..16).unwrap_or_default();
        if !codec.starts_with(b"Hap") {
            codecs.push(String::from_utf8_lossy(codec).trim().to_string());
            continue;
        }
        let size = [be16(stsd, 8 + 32), be16(stsd, 8 + 34)].map(|v| u32::from(v.unwrap_or(0)));
        if size.contains(&0) || size[0].max(size[1]) > 8192 {
            return Err(format!("a {}×{} movie: too big for a texture", size[0], size[1]));
        }
        let mdhd = find(trak, &[b"mdia", b"mdhd"]).ok_or("no media header")?;
        let scale = if mdhd.first() == Some(&1) { be32(mdhd, 20) } else { be32(mdhd, 12) }.filter(|&s| s > 0).ok_or("no time scale")?;
        let ends = durations(find(stbl, &[b"stts"]).ok_or("no frame durations")?, f64::from(scale))?;
        let samples = samples(stbl)?;
        if samples.is_empty() || samples.len() != ends.len() {
            return Err("the movie's frame tables don't agree".into());
        }
        // the format, from the first frame (the codec's name says it too, frames are the truth)
        let movie = Movie { path: path.to_path_buf(), format: Format::Bc1, samples };
        let format = movie.read(&mut file, 0, &mut Vec::new(), &mut Vec::new())?;
        return Ok((Movie { format, ..movie }, size, ends));
    }
    match codecs.first() {
        Some(codec) => Err(format!("not HAP but {codec}: export it as HAP (ffmpeg -i in.mov -c:v hap -format hap_q out.mov)")),
        None => Err("no video in this movie".into()),
    }
}

/// When each frame ends, in seconds, from the time-to-sample table.
fn durations(stts: &[u8], scale: f64) -> Result<Vec<f64>, String> {
    let entries = be32(stts, 4).ok_or("broken stts")? as usize;
    let (mut ends, mut t) = (Vec::new(), 0u64);
    for e in 0..entries {
        let count = be32(stts, 8 + e * 8).ok_or("broken stts")?;
        let delta = be32(stts, 12 + e * 8).ok_or("broken stts")?;
        if ends.len() + count as usize > 1 << 20 {
            return Err("more than a million frames".into());
        }
        for _ in 0..count {
            t += u64::from(delta);
            ends.push(t as f64 / scale);
        }
    }
    Ok(ends)
}

/// Each sample's offset in the file and length: chunks from stco/co64, samples per chunk from
/// stsc, sizes from stsz.
fn samples(stbl: &[u8]) -> Result<Vec<(u64, u32)>, String> {
    let stsz = find(stbl, &[b"stsz"]).ok_or("no sample sizes")?;
    let (fixed, count) = (be32(stsz, 4).ok_or("broken stsz")?, be32(stsz, 8).ok_or("broken stsz")? as usize);
    let size = |i: usize| if fixed != 0 { Some(fixed) } else { be32(stsz, 12 + i * 4) };
    let chunks: Vec<u64> = if let Some(stco) = find(stbl, &[b"stco"]) {
        (0..be32(stco, 4).ok_or("broken stco")? as usize).map(|i| be32(stco, 8 + i * 4).map(u64::from)).collect::<Option<_>>().ok_or("broken stco")?
    } else {
        let co64 = find(stbl, &[b"co64"]).ok_or("no chunk offsets")?;
        (0..be32(co64, 4).ok_or("broken co64")? as usize).map(|i| be64(co64, 8 + i * 8)).collect::<Option<_>>().ok_or("broken co64")?
    };
    let stsc = find(stbl, &[b"stsc"]).ok_or("no sample-to-chunk table")?;
    let runs: Vec<(u32, u32)> = (0..be32(stsc, 4).ok_or("broken stsc")? as usize).map(|i| Some((be32(stsc, 8 + i * 12)?, be32(stsc, 12 + i * 12)?))).collect::<Option<_>>().ok_or("broken stsc")?;
    let mut out = Vec::with_capacity(count);
    for (c, &start) in chunks.iter().enumerate() {
        // chunks are numbered from 1: the last run starting at or before this one
        let per = runs.iter().rev().find(|r| r.0 as usize <= c + 1).map_or(1, |r| r.1);
        let mut offset = start;
        for _ in 0..per {
            if out.len() == count {
                break;
            }
            let len = size(out.len()).ok_or("broken stsz")?;
            out.push((offset, len));
            offset += u64::from(len);
        }
    }
    Ok(out)
}

/// A frame's bytes into its texture's blocks (`out`, reused): one section, stored as it is,
/// Snappy-compressed, or in chunks each stored or compressed (the decode instructions).
pub fn unpack(frame: &[u8], out: &mut Vec<u8>) -> Result<Format, String> {
    let (kind, body, _) = section(frame)?;
    let format = Format::of(kind)?;
    out.clear();
    match kind >> 4 {
        0xA => out.extend_from_slice(body),
        0xB => snappy(body, out)?,
        0xC => {
            let (container, tables, used) = section(body)?;
            if container != 0x01 {
                return Err("no decode instructions".into());
            }
            let (mut compressors, mut sizes, mut offsets): (&[u8], &[u8], Option<&[u8]>) = (&[], &[], None);
            let mut rest = tables;
            while !rest.is_empty() {
                let (kind, table, used) = section(rest)?;
                match kind {
                    0x02 => compressors = table,
                    0x03 => sizes = table,
                    0x04 => offsets = Some(table),
                    _ => {}
                }
                rest = &rest[used..];
            }
            let data = &body[used..];
            let le = |t: &[u8], i: usize| t.get(i * 4..i * 4 + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
            let mut at = 0;
            for (i, &compressor) in compressors.iter().enumerate() {
                let size = le(sizes, i).ok_or("broken chunk sizes")?;
                let start = offsets.map_or(Some(at), |o| le(o, i)).ok_or("broken chunk offsets")?;
                let chunk = data.get(start..start + size).ok_or("a chunk past the frame's end")?;
                match compressor {
                    0x0A => out.extend_from_slice(chunk),
                    0x0B => snappy(chunk, out)?,
                    other => return Err(format!("unknown chunk compressor {other:#x}")),
                }
                at = start + size;
            }
        }
        other => return Err(format!("unknown HAP compressor {other:#x}")),
    }
    Ok(format)
}

/// Snappy-compressed `input`, unpacked at the end of `out`.
fn snappy(input: &[u8], out: &mut Vec<u8>) -> Result<(), String> {
    let len = snap::raw::decompress_len(input).map_err(|e| e.to_string())?;
    let start = out.len();
    out.resize(start + len, 0);
    snap::raw::Decoder::new().decompress(input, &mut out[start..]).map_err(|e| e.to_string())?;
    Ok(())
}

/// A HAP section: its type, its bytes, and how many bytes it took with its header (a 3-byte
/// little-endian size, or 0 there and 4 bytes of size after the type).
fn section(d: &[u8]) -> Result<(u8, &[u8], usize), String> {
    let short = d.get(..4).ok_or("a section cut short")?;
    let (len, head) = match u32::from_le_bytes([short[0], short[1], short[2], 0]) {
        0 => (d.get(4..8).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or("a section cut short")? as usize, 8),
        n => (n as usize, 4),
    };
    let body = d.get(head..head + len).ok_or("a section past the frame's end")?;
    Ok((short[3], body, head + len))
}

/// QuickTime atoms in `data`: their types and contents.
fn children(data: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let (mut out, mut i) = (Vec::new(), 0);
    while let (Some(size), Some(kind)) = (be32(data, i), data.get(i + 4..i + 8)) {
        let (head, size) = match size {
            1 => (16, be64(data, i + 8).unwrap_or(0) as usize),
            0 => (8, data.len() - i),
            s => (8, s as usize),
        };
        let Some(body) = data.get(i + head..i + size).filter(|_| size >= head) else { break };
        out.push(([kind[0], kind[1], kind[2], kind[3]], body));
        i += size;
    }
    out
}

/// The atom down `path` in `data`, its first match at each level.
fn find<'a>(data: &'a [u8], path: &[&[u8; 4]]) -> Option<&'a [u8]> {
    path.iter().try_fold(data, |d, kind| children(d).into_iter().find(|a| &a.0 == *kind).map(|a| a.1))
}

fn be16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn be64(d: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(d.get(at..at + 8)?.try_into().ok()?))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Four frames of 32×16 at 10 fps, made with ffmpeg: red | green over blue, then green,
    /// blue, white.
    pub fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)
    }

    #[test]
    fn each_kind_of_hap_opens_and_unpacks() {
        for (name, format) in [("hap1.mov", Format::Bc1), ("hap5-chunks.mov", Format::Bc3), ("hapq-none.mov", Format::YCoCg)] {
            let (movie, size, ends) = open(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!((movie.format, size, movie.samples.len()), (format, [32, 16], 4), "{name}");
            assert!((ends[3] - 0.4).abs() < 1e-6 && (ends[0] - 0.1).abs() < 1e-6, "{name}: 10 fps, {ends:?}");
            let mut file = File::open(&movie.path).unwrap();
            let (mut packed, mut texture) = (Vec::new(), Vec::new());
            for frame in 0..4 {
                movie.read(&mut file, frame, &mut packed, &mut texture).unwrap_or_else(|e| panic!("{name} {frame}: {e}"));
                assert_eq!(texture.len(), 8 * 4 * format.block(), "{name} {frame}: 8×4 blocks");
            }
        }
    }

    #[test]
    fn what_isnt_hap_says_what_it_is() {
        let dir = std::env::temp_dir().join(format!("velo-hap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("junk.mov"), b"\0\0\0\x10junkjunkjunk").unwrap();
        assert!(open(&dir.join("junk.mov")).unwrap_err().contains("moov"));
        // sections cut short or past the end are errors, not panics
        assert!(unpack(&[0x10, 0, 0, 0xBB], &mut Vec::new()).is_err());
        assert!(unpack(&[1, 0, 0, 0x0D, 0], &mut Vec::new()).unwrap_err().contains("Hap Q Alpha"));
        let mut out = Vec::new();
        assert_eq!(unpack(&[2, 0, 0, 0xAB, 7, 9], &mut out), Ok(Format::Bc1));
        assert_eq!(out, [7, 9]);
    }
}
