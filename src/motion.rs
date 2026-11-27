//! Motion utilities for the native UI experiments.
use std::time::{Duration, Instant};

// Shared wheel tuning: a slower follow-through and a stronger, bounded edge impulse.
pub const SCROLL_OMEGA: f64 = 12.0;
pub const EDGE_GAIN: f64 = 24.0;
pub const EDGE_VELOCITY: f64 = 2800.0;

#[derive(Debug)]
pub struct WheelScroll {
    pub target: f64,
    velocity: f64,
    last: Instant,
}

impl WheelScroll {
    pub fn new(position: f32) -> Self {
        Self {
            target: position as f64,
            velocity: 0.0,
            last: Instant::now(),
        }
    }
    pub fn push(&mut self, delta: f32, maximum: f32) {
        let requested = self.target + delta as f64;
        self.target = requested.clamp(0.0, maximum.max(0.0) as f64);
        if requested != self.target {
            self.velocity = (self.velocity + (requested - self.target) * EDGE_GAIN)
                .clamp(-EDGE_VELOCITY, EDGE_VELOCITY);
        }
    }
    pub fn advance(&mut self, position: &mut f32, now: Instant) -> bool {
        let dt = now
            .saturating_duration_since(self.last)
            .as_secs_f64()
            .min(0.064);
        self.last = now;
        let offset = *position as f64 - self.target;
        let c = self.velocity + SCROLL_OMEGA * offset;
        let decay = (-SCROLL_OMEGA * dt).exp();
        let value = self.target + (offset + c * dt) * decay;
        self.velocity = (self.velocity - SCROLL_OMEGA * c * dt) * decay;
        *position = value as f32;
        let epsilon = 0.03_f64.max(8.0 * f32::EPSILON as f64 * self.target.abs());
        if (value - self.target).abs() < epsilon && self.velocity.abs() < 0.5 {
            *position = self.target as f32;
            false
        } else {
            true
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    pub stiffness: f32,
    pub damping: f32,
    pub epsilon: f32,
}

impl Spring {
    pub const fn new(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            stiffness: 300.0,
            damping: 28.0,
            epsilon: 0.002,
        }
    }

    pub fn retarget(&mut self, target: f32) {
        self.target = target;
    }

    fn epsilon(&self) -> f32 {
        let scale = self.value.abs().max(self.target.abs()).max(1.0);
        self.epsilon.max(8.0 * f32::EPSILON * scale)
    }

    pub fn is_active(&self) -> bool {
        (self.value - self.target).abs() > self.epsilon()
    }

    pub fn advance(&mut self, dt: f32) -> bool {
        let dt = dt.clamp(0.0, 0.064);
        let steps = (dt / (1.0 / 240.0)).ceil() as usize;
        let step = dt / steps.max(1) as f32;

        for _ in 0..steps {
            let acceleration =
                self.stiffness * (self.target - self.value) - self.damping * self.velocity;
            self.velocity += acceleration * step;
            self.value += self.velocity * step;
        }

        if !self.is_active() {
            self.value = self.target;
            self.velocity = 0.0;
            false
        } else {
            true
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Tween {
    pub value: f32,
    pub from: f32,
    pub to: f32,
    pub started: Instant,
    pub duration: Duration,
}

impl Tween {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            from: value,
            to: value,
            started: Instant::now(),
            duration: Duration::ZERO,
        }
    }

    pub fn retarget(&mut self, to: f32, duration: Duration) {
        self.from = self.value;
        self.to = to;
        self.started = Instant::now();
        self.duration = duration;
        if duration.is_zero() {
            self.value = to;
        }
    }

    pub fn start(&mut self, to: f32, duration: Duration) {
        self.from = 0.0;
        self.value = 0.0;
        self.retarget(to, duration);
    }

    pub fn is_active(&self, _now: Instant) -> bool {
        // Keep the final frame subscribed until advance commits the target value.
        !self.duration.is_zero() && (self.value - self.to).abs() > f32::EPSILON
    }

    pub fn advance(&mut self, now: Instant) -> bool {
        if self.duration.is_zero() {
            self.value = self.to;
            return false;
        }

        let t = (now.duration_since(self.started).as_secs_f32() / self.duration.as_secs_f32())
            .clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        self.value = self.from + (self.to - self.from) * eased;

        if t >= 1.0 {
            self.value = self.to;
            false
        } else {
            true
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Slide {
    pub x: Spring,
    pub y: Spring,
}

impl Default for Slide {
    fn default() -> Self {
        Self {
            x: Spring::new(0.0),
            y: Spring::new(0.0),
        }
    }
}

impl Slide {
    pub fn shift(&mut self, dx: f32, dy: f32) {
        self.x.value += dx;
        self.y.value += dy;
    }

    pub fn offset(&self) -> (f32, f32) {
        (self.x.value, self.y.value)
    }

    pub fn advance(&mut self, dt: f32) -> bool {
        let x = self.x.advance(dt);
        let y = self.y.advance(dt);
        x || y
    }
}

#[derive(Debug)]
pub struct Motion {
    pub sidebar: Spring,
    pub find: Spring,
    pub view: Tween,
    last: Option<Instant>,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            sidebar: Spring::new(1.0),
            find: Spring::new(0.0),
            view: Tween::new(1.0),
            last: None,
        }
    }
}

impl Motion {
    pub fn set_sidebar(&mut self, open: bool) {
        self.sidebar.retarget(if open { 1.0 } else { 0.0 });
    }

    pub fn set_find(&mut self, open: bool) {
        self.find.retarget(if open { 1.0 } else { 0.0 });
    }

    pub fn switch_view(&mut self) {
        self.view.start(1.0, Duration::from_millis(240));
    }

    pub fn advance(&mut self, now: Instant) -> bool {
        let dt = self
            .last
            .map_or(0.0, |last| now.duration_since(last).as_secs_f32());
        self.last = Some(now);

        let sidebar = self.sidebar.advance(dt);
        let find = self.find.advance(dt);
        let view = self.view.advance(now);
        sidebar || find || view
    }

    pub fn is_active(&self, now: Instant) -> bool {
        self.sidebar.is_active() || self.find.is_active() || self.view.is_active(now)
    }

    pub fn view_veil(&self) -> f32 {
        if self.view.duration.is_zero() {
            0.0
        } else {
            (1.0 - self.view.value).clamp(0.0, 1.0)
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct FrameBudget {
    pub samples: Vec<f32>,
}

#[allow(dead_code)]
impl FrameBudget {
    pub fn record(&mut self, dt: f32) {
        self.samples.push(dt);
    }

    pub fn percentiles(&self) -> Percentiles {
        if self.samples.is_empty() {
            return Percentiles::default();
        }

        let mut values = self.samples.clone();
        values.sort_by(f32::total_cmp);
        let percentile = |p: f32| {
            let index = ((values.len() - 1) as f32 * p).round() as usize;
            values[index]
        };

        Percentiles {
            count: values.len(),
            p50: percentile(0.50),
            p95: percentile(0.95),
            p99: percentile(0.99),
            max: percentile(1.0),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct Percentiles {
    pub count: usize,
    pub p50: f32,
    pub p95: f32,
    pub p99: f32,
    pub max: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_has_visible_follow_through_and_bounded_edge_rebound() {
        let mut wheel = WheelScroll::new(0.0);
        wheel.push(120.0, 1000.0);
        let start = wheel.last;
        let mut position = 0.0;
        for i in 1..=10 {
            wheel.advance(&mut position, start + Duration::from_millis(i * 16));
        }
        assert!(position > 50.0 && position < 80.0, "{position}");
        wheel.push(-10000.0, 1000.0);
        assert_eq!(wheel.velocity, -EDGE_VELOCITY);
        let mut minimum = position;
        for i in 11..200 {
            wheel.advance(&mut position, start + Duration::from_millis(i * 16));
            minimum = minimum.min(position);
        }
        assert!(minimum < -35.0 && minimum > -100.0, "{minimum}");
        assert_eq!(position, 0.0);
    }

    #[test]
    fn spring_retarget_preserves_velocity() {
        let mut spring = Spring::new(0.0);
        spring.target = 1.0;
        assert!(spring.advance(1.0 / 60.0));
        let velocity = spring.velocity;
        spring.retarget(-1.0);
        assert_eq!(spring.velocity, velocity);
        assert_eq!(spring.target, -1.0);
    }

    #[test]
    fn spring_midflight_retarget_settles() {
        let mut spring = Spring::new(0.0);
        spring.target = 100.0;
        assert!(spring.advance(0.05));
        spring.retarget(-100.0);
        for _ in 0..600 {
            spring.advance(1.0 / 60.0);
        }
        assert!(!spring.is_active());
        assert_eq!(spring.value, -100.0);
        assert_eq!(spring.velocity, 0.0);
    }

    #[test]
    fn spring_settles_at_large_coordinates() {
        let mut spring = Spring::new(15_442.868);
        spring.target = 15_442.857;
        for _ in 0..120 {
            spring.advance(1.0 / 60.0);
        }
        assert!(!spring.is_active());
        assert_eq!(spring.value, spring.target);
        assert_eq!(spring.velocity, 0.0);
    }

    #[test]
    #[allow(clippy::excessive_precision)]
    fn spring_settles_when_f32_integration_stalls() {
        let mut spring = Spring::new(15_442.867_187_5);
        spring.target = 15_442.857_421_875;
        spring.velocity = -0.1;
        for _ in 0..120 {
            spring.advance(1.0 / 60.0);
        }
        assert!(!spring.is_active());
        assert_eq!(spring.value, spring.target);
        assert_eq!(spring.velocity, 0.0);
    }

    #[test]
    fn spring_settles_after_large_delta() {
        let mut spring = Spring::new(0.0);
        spring.target = 100.0;
        for _ in 0..600 {
            spring.advance(1.0 / 60.0);
        }
        assert!(!spring.is_active());
        assert_eq!(spring.value, 100.0);
        assert_eq!(spring.velocity, 0.0);
    }

    #[test]
    fn tween_reaches_target() {
        let mut tween = Tween::new(0.0);
        tween.retarget(1.0, Duration::from_millis(100));
        assert!(tween.advance(tween.started));
        assert!(!tween.advance(tween.started + Duration::from_millis(120)));
        assert_eq!(tween.value, 1.0);
    }

    #[test]
    fn zero_duration_tween_settles_immediately() {
        let mut tween = Tween::new(0.0);
        tween.retarget(1.0, Duration::ZERO);
        assert_eq!(tween.value, 1.0);
        assert!(!tween.is_active(tween.started));
    }

    #[test]
    fn tween_requests_its_final_frame_even_when_the_deadline_has_passed() {
        let mut tween = Tween::new(1.0);
        tween.retarget(0.0, Duration::from_millis(160));
        tween.advance(tween.started + Duration::from_millis(145));
        let late = tween.started + Duration::from_millis(250);
        assert!(tween.is_active(late));
        tween.advance(late);
        assert_eq!(tween.value, 0.0);
        assert!(!tween.is_active(late));
    }

    #[test]
    fn view_transition_starts_fully_veiled() {
        let mut motion = Motion::default();
        motion.switch_view();

        assert!(motion.is_active(motion.view.started));
        assert_eq!(motion.view_veil(), 1.0);
        motion.advance(motion.view.started + Duration::from_millis(120));
        assert!(motion.view_veil() > 0.4 && motion.view_veil() < 0.6);
        motion.advance(motion.view.started + Duration::from_millis(240));
        assert_eq!(motion.view_veil(), 0.0);
        assert!(!motion.is_active(motion.view.started + Duration::from_millis(240)));
    }

    #[test]
    fn shifted_slide_returns_to_its_origin() {
        let mut slide = Slide::default();
        slide.shift(40.0, -80.0);
        assert_eq!(slide.offset(), (40.0, -80.0));

        for _ in 0..240 {
            slide.advance(1.0 / 60.0);
        }

        assert_eq!(slide.offset(), (0.0, 0.0));
        assert!(!slide.advance(1.0 / 60.0));
    }

    #[test]
    fn frame_percentiles_are_ordered() {
        let mut budget = FrameBudget::default();
        for value in [10.0, 2.0, 1.0, 5.0] {
            budget.record(value);
        }
        let values = budget.percentiles();
        assert!(values.p50 <= values.p95 && values.p95 <= values.p99 && values.p99 <= values.max);
        assert_eq!(values.max, 10.0);
        assert_eq!(values.count, 4);
    }
}
