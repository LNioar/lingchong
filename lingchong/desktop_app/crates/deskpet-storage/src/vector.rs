//! 余弦相似度工具。embedding 以 f32 数组存储为 BLOB。

/// 将 f32 切片编码为小端 BLOB 字节。
pub fn encode(emb: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(emb.len() * 4);
    for v in emb {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    buf
}

/// 从 BLOB 字节解码 f32 切片。
pub fn decode(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// 余弦相似度。
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}
