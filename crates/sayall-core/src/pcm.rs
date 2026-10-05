/// 用户可设置增益的单一钳制口径（0–24 dB；非有限值 → 0 dB）。
///
/// 设置持久化（`sayall-core::AppSettings`）、解码管道
/// （`AtvvVoicePipeline::set_gain_db`）与平台推送共用它：同一取值从磁盘、
/// IPC 或运行期推送进入任何一层，语义都一致。注意 `process_pcm` 自身仍
/// 接受 -24–24 dB（对齐 Mac `PCMPostprocessor` 的内部范围），两者不冲突。
pub fn normalize_gain_db(gain_db: f32) -> f32 {
    if gain_db.is_finite() {
        gain_db.clamp(0.0, 24.0)
    } else {
        0.0
    }
}

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

    #[test]
    fn normalizes_the_user_facing_gain_range() {
        assert_eq!(normalize_gain_db(f32::NAN), 0.0);
        assert_eq!(normalize_gain_db(f32::INFINITY), 0.0);
        assert_eq!(normalize_gain_db(-3.0), 0.0);
        assert_eq!(normalize_gain_db(30.0), 24.0);
        assert_eq!(normalize_gain_db(12.0), 12.0);
    }
}
