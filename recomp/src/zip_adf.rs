use std::fs;
use std::path::Path;

/// Load an 880 KB Amiga ADF image from either a raw `.adf` file or a `.zip` archive containing one.
pub fn load_adf_from_path<P: AsRef<Path>>(path: P) -> Result<Vec<u8>, String> {
    let data = fs::read(path.as_ref()).map_err(|e| format!("Failed to read {:?}: {}", path.as_ref(), e))?;
    if data.len() >= 4 && &data[0..4] == b"PK\x03\x04" {
        extract_first_adf_from_zip(&data)
    } else if data.len() == 901_120 {
        Ok(data)
    } else {
        Err(format!("Unsupported file size {} (expected 901120-byte ADF or ZIP)", data.len()))
    }
}

fn r16(b: &[u8], o: usize) -> u16 { u16::from_le_bytes([b[o], b[o + 1]]) }
fn r32(b: &[u8], o: usize) -> u32 { u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) }

fn extract_first_adf_from_zip(zip: &[u8]) -> Result<Vec<u8>, String> {
    // Find EOCD (PK\x05\x06) scanning backwards from end
    let min_pos = zip.len().saturating_sub(65557);
    let mut eocd = None;
    let mut p = zip.len().saturating_sub(22);
    loop {
        if &zip[p..p + 4] == b"PK\x05\x06" {
            eocd = Some(p);
            break;
        }
        if p == min_pos { break; }
        p -= 1;
    }
    let eocd = eocd.ok_or("ZIP EOCD signature not found")?;
    let cd_entries = r16(zip, eocd + 10) as usize;
    let mut cd_off = r32(zip, eocd + 16) as usize;

    for _ in 0..cd_entries {
        if cd_off + 46 > zip.len() || &zip[cd_off..cd_off + 4] != b"PK\x01\x02" {
            return Err("Invalid ZIP central directory entry".into());
        }
        let method = r16(zip, cd_off + 10);
        let comp_size = r32(zip, cd_off + 20) as usize;
        let uncomp_size = r32(zip, cd_off + 24) as usize;
        let name_len = r16(zip, cd_off + 28) as usize;
        let extra_len = r16(zip, cd_off + 30) as usize;
        let comment_len = r16(zip, cd_off + 32) as usize;
        let local_off = r32(zip, cd_off + 42) as usize;
        let name = String::from_utf8_lossy(&zip[cd_off + 46..cd_off + 46 + name_len]);
        cd_off += 46 + name_len + extra_len + comment_len;

        if name.to_ascii_lowercase().ends_with(".adf") || uncomp_size == 901_120 {
            if local_off + 30 > zip.len() || &zip[local_off..local_off + 4] != b"PK\x03\x04" {
                return Err("Invalid ZIP local file header".into());
            }
            let l_name_len = r16(zip, local_off + 26) as usize;
            let l_extra_len = r16(zip, local_off + 28) as usize;
            let data_start = local_off + 30 + l_name_len + l_extra_len;
            let comp = &zip[data_start..data_start + comp_size];
            return match method {
                0 => Ok(comp.to_vec()),
                8 => inflate_raw(comp, uncomp_size),
                _ => Err(format!("Unsupported ZIP compression method {}", method)),
            };
        }
    }
    Err("No .adf entry found in ZIP".into())
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit_buf: u32,
    bit_cnt: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self { Self { data, pos: 0, bit_buf: 0, bit_cnt: 0 } }
    fn ensure(&mut self, n: u32) {
        while self.bit_cnt < n && self.pos < self.data.len() {
            self.bit_buf |= (self.data[self.pos] as u32) << self.bit_cnt;
            self.pos += 1;
            self.bit_cnt += 8;
        }
    }
    fn read_bits(&mut self, n: u32) -> u32 {
        if n == 0 { return 0; }
        self.ensure(n);
        let v = self.bit_buf & ((1u32 << n) - 1);
        self.bit_buf >>= n;
        self.bit_cnt = self.bit_cnt.saturating_sub(n);
        v
    }
    fn align_byte(&mut self) {
        let drop = self.bit_cnt & 7;
        self.bit_buf >>= drop;
        self.bit_cnt -= drop;
    }
    fn read_u16_aligned(&mut self) -> u16 {
        self.align_byte();
        let lo = self.read_bits(8) as u16;
        let hi = self.read_bits(8) as u16;
        lo | (hi << 8)
    }
}

struct Huff {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huff {
    fn from_lengths(lengths: &[u8]) -> Result<Self, String> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            if l > 15 { return Err("Invalid Huffman code length".into()); }
            if l > 0 { counts[l as usize] += 1; }
        }
        let mut offsets = [0u16; 16];
        let mut total = 0u16;
        for i in 1..16 {
            offsets[i] = total;
            total += counts[i];
        }
        let mut symbols = vec![0u16; total as usize];
        for (sym, &l) in lengths.iter().enumerate() {
            if l > 0 {
                symbols[offsets[l as usize] as usize] = sym as u16;
                offsets[l as usize] += 1;
            }
        }
        Ok(Self { counts, symbols })
    }

    fn decode(&self, br: &mut BitReader<'_>) -> Result<u16, String> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..16 {
            code |= br.read_bits(1) as i32;
            let count = self.counts[len] as i32;
            if code - count < first {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("Invalid Huffman code in deflate stream".into())
    }
}

fn inflate_raw(input: &[u8], expected_size: usize) -> Result<Vec<u8>, String> {
    const LEN_BASE: [u16; 29] = [
        3,4,5,6,7,8,9,10,11,13,15,17,19,23,27,31,35,43,51,59,67,83,99,115,131,163,195,227,258
    ];
    const LEN_EXTRA: [u32; 29] = [
        0,0,0,0,0,0,0,0,1,1,1,1,2,2,2,2,3,3,3,3,4,4,4,4,5,5,5,5,0
    ];
    const DIST_BASE: [u16; 30] = [
        1,2,3,4,5,7,9,13,17,25,33,49,65,97,129,193,257,385,513,769,
        1025,1537,2049,3073,4097,6145,8193,12289,16385,24577
    ];
    const DIST_EXTRA: [u32; 30] = [
        0,0,0,0,1,1,2,2,3,3,4,4,5,5,6,6,7,7,8,8,9,9,10,10,11,11,12,12,13,13
    ];
    const CL_ORDER: [usize; 19] = [16,17,18,0,8,7,9,6,10,5,11,4,12,3,13,2,14,1,15];

    let mut br = BitReader::new(input);
    let mut out = Vec::with_capacity(expected_size);

    loop {
        let bfinal = br.read_bits(1);
        let btype = br.read_bits(2);
        match btype {
            0 => {
                let len = br.read_u16_aligned() as usize;
                let _nlen = br.read_u16_aligned();
                for _ in 0..len {
                    out.push(br.read_bits(8) as u8);
                }
            }
            1 | 2 => {
                let (lit_huff, dist_huff) = if btype == 1 {
                    let mut lit = [0u8; 288];
                    for i in 0..=143 { lit[i] = 8; }
                    for i in 144..=255 { lit[i] = 9; }
                    for i in 256..=279 { lit[i] = 7; }
                    for i in 280..=287 { lit[i] = 8; }
                    let dist = [5u8; 32];
                    (Huff::from_lengths(&lit)?, Huff::from_lengths(&dist)?)
                } else {
                    let hlit = br.read_bits(5) as usize + 257;
                    let hdist = br.read_bits(5) as usize + 1;
                    let hclen = br.read_bits(4) as usize + 4;
                    let mut cl_lens = [0u8; 19];
                    for i in 0..hclen {
                        cl_lens[CL_ORDER[i]] = br.read_bits(3) as u8;
                    }
                    let cl_huff = Huff::from_lengths(&cl_lens)?;
                    let mut all = Vec::with_capacity(hlit + hdist);
                    while all.len() < hlit + hdist {
                        let sym = cl_huff.decode(&mut br)?;
                        match sym {
                            0..=15 => all.push(sym as u8),
                            16 => {
                                let prev = *all.last().ok_or("bad repeat")?;
                                let rep = br.read_bits(2) as usize + 3;
                                for _ in 0..rep { all.push(prev); }
                            }
                            17 => {
                                let rep = br.read_bits(3) as usize + 3;
                                for _ in 0..rep { all.push(0); }
                            }
                            18 => {
                                let rep = br.read_bits(7) as usize + 11;
                                for _ in 0..rep { all.push(0); }
                            }
                            _ => return Err("bad code length sym".into()),
                        }
                    }
                    (Huff::from_lengths(&all[..hlit])?, Huff::from_lengths(&all[hlit..hlit + hdist])?)
                };

                loop {
                    let sym = lit_huff.decode(&mut br)?;
                    if sym < 256 {
                        out.push(sym as u8);
                    } else if sym == 256 {
                        break;
                    } else {
                        let li = (sym - 257) as usize;
                        if li >= 29 { return Err("bad length code".into()); }
                        let length = LEN_BASE[li] as usize + br.read_bits(LEN_EXTRA[li]) as usize;
                        let di = dist_huff.decode(&mut br)? as usize;
                        if di >= 30 { return Err("bad dist code".into()); }
                        let dist = DIST_BASE[di] as usize + br.read_bits(DIST_EXTRA[di]) as usize;
                        if dist > out.len() { return Err("dist past start".into()); }
                        for _ in 0..length {
                            let b = out[out.len() - dist];
                            out.push(b);
                        }
                    }
                }
            }
            _ => return Err("Invalid deflate block type".into()),
        }
        if bfinal != 0 { break; }
    }
    Ok(out)
}
