//! Prints the race.ini ACXS writes for a join, to compare with one from Content Manager.
use acxs_manager::race::OnlineJoin;

fn main() {
    let join = OnlineJoin {
        ip: "194.35.12.49".into(),
        port: 9600,
        http_port: 8081,
        server_name: "Cruisin USA | L.A. Canyons EU 1".into(),
        track: "la_canyons".into(),
        track_layout: Some("freeroam".into()),
        car: "ks_lamborghini_aventador_sv".into(),
        skin: "09_grigio_thalasso_metal".into(),
        driver: "Driver".into(),
        password: String::new(),
        steam_id: "0".into(),
        features: vec!["WEATHERFX_V1".into(), "STEAM_TICKET".into()],
    };
    print!("{}", join.to_race_ini());
}
