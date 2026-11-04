//! Codificação da tela: divide em blocos de 64 × 64, compara com o quadro
//! anterior e envia só os blocos que mudaram, em JPEG. Quando a tela fica
//! parada, os blocos enviados com qualidade baixa são reenviados com
//! qualidade alta ("refinamento").

use crate::msg::Tile;

pub const TILE: u32 = 64;

pub struct Encoder {
    w: u32,
    h: u32,
    prev: Vec<u8>,
    /// Hash de cada bloco no último envio e a qualidade usada.
    sent_q: Vec<u8>,
}

fn tile_hash(buf: &[u8], w: u32, x0: u32, y0: u32, tw: u32, th: u32) -> u64 {
    // FNV-1a amostrando todos os pixels do bloco.
    let mut h: u64 = 0xcbf29ce484222325;
    for y in y0..y0 + th {
        let row = ((y * w + x0) * 4) as usize;
        for b in &buf[row..row + (tw * 4) as usize] {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

fn tile_eq(a: &[u8], b: &[u8], w: u32, x0: u32, y0: u32, tw: u32, th: u32) -> bool {
    (y0..y0 + th).all(|y| {
        let r = ((y * w + x0) * 4) as usize;
        let n = (tw * 4) as usize;
        a[r..r + n] == b[r..r + n]
    })
}

fn crop(buf: &[u8], w: u32, x0: u32, y0: u32, tw: u32, th: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((tw * th * 4) as usize);
    for y in y0..y0 + th {
        let r = ((y * w + x0) * 4) as usize;
        out.extend_from_slice(&buf[r..r + (tw * 4) as usize]);
    }
    out
}

pub fn jpeg(rgba: &[u8], w: u32, h: u32, q: u8) -> Vec<u8> {
    let mut out = Vec::new();
    let enc = jpeg_encoder::Encoder::new(&mut out, q.clamp(5, 98));
    let _ = enc.encode(rgba, w as u16, h as u16, jpeg_encoder::ColorType::Rgba);
    out
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Encoder {
    pub fn new() -> Self {
        Self { w: 0, h: 0, prev: Vec::new(), sent_q: Vec::new() }
    }

    fn grid(&self) -> (u32, u32) {
        (self.w.div_ceil(TILE), self.h.div_ceil(TILE))
    }

    /// Blocos alterados (ou todos, se `full` ou a resolução mudou).
    /// Devolve (blocos, quadro completo?).
    pub fn encode(&mut self, rgba: &[u8], w: u32, h: u32, quality: u8, full: bool) -> (Vec<Tile>, bool) {
        let resized = w != self.w || h != self.h || self.prev.len() != rgba.len();
        let full = full || resized;
        if resized {
            self.w = w;
            self.h = h;
            let (gx, gy) = self.grid();
            self.sent_q = vec![0; (gx * gy) as usize];
        }
        let (gx, gy) = self.grid();
        let mut tiles = Vec::new();
        for ty in 0..gy {
            for tx in 0..gx {
                let x0 = tx * TILE;
                let y0 = ty * TILE;
                let tw = TILE.min(w - x0);
                let th = TILE.min(h - y0);
                if !full && tile_eq(rgba, &self.prev, w, x0, y0, tw, th) {
                    continue;
                }
                let px = crop(rgba, w, x0, y0, tw, th);
                tiles.push(Tile { x: x0 as u16, y: y0 as u16, w: tw as u16, h: th as u16, data: jpeg(&px, tw, th, quality), raw: false });
                self.sent_q[(ty * gx + tx) as usize] = quality;
            }
        }
        self.prev.clear();
        self.prev.extend_from_slice(rgba);
        (tiles, full)
    }

    /// Reenvia em alta qualidade os blocos que foram com qualidade baixa
    /// (chamar quando a tela estiver parada). No máximo `limit` blocos.
    pub fn refine(&mut self, quality: u8, limit: usize) -> Vec<Tile> {
        let (gx, gy) = self.grid();
        let mut out = Vec::new();
        for ty in 0..gy {
            for tx in 0..gx {
                let i = (ty * gx + tx) as usize;
                if self.sent_q[i] >= quality || out.len() >= limit {
                    continue;
                }
                let x0 = tx * TILE;
                let y0 = ty * TILE;
                let tw = TILE.min(self.w - x0);
                let th = TILE.min(self.h - y0);
                let px = crop(&self.prev, self.w, x0, y0, tw, th);
                out.push(Tile { x: x0 as u16, y: y0 as u16, w: tw as u16, h: th as u16, data: jpeg(&px, tw, th, quality), raw: false });
                self.sent_q[i] = quality;
            }
        }
        out
    }

    pub fn hash_of(&self, tx: u32, ty: u32) -> u64 {
        let tw = TILE.min(self.w - tx * TILE);
        let th = TILE.min(self.h - ty * TILE);
        tile_hash(&self.prev, self.w, tx * TILE, ty * TILE, tw, th)
    }
}

/// Lado do visualizador: aplica blocos num buffer RGBA.
#[derive(Default)]
pub struct Decoder {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}


impl Decoder {
    pub fn resize(&mut self, w: u32, h: u32) {
        if w != self.w || h != self.h {
            self.w = w;
            self.h = h;
            self.rgba = vec![0; (w * h * 4) as usize];
        }
    }

    pub fn apply(&mut self, t: &Tile) -> bool {
        let (tw, th) = (t.w as u32, t.h as u32);
        if t.x as u32 + tw > self.w || t.y as u32 + th > self.h {
            return false;
        }
        let px: Vec<u8> = if t.raw {
            t.data.clone()
        } else {
            match image::load_from_memory_with_format(&t.data, image::ImageFormat::Jpeg) {
                Ok(img) => img.to_rgba8().into_raw(),
                Err(_) => return false,
            }
        };
        if px.len() < (tw * th * 4) as usize {
            return false;
        }
        for y in 0..th {
            let dst = (((t.y as u32 + y) * self.w + t.x as u32) * 4) as usize;
            let src = (y * tw * 4) as usize;
            self.rgba[dst..dst + (tw * 4) as usize].copy_from_slice(&px[src..src + (tw * 4) as usize]);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_changed_tiles_and_roundtrip() {
        let (w, h) = (200u32, 130u32);
        let mut img = vec![255u8; (w * h * 4) as usize];
        let mut enc = Encoder::new();
        let (t, full) = enc.encode(&img, w, h, 80, false);
        assert!(full);
        assert_eq!(t.len(), 4 * 3);
        let (t, _) = enc.encode(&img, w, h, 80, false);
        assert!(t.is_empty());
        // Muda um pixel no bloco (1, 1).
        let i = ((70 * w + 70) * 4) as usize;
        img[i] = 0;
        let (t, _) = enc.encode(&img, w, h, 40, false);
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].x, t[0].y), (64, 64));
        // Refinamento reenvia todos com qualidade menor que 90.
        assert_eq!(enc.refine(90, 100).len(), 12);
        assert!(enc.refine(90, 100).is_empty());
        let mut dec = Decoder::default();
        dec.resize(w, h);
        for t in enc.encode(&img, w, h, 95, true).0 {
            assert!(dec.apply(&t));
        }
        // JPEG com perdas: o branco continua quase branco.
        assert!(dec.rgba[0] > 240);
        let _ = enc.hash_of(0, 0);
    }
}
