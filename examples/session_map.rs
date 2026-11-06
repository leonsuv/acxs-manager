//! Draws a recorded session's track and trouble spots to a PNG, to look at real data without
//! opening the app: `cargo run --example session_map -- <session.json> <out.png>`.

use acxs_manager::session_recorder::{RecordedSession, trace_breaks};
use acxs_manager::track_map::project_lap;

const SIZE: usize = 900;

fn line(rgba: &mut [u8], a: [f32; 2], b: [f32; 2], color: [u8; 3]) {
    let steps = ((b[0] - a[0]).abs().max((b[1] - a[1]).abs()) as usize).max(1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        dot(rgba, [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t], 1, color);
    }
}

fn dot(rgba: &mut [u8], at: [f32; 2], radius: i32, color: [u8; 3]) {
    for y in -radius..=radius {
        for x in -radius..=radius {
            if x * x + y * y > radius * radius {
                continue;
            }
            let (px, py) = (at[0] as i32 + x, at[1] as i32 + y);
            if (0..SIZE as i32).contains(&px) && (0..SIZE as i32).contains(&py) {
                let i = (py as usize * SIZE + px as usize) * 4;
                rgba[i..i + 3].copy_from_slice(&color);
            }
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args.next().expect("session.json");
    let output = args.next().expect("out.png");
    let session: RecordedSession = serde_json::from_str(&std::fs::read_to_string(input).unwrap()).unwrap();
    let mut world: Vec<[f32; 2]> = session.trace.iter().map(|&(x, z)| [x, z]).collect();
    let line_len = world.len();
    world.extend(session.event_points.iter().map(|p| [p.x, p.z]));
    let unit = project_lap(&world);
    let screen = |p: [f32; 2]| [30.0 + p[0] * (SIZE as f32 - 60.0), 30.0 + p[1] * (SIZE as f32 - 60.0)];
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    rgba.chunks_mut(4).for_each(|c| c.copy_from_slice(&[11, 13, 17, 255]));
    let breaks = trace_breaks(&session.trace);
    println!(
        "trace {} points, {} cuts; {} event points; path {} slices",
        line_len,
        breaks.iter().filter(|b| **b).count(),
        session.event_points.len(),
        session.path.len()
    );
    for i in 1..line_len {
        if !breaks[i] {
            line(&mut rgba, screen(unit[i - 1]), screen(unit[i]), [120, 130, 150]);
        }
    }
    for (point, event) in unit[line_len..].iter().zip(&session.event_points) {
        let color = match format!("{:?}", event.kind).as_str() {
            "Crash" => [255, 92, 108],
            "OffTrack" => [245, 160, 66],
            "FrontLockup" | "RearLockup" => [245, 215, 70],
            "Wheelspin" => [61, 214, 140],
            "Jump" => [110, 205, 240],
            "Understeer" => [91, 140, 255],
            _ => [200, 110, 240],
        };
        dot(&mut rgba, screen(*point), 4, color);
    }
    std::fs::write(output, acxs_manager::png::encode_rgba(SIZE as u32, SIZE as u32, &rgba).unwrap()).unwrap();
}
