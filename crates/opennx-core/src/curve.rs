use std::collections::HashMap;

use crate::config::FanCurve;

/// Evaluates a piecewise-linear [`FanCurve`] at `temp_c`, clamping the resulting duty to `0..=100`.
pub fn evaluate_curve(curve: &FanCurve, temp_c: f32) -> u8 {
    if curve.points.is_empty() || !temp_c.is_finite() {
        return 0;
    }

    let mut points: Vec<_> = curve
        .points
        .iter()
        .filter(|p| p.temp_c.is_finite())
        .cloned()
        .collect();
    if points.is_empty() {
        return 0;
    }

    points.sort_by(|a, b| a.temp_c.total_cmp(&b.temp_c));

    let first = &points[0];
    if temp_c <= first.temp_c {
        return first.duty.min(100);
    }

    let last = &points[points.len() - 1];
    if temp_c >= last.temp_c {
        return last.duty.min(100);
    }

    for window in points.windows(2) {
        let p0 = &window[0];
        let p1 = &window[1];
        if temp_c >= p0.temp_c && temp_c <= p1.temp_c {
            let span = p1.temp_c - p0.temp_c;
            if span <= f32::EPSILON {
                return p1.duty.min(100);
            }
            let ratio = (temp_c - p0.temp_c) / span;
            let d0 = f32::from(p0.duty.min(100));
            let d1 = f32::from(p1.duty.min(100));
            let interpolated = d0 + ratio * (d1 - d0);
            return interpolated.round().clamp(0.0, 100.0) as u8;
        }
    }

    last.duty.min(100)
}

#[derive(Debug, Clone, Copy)]
struct ChannelState {
    last_temp_c: f32,
    last_duty: u8,
}

/// Filters rapid duty-cycle fluctuations using temperature hysteresis and minimum duty delta.
#[derive(Debug, Clone)]
pub struct HysteresisController {
    hysteresis_c: f32,
    min_duty_delta: u8,
    last_applied: HashMap<String, ChannelState>,
}

impl HysteresisController {
    /// Creates a new [`HysteresisController`] with temperature deadband `hysteresis_c` and `min_duty_delta`.
    pub fn new(hysteresis_c: f32, min_duty_delta: u8) -> Self {
        Self {
            hysteresis_c: if hysteresis_c.is_finite() && hysteresis_c > 0.0 {
                hysteresis_c
            } else {
                0.0
            },
            min_duty_delta,
            last_applied: HashMap::new(),
        }
    }

    /// Returns `true` when `new_duty` should be applied for `channel_key` given current `temp_c`.
    ///
    /// The first observation for a channel is always applied. Subsequent updates require both a
    /// duty change of at least `min_duty_delta` and a temperature move of at least `hysteresis_c`
    /// from the last applied state.
    pub fn should_apply(&mut self, channel_key: &str, temp_c: f32, new_duty: u8) -> bool {
        if !temp_c.is_finite() {
            return false;
        }

        let clamped_duty = new_duty.min(100);

        let Some(prev) = self.last_applied.get(channel_key).copied() else {
            self.last_applied.insert(
                channel_key.to_string(),
                ChannelState {
                    last_temp_c: temp_c,
                    last_duty: clamped_duty,
                },
            );
            return true;
        };

        if clamped_duty == prev.last_duty {
            return false;
        }

        let duty_delta = clamped_duty.abs_diff(prev.last_duty);
        let temp_delta = (temp_c - prev.last_temp_c).abs();

        if duty_delta < self.min_duty_delta || temp_delta < self.hysteresis_c {
            return false;
        }

        self.last_applied.insert(
            channel_key.to_string(),
            ChannelState {
                last_temp_c: temp_c,
                last_duty: clamped_duty,
            },
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CurvePoint;

    fn sample_curve() -> FanCurve {
        FanCurve {
            points: vec![
                CurvePoint {
                    temp_c: 30.0,
                    duty: 20,
                },
                CurvePoint {
                    temp_c: 50.0,
                    duty: 60,
                },
                CurvePoint {
                    temp_c: 70.0,
                    duty: 100,
                },
            ],
        }
    }

    #[test]
    fn evaluate_curve_clamps_endpoints_and_interpolates() {
        let curve = sample_curve();
        assert_eq!(evaluate_curve(&curve, 10.0), 20);
        assert_eq!(evaluate_curve(&curve, 30.0), 20);
        assert_eq!(evaluate_curve(&curve, 40.0), 40);
        assert_eq!(evaluate_curve(&curve, 50.0), 60);
        assert_eq!(evaluate_curve(&curve, 60.0), 80);
        assert_eq!(evaluate_curve(&curve, 70.0), 100);
        assert_eq!(evaluate_curve(&curve, 95.0), 100);
    }

    #[test]
    fn evaluate_curve_handles_unsorted_points_and_clamps_over_100() {
        let curve = FanCurve {
            points: vec![
                CurvePoint {
                    temp_c: 60.0,
                    duty: 120,
                },
                CurvePoint {
                    temp_c: 20.0,
                    duty: 10,
                },
            ],
        };
        assert_eq!(evaluate_curve(&curve, 20.0), 10);
        assert_eq!(evaluate_curve(&curve, 40.0), 55);
        assert_eq!(evaluate_curve(&curve, 60.0), 100);
    }

    #[test]
    fn evaluate_curve_single_point_and_empty() {
        let empty = FanCurve { points: vec![] };
        assert_eq!(evaluate_curve(&empty, 45.0), 0);

        let single = FanCurve {
            points: vec![CurvePoint {
                temp_c: 40.0,
                duty: 55,
            }],
        };
        assert_eq!(evaluate_curve(&single, 20.0), 55);
        assert_eq!(evaluate_curve(&single, 80.0), 55);
    }

    #[test]
    fn hysteresis_applies_first_reading_then_enforces_deadbands() {
        let mut ctrl = HysteresisController::new(2.0, 3);

        // First observation always applies.
        assert!(ctrl.should_apply("smart:fan1", 35.0, 40));
        // Identical duty does not apply.
        assert!(!ctrl.should_apply("smart:fan1", 40.0, 40));
        // Temp delta < hysteresis_c (1.5 < 2.0) suppressed even with large duty delta.
        assert!(!ctrl.should_apply("smart:fan1", 36.5, 50));
        // Duty delta < min_duty_delta (2 < 3) suppressed even with large temp delta.
        assert!(!ctrl.should_apply("smart:fan1", 38.0, 42));
        // Both thresholds satisfied -> applies and updates anchor to (38.0, 45).
        assert!(ctrl.should_apply("smart:fan1", 38.0, 45));
        // Small cooling drop from new anchor (38.0 -> 36.5) is suppressed.
        assert!(!ctrl.should_apply("smart:fan1", 36.5, 40));
        // Larger cooling drop (38.0 -> 35.5) with duty delta 5 applies.
        assert!(ctrl.should_apply("smart:fan1", 35.5, 40));
        // Independent channel tracks its own state.
        assert!(ctrl.should_apply("smart:fan2", 35.5, 40));
    }
}
