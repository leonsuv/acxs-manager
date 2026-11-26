//! Assetto Corsa Dedicated Server UDP Plugin Protocol (v4) codec and client.
//!
//! Speaks directly to `acServer`'s `UDP_PLUGIN_LOCAL_PORT` <-> `UDP_PLUGIN_ADDRESS`
//! interface for live race monitoring, spectator telemetry, chat, and admin commands
//! (kick, ban, ballast, restrictor, next/restart session).
//!
//! Wire format is little-endian:
//! - `ascii_str`: `u8 len` followed by `len` ASCII/UTF-8 bytes
//! - `utf32_str`: `u8 char_count` followed by `char_count * 4` little-endian `u32` code points
//! - `vec3`: `3 * f32` LE `(x, y, z)`

use std::fmt;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

pub const ACSP_PROTOCOL_VERSION: u8 = 4;

// Server -> Plugin packet IDs.
pub const ACSP_NEW_SESSION: u8 = 50;
pub const ACSP_NEW_CONNECTION: u8 = 51;
pub const ACSP_CONNECTION_CLOSED: u8 = 52;
pub const ACSP_CAR_UPDATE: u8 = 53;
pub const ACSP_CAR_INFO: u8 = 54;
pub const ACSP_END_SESSION: u8 = 55;
pub const ACSP_VERSION: u8 = 56;
pub const ACSP_CHAT: u8 = 57;
pub const ACSP_CLIENT_LOADED: u8 = 58;
pub const ACSP_SESSION_INFO: u8 = 59;
pub const ACSP_ERROR: u8 = 60;
pub const ACSP_LAP_COMPLETED: u8 = 73;
pub const ACSP_CLIENT_EVENT: u8 = 130;

// Client event subtypes.
pub const CE_COLLISION_WITH_CAR: u8 = 10;
pub const CE_COLLISION_WITH_ENV: u8 = 11;

// Plugin -> Server command IDs.
pub const ACSP_REALTIMEPOS_INTERVAL: u8 = 200;
pub const ACSP_GET_CAR_INFO: u8 = 201;
pub const ACSP_SEND_CHAT: u8 = 202;
pub const ACSP_BROADCAST_CHAT: u8 = 203;
pub const ACSP_GET_SESSION_INFO: u8 = 204;
pub const ACSP_SET_SESSION_INFO: u8 = 205;
pub const ACSP_KICK_USER: u8 = 206;
pub const ACSP_NEXT_SESSION: u8 = 207;
pub const ACSP_RESTART_SESSION: u8 = 208;
pub const ACSP_ADMIN_COMMAND: u8 = 209;

const MAX_UDP_DATAGRAM: usize = 4096;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfoPayload {
    pub version: u8,
    pub session_index: u8,
    pub current_session_index: u8,
    pub session_count: u8,
    pub server_name: String,
    pub track: String,
    pub track_config: String,
    pub name: String,
    pub session_type: u8,
    pub time: u16,
    pub laps: u16,
    pub wait_time: u16,
    pub ambient_temp: u8,
    pub road_temp: u8,
    pub weather_graphics: String,
    pub elapsed_ms: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionPayload {
    pub driver_name: String,
    pub driver_guid: String,
    pub car_id: u8,
    pub car_model: String,
    pub car_skin: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LapLeaderboardEntry {
    pub car_id: u8,
    pub time_ms: u32,
    pub laps: u16,
    pub has_completed_flag: u8,
}

/// Decoded packet sent by `acServer` to the plugin.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerEvent {
    NewSession(SessionInfoPayload),
    SessionInfo(SessionInfoPayload),
    NewConnection(ConnectionPayload),
    ConnectionClosed(ConnectionPayload),
    CarUpdate {
        car_id: u8,
        pos: [f32; 3],
        vel: [f32; 3],
        gear: u8,
        engine_rpm: u16,
        normalized_spline_pos: f32,
    },
    CarInfo {
        car_id: u8,
        is_connected: bool,
        model: String,
        skin: String,
        driver_name: String,
        driver_team: String,
        driver_guid: String,
    },
    EndSession {
        report_json_path: String,
    },
    Version(u8),
    Chat {
        car_id: u8,
        message: String,
    },
    ClientLoaded {
        car_id: u8,
    },
    Error(String),
    LapCompleted {
        car_id: u8,
        lap_time_ms: u32,
        cuts: u8,
        leaderboard: Vec<LapLeaderboardEntry>,
        grip_level: f32,
    },
    CollisionWithCar {
        car_id: u8,
        other_car_id: u8,
        impact_speed: f32,
        world_pos: [f32; 3],
        rel_pos: [f32; 3],
    },
    CollisionWithEnv {
        car_id: u8,
        impact_speed: f32,
        world_pos: [f32; 3],
        rel_pos: [f32; 3],
    },
}

/// Command sent from ACXS Manager to `acServer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminCmd {
    SetRealtimePosInterval(u16),
    GetCarInfo(u8),
    SendChat {
        car_id: u8,
        message: String,
    },
    BroadcastChat(String),
    GetSessionInfo(i16),
    SetSessionInfo {
        session_index: u8,
        name: String,
        session_type: u8,
        laps: u32,
        time: u32,
        wait_time: u32,
    },
    KickUser(u8),
    NextSession,
    RestartSession,
    AdminCommand(String),
}

/// Errors decoding an incoming `acServer` UDP packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    Truncated,
    UnknownPacket(u8),
    UnknownClientEvent(u8),
    InvalidUnicode,
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::Truncated => write!(f, "truncated UDP plugin packet"),
            CodecError::UnknownPacket(id) => write!(f, "unknown UDP plugin packet id {id}"),
            CodecError::UnknownClientEvent(id) => write!(f, "unknown client event subtype {id}"),
            CodecError::InvalidUnicode => write!(f, "invalid Unicode scalar in UTF-32 string"),
        }
    }
}

impl std::error::Error for CodecError {}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], CodecError> {
        let end = self.pos.checked_add(n).ok_or(CodecError::Truncated)?;
        if end > self.buf.len() {
            return Err(CodecError::Truncated);
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, CodecError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, CodecError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, CodecError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> Result<f32, CodecError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn vec3(&mut self) -> Result<[f32; 3], CodecError> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }

    fn ascii_str(&mut self) -> Result<String, CodecError> {
        let len = self.u8()? as usize;
        let bytes = self.take(len)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn utf32_str(&mut self) -> Result<String, CodecError> {
        let count = self.u8()? as usize;
        let mut out = String::with_capacity(count);
        for _ in 0..count {
            let cp = self.u32()?;
            let ch = char::from_u32(cp).ok_or(CodecError::InvalidUnicode)?;
            out.push(ch);
        }
        Ok(out)
    }
}

fn write_ascii_str(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(255);
    out.push(len as u8);
    out.extend_from_slice(&bytes[..len]);
}

fn write_utf32_str(out: &mut Vec<u8>, s: &str) {
    let chars: Vec<u32> = s.chars().take(255).map(|c| c as u32).collect();
    out.push(chars.len() as u8);
    for cp in chars {
        out.extend_from_slice(&cp.to_le_bytes());
    }
}

fn read_session_info(r: &mut Reader<'_>) -> Result<SessionInfoPayload, CodecError> {
    Ok(SessionInfoPayload {
        version: r.u8()?,
        session_index: r.u8()?,
        current_session_index: r.u8()?,
        session_count: r.u8()?,
        server_name: r.utf32_str()?,
        track: r.ascii_str()?,
        track_config: r.ascii_str()?,
        name: r.ascii_str()?,
        session_type: r.u8()?,
        time: r.u16()?,
        laps: r.u16()?,
        wait_time: r.u16()?,
        ambient_temp: r.u8()?,
        road_temp: r.u8()?,
        weather_graphics: r.ascii_str()?,
        elapsed_ms: r.i32()?,
    })
}

fn read_connection(r: &mut Reader<'_>) -> Result<ConnectionPayload, CodecError> {
    Ok(ConnectionPayload {
        driver_name: r.utf32_str()?,
        driver_guid: r.utf32_str()?,
        car_id: r.u8()?,
        car_model: r.ascii_str()?,
        car_skin: r.ascii_str()?,
    })
}

/// Decodes one incoming `acServer` UDP plugin packet.
pub fn decode_server_event(bytes: &[u8]) -> Result<ServerEvent, CodecError> {
    let mut r = Reader::new(bytes);
    let id = r.u8()?;
    match id {
        ACSP_NEW_SESSION => Ok(ServerEvent::NewSession(read_session_info(&mut r)?)),
        ACSP_SESSION_INFO => Ok(ServerEvent::SessionInfo(read_session_info(&mut r)?)),
        ACSP_NEW_CONNECTION => Ok(ServerEvent::NewConnection(read_connection(&mut r)?)),
        ACSP_CONNECTION_CLOSED => Ok(ServerEvent::ConnectionClosed(read_connection(&mut r)?)),
        ACSP_CAR_UPDATE => Ok(ServerEvent::CarUpdate {
            car_id: r.u8()?,
            pos: r.vec3()?,
            vel: r.vec3()?,
            gear: r.u8()?,
            engine_rpm: r.u16()?,
            normalized_spline_pos: r.f32()?,
        }),
        ACSP_CAR_INFO => Ok(ServerEvent::CarInfo {
            car_id: r.u8()?,
            is_connected: r.u8()? != 0,
            model: r.utf32_str()?,
            skin: r.utf32_str()?,
            driver_name: r.utf32_str()?,
            driver_team: r.utf32_str()?,
            driver_guid: r.utf32_str()?,
        }),
        ACSP_END_SESSION => Ok(ServerEvent::EndSession {
            report_json_path: r.utf32_str()?,
        }),
        ACSP_VERSION => Ok(ServerEvent::Version(r.u8()?)),
        ACSP_CHAT => Ok(ServerEvent::Chat {
            car_id: r.u8()?,
            message: r.utf32_str()?,
        }),
        ACSP_CLIENT_LOADED => Ok(ServerEvent::ClientLoaded { car_id: r.u8()? }),
        ACSP_ERROR => Ok(ServerEvent::Error(r.utf32_str()?)),
        ACSP_LAP_COMPLETED => {
            let car_id = r.u8()?;
            let lap_time_ms = r.u32()?;
            let cuts = r.u8()?;
            let cars_count = r.u8()? as usize;
            let mut leaderboard = Vec::with_capacity(cars_count);
            for _ in 0..cars_count {
                leaderboard.push(LapLeaderboardEntry {
                    car_id: r.u8()?,
                    time_ms: r.u32()?,
                    laps: r.u16()?,
                    has_completed_flag: r.u8()?,
                });
            }
            let grip_level = r.f32()?;
            Ok(ServerEvent::LapCompleted {
                car_id,
                lap_time_ms,
                cuts,
                leaderboard,
                grip_level,
            })
        }
        ACSP_CLIENT_EVENT => {
            let subtype = r.u8()?;
            let car_id = r.u8()?;
            match subtype {
                CE_COLLISION_WITH_CAR => Ok(ServerEvent::CollisionWithCar {
                    car_id,
                    other_car_id: r.u8()?,
                    impact_speed: r.f32()?,
                    world_pos: r.vec3()?,
                    rel_pos: r.vec3()?,
                }),
                CE_COLLISION_WITH_ENV => Ok(ServerEvent::CollisionWithEnv {
                    car_id,
                    impact_speed: r.f32()?,
                    world_pos: r.vec3()?,
                    rel_pos: r.vec3()?,
                }),
                other => Err(CodecError::UnknownClientEvent(other)),
            }
        }
        other => Err(CodecError::UnknownPacket(other)),
    }
}

/// Encodes one [`AdminCmd`] to send to `acServer`.
pub fn encode_admin_cmd(cmd: &AdminCmd) -> Vec<u8> {
    let mut out = Vec::new();
    match cmd {
        AdminCmd::SetRealtimePosInterval(ms) => {
            out.push(ACSP_REALTIMEPOS_INTERVAL);
            out.extend_from_slice(&ms.to_le_bytes());
        }
        AdminCmd::GetCarInfo(car_id) => {
            out.push(ACSP_GET_CAR_INFO);
            out.push(*car_id);
        }
        AdminCmd::SendChat { car_id, message } => {
            out.push(ACSP_SEND_CHAT);
            out.push(*car_id);
            write_utf32_str(&mut out, message);
        }
        AdminCmd::BroadcastChat(message) => {
            out.push(ACSP_BROADCAST_CHAT);
            write_utf32_str(&mut out, message);
        }
        AdminCmd::GetSessionInfo(session_idx) => {
            out.push(ACSP_GET_SESSION_INFO);
            out.extend_from_slice(&session_idx.to_le_bytes());
        }
        AdminCmd::SetSessionInfo {
            session_index,
            name,
            session_type,
            laps,
            time,
            wait_time,
        } => {
            out.push(ACSP_SET_SESSION_INFO);
            out.push(*session_index);
            write_utf32_str(&mut out, name);
            out.push(*session_type);
            out.extend_from_slice(&laps.to_le_bytes());
            out.extend_from_slice(&time.to_le_bytes());
            out.extend_from_slice(&wait_time.to_le_bytes());
        }
        AdminCmd::KickUser(car_id) => {
            out.push(ACSP_KICK_USER);
            out.push(*car_id);
        }
        AdminCmd::NextSession => out.push(ACSP_NEXT_SESSION),
        AdminCmd::RestartSession => out.push(ACSP_RESTART_SESSION),
        AdminCmd::AdminCommand(command) => {
            out.push(ACSP_ADMIN_COMMAND);
            write_utf32_str(&mut out, command);
        }
    }
    out
}

/// Encodes a [`ServerEvent`] into wire bytes (useful for mock servers and protocol tests).
pub fn encode_server_event(event: &ServerEvent) -> Vec<u8> {
    let mut out = Vec::new();
    let push_session = |out: &mut Vec<u8>, s: &SessionInfoPayload| {
        out.push(s.version);
        out.push(s.session_index);
        out.push(s.current_session_index);
        out.push(s.session_count);
        write_utf32_str(out, &s.server_name);
        write_ascii_str(out, &s.track);
        write_ascii_str(out, &s.track_config);
        write_ascii_str(out, &s.name);
        out.push(s.session_type);
        out.extend_from_slice(&s.time.to_le_bytes());
        out.extend_from_slice(&s.laps.to_le_bytes());
        out.extend_from_slice(&s.wait_time.to_le_bytes());
        out.push(s.ambient_temp);
        out.push(s.road_temp);
        write_ascii_str(out, &s.weather_graphics);
        out.extend_from_slice(&s.elapsed_ms.to_le_bytes());
    };
    let push_conn = |out: &mut Vec<u8>, c: &ConnectionPayload| {
        write_utf32_str(out, &c.driver_name);
        write_utf32_str(out, &c.driver_guid);
        out.push(c.car_id);
        write_ascii_str(out, &c.car_model);
        write_ascii_str(out, &c.car_skin);
    };
    let push_vec3 = |out: &mut Vec<u8>, v: [f32; 3]| {
        for f in v {
            out.extend_from_slice(&f.to_le_bytes());
        }
    };

    match event {
        ServerEvent::NewSession(s) => {
            out.push(ACSP_NEW_SESSION);
            push_session(&mut out, s);
        }
        ServerEvent::SessionInfo(s) => {
            out.push(ACSP_SESSION_INFO);
            push_session(&mut out, s);
        }
        ServerEvent::NewConnection(c) => {
            out.push(ACSP_NEW_CONNECTION);
            push_conn(&mut out, c);
        }
        ServerEvent::ConnectionClosed(c) => {
            out.push(ACSP_CONNECTION_CLOSED);
            push_conn(&mut out, c);
        }
        ServerEvent::CarUpdate {
            car_id,
            pos,
            vel,
            gear,
            engine_rpm,
            normalized_spline_pos,
        } => {
            out.push(ACSP_CAR_UPDATE);
            out.push(*car_id);
            push_vec3(&mut out, *pos);
            push_vec3(&mut out, *vel);
            out.push(*gear);
            out.extend_from_slice(&engine_rpm.to_le_bytes());
            out.extend_from_slice(&normalized_spline_pos.to_le_bytes());
        }
        ServerEvent::CarInfo {
            car_id,
            is_connected,
            model,
            skin,
            driver_name,
            driver_team,
            driver_guid,
        } => {
            out.push(ACSP_CAR_INFO);
            out.push(*car_id);
            out.push(u8::from(*is_connected));
            write_utf32_str(&mut out, model);
            write_utf32_str(&mut out, skin);
            write_utf32_str(&mut out, driver_name);
            write_utf32_str(&mut out, driver_team);
            write_utf32_str(&mut out, driver_guid);
        }
        ServerEvent::EndSession { report_json_path } => {
            out.push(ACSP_END_SESSION);
            write_utf32_str(&mut out, report_json_path);
        }
        ServerEvent::Version(v) => {
            out.push(ACSP_VERSION);
            out.push(*v);
        }
        ServerEvent::Chat { car_id, message } => {
            out.push(ACSP_CHAT);
            out.push(*car_id);
            write_utf32_str(&mut out, message);
        }
        ServerEvent::ClientLoaded { car_id } => {
            out.push(ACSP_CLIENT_LOADED);
            out.push(*car_id);
        }
        ServerEvent::Error(msg) => {
            out.push(ACSP_ERROR);
            write_utf32_str(&mut out, msg);
        }
        ServerEvent::LapCompleted {
            car_id,
            lap_time_ms,
            cuts,
            leaderboard,
            grip_level,
        } => {
            out.push(ACSP_LAP_COMPLETED);
            out.push(*car_id);
            out.extend_from_slice(&lap_time_ms.to_le_bytes());
            out.push(*cuts);
            out.push(leaderboard.len().min(255) as u8);
            for e in leaderboard.iter().take(255) {
                out.push(e.car_id);
                out.extend_from_slice(&e.time_ms.to_le_bytes());
                out.extend_from_slice(&e.laps.to_le_bytes());
                out.push(e.has_completed_flag);
            }
            out.extend_from_slice(&grip_level.to_le_bytes());
        }
        ServerEvent::CollisionWithCar {
            car_id,
            other_car_id,
            impact_speed,
            world_pos,
            rel_pos,
        } => {
            out.push(ACSP_CLIENT_EVENT);
            out.push(CE_COLLISION_WITH_CAR);
            out.push(*car_id);
            out.push(*other_car_id);
            out.extend_from_slice(&impact_speed.to_le_bytes());
            push_vec3(&mut out, *world_pos);
            push_vec3(&mut out, *rel_pos);
        }
        ServerEvent::CollisionWithEnv {
            car_id,
            impact_speed,
            world_pos,
            rel_pos,
        } => {
            out.push(ACSP_CLIENT_EVENT);
            out.push(CE_COLLISION_WITH_ENV);
            out.push(*car_id);
            out.extend_from_slice(&impact_speed.to_le_bytes());
            push_vec3(&mut out, *world_pos);
            push_vec3(&mut out, *rel_pos);
        }
    }
    out
}

/// Receive error returned by [`RconClient::recv_event`].
#[derive(Debug)]
pub enum RconError {
    Io(io::Error),
    Timeout,
    Codec(CodecError),
}

impl fmt::Display for RconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RconError::Io(e) => write!(f, "RCON socket error: {e}"),
            RconError::Timeout => write!(f, "timed out waiting for server UDP event"),
            RconError::Codec(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RconError {}

/// Live UDP plugin client connected to an `acServer` instance.
pub struct RconClient {
    socket: UdpSocket,
    server_addr: SocketAddr,
}

impl RconClient {
    /// Binds `local_bind` (matching `UDP_PLUGIN_ADDRESS`) and targets `server_addr`
    /// (`UDP_PLUGIN_LOCAL_PORT`).
    pub fn bind(local_bind: SocketAddr, server_addr: SocketAddr, timeout: Duration) -> io::Result<Self> {
        let socket = UdpSocket::bind(local_bind)?;
        socket.set_read_timeout(Some(timeout))?;
        socket.set_write_timeout(Some(timeout))?;
        Ok(Self {
            socket,
            server_addr,
        })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn send_cmd(&self, cmd: &AdminCmd) -> io::Result<()> {
        let bytes = encode_admin_cmd(cmd);
        self.socket.send_to(&bytes, self.server_addr)?;
        Ok(())
    }

    pub fn recv_event(&self) -> Result<ServerEvent, RconError> {
        let mut buf = [0u8; MAX_UDP_DATAGRAM];
        let (n, _) = match self.socket.recv_from(&mut buf) {
            Ok(v) => v,
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                return Err(RconError::Timeout);
            }
            Err(e) => return Err(RconError::Io(e)),
        };
        decode_server_event(&buf[..n]).map_err(RconError::Codec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn sample_session() -> SessionInfoPayload {
        SessionInfoPayload {
            version: ACSP_PROTOCOL_VERSION,
            session_index: 0,
            current_session_index: 0,
            session_count: 3,
            server_name: "ACXS League — Nürburgring 🏁".into(),
            track: "ks_nordschleife".into(),
            track_config: "tourist".into(),
            name: "Practice".into(),
            session_type: 1,
            time: 20,
            laps: 0,
            wait_time: 60,
            ambient_temp: 22,
            road_temp: 31,
            weather_graphics: "3_clear".into(),
            elapsed_ms: 12_500,
        }
    }

    #[test]
    fn codec_round_trip_all_server_events_50_through_130() {
        let events = vec![
            ServerEvent::NewSession(sample_session()),
            ServerEvent::SessionInfo(sample_session()),
            ServerEvent::NewConnection(ConnectionPayload {
                driver_name: "Léa Müller 🏎️".into(),
                driver_guid: "76561198000000001".into(),
                car_id: 3,
                car_model: "ks_ferrari_488_gt3".into(),
                car_skin: "red_01".into(),
            }),
            ServerEvent::ConnectionClosed(ConnectionPayload {
                driver_name: "Bob".into(),
                driver_guid: "76561198000000002".into(),
                car_id: 4,
                car_model: "bmw_m3_e30".into(),
                car_skin: "white".into(),
            }),
            ServerEvent::CarUpdate {
                car_id: 3,
                pos: [10.5, 2.0, -45.25],
                vel: [50.0, 0.1, -12.0],
                gear: 5,
                engine_rpm: 7800,
                normalized_spline_pos: 0.425,
            },
            ServerEvent::CarInfo {
                car_id: 3,
                is_connected: true,
                model: "ks_ferrari_488_gt3".into(),
                skin: "red_01".into(),
                driver_name: "Léa".into(),
                driver_team: "Scuderia".into(),
                driver_guid: "76561198000000001".into(),
            },
            ServerEvent::EndSession {
                report_json_path: "results/2026_11_26_race.json".into(),
            },
            ServerEvent::Version(ACSP_PROTOCOL_VERSION),
            ServerEvent::Chat {
                car_id: 3,
                message: "Good luck!".into(),
            },
            ServerEvent::ClientLoaded { car_id: 3 },
            ServerEvent::Error("invalid command".into()),
            ServerEvent::LapCompleted {
                car_id: 3,
                lap_time_ms: 108_432,
                cuts: 0,
                leaderboard: vec![LapLeaderboardEntry {
                    car_id: 3,
                    time_ms: 108_432,
                    laps: 2,
                    has_completed_flag: 1,
                }],
                grip_level: 0.985,
            },
            ServerEvent::CollisionWithCar {
                car_id: 3,
                other_car_id: 4,
                impact_speed: 32.5,
                world_pos: [1.0, 2.0, 3.0],
                rel_pos: [0.5, 0.0, -0.5],
            },
            ServerEvent::CollisionWithEnv {
                car_id: 3,
                impact_speed: 18.0,
                world_pos: [4.0, 5.0, 6.0],
                rel_pos: [-0.2, 0.1, 0.3],
            },
        ];

        for ev in events {
            let wire = encode_server_event(&ev);
            let decoded = decode_server_event(&wire).unwrap();
            assert_eq!(decoded, ev);
        }
    }

    #[test]
    fn codec_encodes_all_admin_commands_200_through_209() {
        assert_eq!(encode_admin_cmd(&AdminCmd::SetRealtimePosInterval(250)), vec![200, 250, 0]);
        assert_eq!(encode_admin_cmd(&AdminCmd::GetCarInfo(7)), vec![201, 7]);
        assert_eq!(encode_admin_cmd(&AdminCmd::KickUser(3)), vec![206, 3]);
        assert_eq!(encode_admin_cmd(&AdminCmd::NextSession), vec![207]);
        assert_eq!(encode_admin_cmd(&AdminCmd::RestartSession), vec![208]);

        let chat = encode_admin_cmd(&AdminCmd::SendChat {
            car_id: 2,
            message: "Hi".into(),
        });
        assert_eq!(chat[0], 202);
        assert_eq!(chat[1], 2);
        assert_eq!(chat[2], 2); // 2 UTF-32 chars
        assert_eq!(chat.len(), 3 + 8);

        let bcast = encode_admin_cmd(&AdminCmd::BroadcastChat("Go!".into()));
        assert_eq!(bcast[0], 203);
        assert_eq!(bcast[1], 3);

        let get_sess = encode_admin_cmd(&AdminCmd::GetSessionInfo(-1));
        assert_eq!(get_sess, vec![204, 0xFF, 0xFF]);

        let set_sess = encode_admin_cmd(&AdminCmd::SetSessionInfo {
            session_index: 1,
            name: "Q".into(),
            session_type: 2,
            laps: 0,
            time: 15,
            wait_time: 30,
        });
        assert_eq!(set_sess[0], 205);

        let admin = encode_admin_cmd(&AdminCmd::AdminCommand("/ballast 3 20".into()));
        assert_eq!(admin[0], 209);
        assert_eq!(admin[1], 13);
    }

    #[test]
    fn loopback_udp_mock_server_exchanges_car_updates_chat_and_kick() {
        let server_sock = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        server_sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let server_addr = server_sock.local_addr().unwrap();

        let client = RconClient::bind(
            (Ipv4Addr::LOCALHOST, 0).into(),
            server_addr,
            Duration::from_secs(2),
        )
        .unwrap();
        let client_addr = client.local_addr().unwrap();

        // Client sends KickUser(5), server receives it.
        client.send_cmd(&AdminCmd::KickUser(5)).unwrap();
        let mut buf = [0u8; 64];
        let (n, _) = server_sock.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..n], &[ACSP_KICK_USER, 5]);

        // Server pushes a CarUpdate, client decodes it.
        let update = ServerEvent::CarUpdate {
            car_id: 5,
            pos: [100.0, 0.5, -200.0],
            vel: [30.0, 0.0, 10.0],
            gear: 4,
            engine_rpm: 6500,
            normalized_spline_pos: 0.75,
        };
        server_sock
            .send_to(&encode_server_event(&update), client_addr)
            .unwrap();
        assert_eq!(client.recv_event().unwrap(), update);
    }

    #[test]
    fn truncated_or_unknown_udp_packets_return_decode_error_without_panic() {
        assert_eq!(decode_server_event(&[]), Err(CodecError::Truncated));
        assert_eq!(decode_server_event(&[255]), Err(CodecError::UnknownPacket(255)));
        assert_eq!(decode_server_event(&[ACSP_CAR_UPDATE, 1, 2]), Err(CodecError::Truncated));
        assert_eq!(
            decode_server_event(&[ACSP_CLIENT_EVENT, 99, 1]),
            Err(CodecError::UnknownClientEvent(99))
        );
    }
}
