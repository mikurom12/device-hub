use device_hub::motion;
use std::time::Duration;

fn frame() {
    std::thread::sleep(Duration::from_millis(8));
    motion::begin_frame();
}

#[test]
fn at_is_monotonic_bounded_and_stops() {
    motion::restart("itest:at-monotonic", 0.);
    let mut values = Vec::new();
    for _ in 0..30 {
        frame();
        values.push(motion::at(
            "itest:at-monotonic",
            1.,
            motion::DUR_INDICATOR,
            motion::EASE_OUT,
        ));
    }
    assert!(values.iter().all(|v| (0.0..=1.0).contains(v)));
    assert!(values.windows(2).all(|w| w[1] + 1e-6 >= w[0]));
    assert_eq!(*values.last().unwrap(), 1.);
    frame();
    assert_eq!(
        motion::at(
            "itest:at-monotonic",
            1.,
            motion::DUR_INDICATOR,
            motion::EASE_OUT
        ),
        1.
    );
}


#[test]
fn at_reversal_is_bounded_and_monotonic_per_leg() {
    motion::restart("itest:at-reversal", 0.);
    let mut up = Vec::new();
    for _ in 0..6 {
        frame();
        up.push(motion::at(
            "itest:at-reversal",
            1.,
            motion::DUR_INDICATOR,
            motion::EASE_OUT,
        ));
    }
    let mut down = Vec::new();
    for _ in 0..30 {
        frame();
        down.push(motion::at(
            "itest:at-reversal",
            0.,
            motion::DUR_INDICATOR,
            motion::EASE_OUT,
        ));
    }
    assert!(up.windows(2).all(|w| w[1] + 1e-6 >= w[0]));
    assert!(down.windows(2).all(|w| w[1] <= w[0] + 1e-6));
    assert!(up.iter().chain(&down).all(|v| (0.0..=1.0).contains(v)));
    assert_eq!(*down.last().unwrap(), 0.);
}

#[test]
fn critical_spring_from_rest_is_monotonic_bounded_and_stops() {
    motion::restart("itest:spring-monotonic", 0.2);
    let mut values = Vec::new();
    for _ in 0..100 {
        frame();
        values.push(motion::at_spring("itest:spring-monotonic", 1., 18., 1.));
    }
    assert!(values.iter().all(|v| (0.0..=1.0).contains(v)));
    assert!(values.windows(2).all(|w| w[1] + 1e-6 >= w[0]));
    assert_eq!(*values.last().unwrap(), 1.);
    frame();
    assert_eq!(motion::at_spring("itest:spring-monotonic", 1., 18., 1.), 1.);
}


#[test]
fn critical_spring_reversal_does_not_continue_away_from_target() {
    motion::restart("itest:spring-reversal", 0.);
    for _ in 0..8 {
        frame();
        motion::at_spring("itest:spring-reversal", 1., 18., 1.);
    }
    let mut down = Vec::new();
    for _ in 0..100 {
        frame();
        down.push(motion::at_spring("itest:spring-reversal", 0., 18., 1.));
    }
    assert!(down.windows(2).all(|w| w[1] <= w[0] + 1e-6));
    assert!(down.iter().all(|v| (0.0..=1.0).contains(v)));
    assert_eq!(*down.last().unwrap(), 0.);
}

#[test]
fn spring_step_clamps_target_crossing() {
    let (x, v) = motion::spring_advance(0.9, 30., 1., 18., 1., 1. / 60.);
    assert_eq!(x, 1.);
    assert_eq!(v, 0.);
}

#[test]
fn ease_out_curve_is_monotonic_and_bounded() {
    let mut last = 0.;
    for i in 0..=100 {
        let y = motion::bezier(
            motion::EASE_OUT[0],
            motion::EASE_OUT[1],
            motion::EASE_OUT[2],
            motion::EASE_OUT[3],
            i as f32 / 100.,
        );
        assert!((0.0..=1.0).contains(&y));
        assert!(y + 1e-6 >= last);
        last = y;
    }
    assert_eq!(last, 1.);
}
