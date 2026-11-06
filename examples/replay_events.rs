//! Lists sudden speed losses in a replay, to tune crash detection against real drives:
//! `cargo run --example replay_events -- <file.acreplay>`.

use acxs_manager::replay::read_replay_samples;

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(replay) = read_replay_samples(std::path::Path::new(&path)) else {
            println!("{path}: unreadable");
            continue;
        };
        let info = replay.info();
        println!("{path}\n  car {} driver {} cars {} {:.0} s", info.car, info.driver, info.num_cars, info.duration_s);
        let samples = replay.samples();
        let mut last = f64::NEG_INFINITY;
        for (i, s) in samples.iter().enumerate() {
            let from = i.saturating_sub(12);
            let window = &samples[from..=i];
            let top = window.iter().map(|w| w.speed_kmh).fold(0.0_f32, f32::max);
            if top - s.speed_kmh >= 20.0 && s.time_s - last > 2.0 {
                last = s.time_s;
                let later = samples.get(i + 30).map_or(s.speed_kmh, |l| l.speed_kmh);
                println!(
                    "  t={:7.1}  {:5.0} -> {:5.0} km/h in {:.2} s  brake {:.2} throttle {:.2}  speed 1 s later {:5.0}",
                    s.time_s, top, s.speed_kmh, s.time_s - window[0].time_s, s.brake, s.throttle, later
                );
            }
        }
    }
}
