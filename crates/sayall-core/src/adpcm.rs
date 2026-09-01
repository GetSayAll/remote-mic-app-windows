#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NibbleOrder {
    HighFirst,
    LowFirst,
}

#[derive(Debug, Clone)]
pub struct ImaAdpcmDecoder {
    predictor: i32,
    step_index: usize,
    order: NibbleOrder,
}

impl Default for ImaAdpcmDecoder {
    fn default() -> Self {
        Self::new(NibbleOrder::HighFirst)
    }
}

impl ImaAdpcmDecoder {
    const STEP_TABLE: [i32; 89] = [
        7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60,
        66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371,
        408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878,
        2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845,
        8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086,
        29794, 32767,
    ];
    const INDEX_TABLE: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

    pub fn new(order: NibbleOrder) -> Self {
        Self {
            predictor: 0,
            step_index: 0,
            order,
        }
    }

    pub fn reset(&mut self, predictor: i32, step_index: i32) {
        self.predictor = predictor.clamp(i16::MIN as i32, i16::MAX as i32);
        self.step_index = step_index.clamp(0, 88) as usize;
    }

    pub fn predictor(&self) -> i32 {
        self.predictor
    }

    pub fn step_index(&self) -> usize {
        self.step_index
    }

    pub fn decode(&mut self, bytes: &[u8]) -> Vec<i16> {
        let mut samples = Vec::with_capacity(bytes.len() * 2);
        for byte in bytes {
            let nibbles = match self.order {
                NibbleOrder::HighFirst => [byte >> 4, byte & 0x0F],
                NibbleOrder::LowFirst => [byte & 0x0F, byte >> 4],
            };
            for nibble in nibbles {
                samples.push(self.decode_nibble(nibble as usize));
            }
        }
        samples
    }

    fn decode_nibble(&mut self, nibble: usize) -> i16 {
        let step = Self::STEP_TABLE[self.step_index];
        let mut difference = step >> 3;
        if nibble & 1 != 0 {
            difference += step >> 2;
        }
        if nibble & 2 != 0 {
            difference += step >> 1;
        }
        if nibble & 4 != 0 {
            difference += step;
        }

        if nibble & 8 != 0 {
            self.predictor -= difference;
        } else {
            self.predictor += difference;
        }
        self.predictor = self.predictor.clamp(i16::MIN as i32, i16::MAX as i32);

        let next_index = self.step_index as i32 + Self::INDEX_TABLE[nibble & 7];
        self.step_index = next_index.clamp(0, 88) as usize;
        self.predictor as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_rc003_high_nibble_first() {
        let mut decoder = ImaAdpcmDecoder::default();
        assert_eq!(decoder.decode(&[0x11]), vec![1, 2]);
        decoder.reset(0, 0);
        assert_eq!(decoder.decode(&[0x7F]), vec![11, -19]);
    }

    #[test]
    fn matches_synthetic_golden_fixture() {
        let mut decoder = ImaAdpcmDecoder::default();
        assert_eq!(
            decoder.decode(&[0x00, 0x7F, 0x80, 0xFF]),
            vec![0, 0, 11, -19, -23, -20, -72, -184]
        );
    }

    #[test]
    fn clamps_decoder_state() {
        let mut decoder = ImaAdpcmDecoder::default();
        decoder.reset(100_000, 1_000);
        assert_eq!(decoder.predictor(), i16::MAX as i32);
        assert_eq!(decoder.step_index(), 88);
    }
}
