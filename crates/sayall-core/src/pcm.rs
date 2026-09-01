pub fn process_pcm(input: &[i16], gain_db: f32) -> Vec<i16> {
    if input.is_empty() {
        return Vec::new();
    }

    let mut filtered: Vec<i32> = input.iter().map(|sample| i32::from(*sample)).collect();
    if input.len() >= 3 {
        for index in 1..(input.len() - 1) {
            filtered[index] = (i32::from(input[index - 1])
                + 2 * i32::from(input[index])
                + i32::from(input[index + 1]))
                >> 2;
        }
    }

    let safe_gain_db = if gain_db.is_finite() {
        gain_db.clamp(-24.0, 24.0)
    } else {
        0.0
    };
    let gain = 10_f32.powf(safe_gain_db / 20.0);
    filtered
        .into_iter()
        .map(|sample| {
            ((sample as f32 * gain).round() as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooths_and_applies_finite_gain() {
        assert_eq!(process_pcm(&[0, 100, 0], 0.0), vec![0, 50, 0]);
        assert_eq!(process_pcm(&[100], 6.0206), vec![200]);
    }

    #[test]
    fn clamps_invalid_gain_and_samples() {
        assert_eq!(process_pcm(&[100], f32::NAN), vec![100]);
        assert_eq!(process_pcm(&[i16::MAX], 24.0), vec![i16::MAX]);
    }
}
