//! Allocation file: un bit por bloque de asignación, el más significativo primero.

#[derive(Debug, Clone)]
pub struct AllocationBitmap {
    bits: Vec<u8>,
    total: u32,
}

impl AllocationBitmap {
    pub fn from_bytes(bits: Vec<u8>, total: u32) -> Self {
        Self { bits, total }
    }

    pub fn new_empty(total: u32) -> Self {
        Self { bits: vec![0; (total as usize).div_ceil(8)], total }
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn is_set(&self, block: u32) -> bool {
        self.bits.get(block as usize / 8).map(|b| b & (0x80 >> (block % 8)) != 0).unwrap_or(false)
    }

    pub fn set(&mut self, block: u32) {
        if let Some(b) = self.bits.get_mut(block as usize / 8) {
            *b |= 0x80 >> (block % 8);
        }
    }

    pub fn set_range(&mut self, start: u32, count: u32) {
        for b in start..start.saturating_add(count).min(self.total) {
            self.set(b);
        }
    }

    pub fn count_set(&self) -> u32 {
        (0..self.total).filter(|&b| self.is_set(b)).count() as u32
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bits
    }
}
