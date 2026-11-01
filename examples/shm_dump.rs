//! Prints one snapshot of the game's shared memory: `cargo run --example shm_dump`.
fn main() {
    match acxs_manager::sharedmem::read_snapshot() {
        Ok(s) => {
            let p = &s.physics;
            println!("status {:?} session {:?} packet {}", s.graphics.status, s.graphics.session, p.packet_id);
            println!("car {} skin {} track {} layout '{}'", s.static_page.car_model, s.static_page.car_skin, s.static_page.track, s.static_page.track_configuration);
            println!("speed {} steer {} gforces {:?} yawvel {:?} localvel {:?}", p.speed_kmh, p.steer_angle, p.g_forces, p.local_angular_velocity, p.local_velocity);
            println!("wheel_load {:?} ang {:?} tyre_radius {:?} tyres_out {}", p.wheel_load, p.wheel_angular_speed, s.static_page.tyre_radius, p.number_of_tyres_out);
            println!("pos {} coords {:?} laps {} last {} best {}", s.graphics.normalized_car_position, s.graphics.car_coordinates, s.graphics.completed_laps, s.graphics.last_lap_ms, s.graphics.best_lap_ms);
            println!("spline len {} ai {}", s.static_page.track_spline_length, p.is_ai_controlled);
        }
        Err(e) => println!("no shared memory: {e}"),
    }
}
