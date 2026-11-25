//! Shared read-only IPC wire protocol between `textree.exe` and `textree-service.exe`.
//!
//! Pure Rust with zero GUI dependencies so `textree-service.exe` stays tiny and clean.

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

#[allow(dead_code)]
pub const SERVICE_NAME: &str = "TexTreeMftService";
#[allow(dead_code)]
pub const SERVICE_DISPLAY_NAME: &str = "TexTree Read-Only MFT Scanner Service";
#[allow(dead_code)]
pub const SERVICE_DESCRIPTION: &str = "Provides fast, read-only NTFS Master File Table scanning and file-lock inspection for TexTree without UAC prompts.";
#[allow(dead_code)]
pub const PIPE_NAME: &str = r"\\.\pipe\TexTreeMftServiceV1";
#[allow(dead_code)]
pub const PROTOCOL_VERSION: u32 = 1;

// Raw bitflag masks mirroring `crate::models::NodeFlags` bit-for-bit.
#[allow(dead_code)]
pub const FLAG_IS_DIR: u16 = 1 << 0;
#[allow(dead_code)]
pub const FLAG_IS_VOLUME: u16 = 1 << 1;
#[allow(dead_code)]
pub const FLAG_IS_VIRTUAL: u16 = 1 << 2;
#[allow(dead_code)]
pub const FLAG_ACCESS_DENIED: u16 = 1 << 3;
#[allow(dead_code)]
pub const FLAG_IS_SYMLINK: u16 = 1 << 4;
#[allow(dead_code)]
pub const FLAG_IS_HIDDEN: u16 = 1 << 5;
#[allow(dead_code)]
pub const FLAG_IS_SYSTEM: u16 = 1 << 6;
#[allow(dead_code)]
pub const FLAG_IS_READONLY: u16 = 1 << 7;
#[allow(dead_code)]
pub const FLAG_IS_COMPRESSED: u16 = 1 << 8;

pub const FRAME_JSON_REQ: u8 = 1;
pub const FRAME_JSON_MSG: u8 = 2;
pub const FRAME_BINARY_TREE: u8 = 3;

const MAX_JSON_FRAME_BYTES: usize = 8 * 1024 * 1024;
const MAX_BINARY_TREE_BYTES: usize = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceRequest {
    Ping,
    ScanVolume { drive_letter: char },
    InspectFileLock { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireLockProcess {
    pub pid: u32,
    pub name: String,
    pub app_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireLockStatus {
    NotApplicable,
    Unlocked,
    LockedBy(Vec<WireLockProcess>),
    LockedUnknown,
    AccessDenied,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceMessage {
    Pong {
        protocol_version: u32,
        service_version: String,
    },
    Progress {
        scanned_files: u64,
        scanned_dirs: u64,
        scanned_bytes: u64,
        percent: u8,
    },
    LockResult(WireLockStatus),
    Error(String),
}

/// Flat DFS-ordered node emitted by the MFT engine.
/// Root is always index `0` with `parent == u32::MAX`, and for every node `i > 0`, `parent < i`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireNode {
    pub parent: u32,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub modified_secs: i64,
    pub flags: u16,
    pub name: String,
}

#[allow(dead_code)]
pub fn write_json_frame<W: Write, T: Serialize>(
    writer: &mut W,
    kind: u8,
    value: &T,
) -> Result<(), String> {
    let payload =
        serde_json::to_vec(value).map_err(|e| format!("Failed serializing IPC frame: {e}"))?;
    write_raw_frame(writer, kind, &payload)
}

#[allow(dead_code)]
pub fn write_binary_tree_frame<W: Write>(
    writer: &mut W,
    nodes: &[WireNode],
) -> Result<(), String> {
    let payload = encode_wire_nodes(nodes);
    write_raw_frame(writer, FRAME_BINARY_TREE, &payload)
}

#[allow(dead_code)]
pub fn write_raw_frame<W: Write>(writer: &mut W, kind: u8, payload: &[u8]) -> Result<(), String> {
    let len = u32::try_from(payload.len())
        .map_err(|_| "IPC payload exceeded 4 GiB frame limit.".to_string())?;
    let mut hdr = [0u8; 5];
    hdr[0] = kind;
    hdr[1..5].copy_from_slice(&len.to_le_bytes());
    writer
        .write_all(&hdr)
        .map_err(|e| format!("Failed writing IPC header: {e}"))?;
    writer
        .write_all(payload)
        .map_err(|e| format!("Failed writing IPC payload: {e}"))?;
    writer
        .flush()
        .map_err(|e| format!("Failed flushing IPC stream: {e}"))?;
    Ok(())
}

#[allow(dead_code)]
pub fn read_frame<R: Read>(reader: &mut R) -> Result<(u8, Vec<u8>), String> {
    let mut hdr = [0u8; 5];
    reader
        .read_exact(&mut hdr)
        .map_err(|e| format!("Failed reading IPC header: {e}"))?;
    let kind = hdr[0];
    let len = u32::from_le_bytes([hdr[1], hdr[2], hdr[3], hdr[4]]) as usize;
    let max_allowed = if kind == FRAME_BINARY_TREE {
        MAX_BINARY_TREE_BYTES
    } else {
        MAX_JSON_FRAME_BYTES
    };
    if len > max_allowed {
        return Err(format!("IPC frame too large ({len} bytes)."));
    }
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .map_err(|e| format!("Failed reading IPC payload: {e}"))?;
    Ok((kind, buf))
}

#[allow(dead_code)]
pub fn encode_wire_nodes(nodes: &[WireNode]) -> Vec<u8> {
    let est_bytes = 4 + nodes.len().saturating_mul(48);
    let mut out = Vec::with_capacity(est_bytes);
    out.extend_from_slice(&(nodes.len() as u32).to_le_bytes());
    for n in nodes {
        out.extend_from_slice(&n.parent.to_le_bytes());
        out.extend_from_slice(&n.logical_size.to_le_bytes());
        out.extend_from_slice(&n.allocated_size.to_le_bytes());
        out.extend_from_slice(&n.modified_secs.to_le_bytes());
        out.extend_from_slice(&n.flags.to_le_bytes());
        let name_bytes = n.name.as_bytes();
        let name_len = name_bytes.len().min(u16::MAX as usize);
        out.extend_from_slice(&(name_len as u16).to_le_bytes());
        out.extend_from_slice(&name_bytes[..name_len]);
    }
    out
}

#[allow(dead_code)]
pub fn decode_wire_nodes(buf: &[u8]) -> Result<Vec<WireNode>, String> {
    if buf.len() < 4 {
        return Err("Truncated binary volume tree payload.".to_string());
    }
    let count = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let mut pos = 4usize;
    let mut nodes = Vec::with_capacity(count.min(4_000_000));

    for _ in 0..count {
        if pos + 32 > buf.len() {
            return Err("Truncated wire node header.".to_string());
        }
        let parent = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]);
        let logical_size = u64::from_le_bytes([
            buf[pos + 4],
            buf[pos + 5],
            buf[pos + 6],
            buf[pos + 7],
            buf[pos + 8],
            buf[pos + 9],
            buf[pos + 10],
            buf[pos + 11],
        ]);
        let allocated_size = u64::from_le_bytes([
            buf[pos + 12],
            buf[pos + 13],
            buf[pos + 14],
            buf[pos + 15],
            buf[pos + 16],
            buf[pos + 17],
            buf[pos + 18],
            buf[pos + 19],
        ]);
        let modified_secs = i64::from_le_bytes([
            buf[pos + 20],
            buf[pos + 21],
            buf[pos + 22],
            buf[pos + 23],
            buf[pos + 24],
            buf[pos + 25],
            buf[pos + 26],
            buf[pos + 27],
        ]);
        let flags = u16::from_le_bytes([buf[pos + 28], buf[pos + 29]]);
        let name_len = u16::from_le_bytes([buf[pos + 30], buf[pos + 31]]) as usize;
        pos += 32;

        if pos + name_len > buf.len() {
            return Err("Truncated wire node name.".to_string());
        }
        let name = String::from_utf8_lossy(&buf[pos..pos + name_len]).into_owned();
        pos += name_len;

        nodes.push(WireNode {
            parent,
            logical_size,
            allocated_size,
            modified_secs,
            flags,
            name,
        });
    }

    Ok(nodes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrips_json_and_binary_wire_frames() {
        let mut wire = Vec::new();
        let req = ServiceRequest::ScanVolume { drive_letter: 'E' };
        write_json_frame(&mut wire, FRAME_JSON_REQ, &req).unwrap();

        let nodes = vec![
            WireNode {
                parent: u32::MAX,
                logical_size: 0,
                allocated_size: 0,
                modified_secs: 1_700_000_000,
                flags: FLAG_IS_DIR | FLAG_IS_VOLUME,
                name: "E:\\".to_string(),
            },
            WireNode {
                parent: 0,
                logical_size: 0,
                allocated_size: 0,
                modified_secs: 1_700_000_001,
                flags: FLAG_IS_DIR,
                name: "Vídeos".to_string(),
            },
            WireNode {
                parent: 1,
                logical_size: 987_654_321,
                allocated_size: 987_656_192,
                modified_secs: 1_700_000_002,
                flags: FLAG_IS_HIDDEN | FLAG_IS_READONLY,
                name: "clip_4k.mkv".to_string(),
            },
        ];
        write_binary_tree_frame(&mut wire, &nodes).unwrap();

        let mut cur = Cursor::new(wire);
        let (k1, p1) = read_frame(&mut cur).unwrap();
        assert_eq!(k1, FRAME_JSON_REQ);
        let decoded_req: ServiceRequest = serde_json::from_slice(&p1).unwrap();
        assert_eq!(decoded_req, req);

        let (k2, p2) = read_frame(&mut cur).unwrap();
        assert_eq!(k2, FRAME_BINARY_TREE);
        let decoded_nodes = decode_wire_nodes(&p2).unwrap();
        assert_eq!(decoded_nodes, nodes);
    }
}
