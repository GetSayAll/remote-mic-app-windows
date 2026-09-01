#[derive(Debug, Default, Clone)]
pub struct FrameAccumulator {
    pending: Vec<u8>,
}

impl FrameAccumulator {
    pub fn pending(&self) -> &[u8] {
        &self.pending
    }

    pub fn append(&mut self, bytes: &[u8], frame_size: usize) -> Vec<Vec<u8>> {
        if frame_size == 0 {
            return Vec::new();
        }
        self.pending.extend_from_slice(bytes);
        let complete_frames = self.pending.len() / frame_size;
        let mut frames = Vec::with_capacity(complete_frames);
        for _ in 0..complete_frames {
            frames.push(self.pending.drain(..frame_size).collect());
        }
        frames
    }

    pub fn reset(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_partial_data() {
        let mut accumulator = FrameAccumulator::default();
        assert!(accumulator.append(&[1, 2], 3).is_empty());
        assert_eq!(accumulator.pending(), &[1, 2]);
        assert_eq!(
            accumulator.append(&[3, 4, 5, 6, 7], 3),
            vec![vec![1, 2, 3], vec![4, 5, 6]]
        );
        assert_eq!(accumulator.pending(), &[7]);
    }
}
