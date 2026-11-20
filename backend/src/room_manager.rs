use crate::protocol::{ParticipantInfo, ServerMessage};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand_core::OsRng;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

pub type Outbound = mpsc::Sender<ServerMessage>;

#[derive(Default)]
pub struct RoomManager {
    state: Arc<RwLock<State>>,
}

#[derive(Default)]
struct State {
    rooms: HashMap<String, Room>,
    user_rooms: HashMap<Uuid, String>,
}

struct Room {
    password_hash: String,
    participants: HashMap<Uuid, Participant>,
}

struct Participant {
    username: String,
    outbound: Outbound,
    microphone_enabled: bool,
    camera_enabled: bool,
    screen_sharing: bool,
}

pub struct RoomEntry {
    pub room_id: String,
    pub participants: Vec<ParticipantInfo>,
}

pub struct LeaveResult {
    pub room_id: String,
    pub username: String,
    pub remaining: Vec<(Outbound, ServerMessage)>,
}

impl RoomManager {
    pub async fn create_room(
        &self,
        user_id: Uuid,
        username: &str,
        room_id: &str,
        password: &str,
        outbound: Outbound,
    ) -> Result<RoomEntry, String> {
        validate_room_id(room_id)?;
        validate_password(password)?;
        let room_key = room_id.to_lowercase();
        let password_owned = password.to_owned();
        let password_hash = tokio::task::spawn_blocking(move || hash_password(&password_owned))
            .await
            .map_err(|_| "falha interna ao preparar senha".to_string())??;

        let mut state = self.state.write().await;
        if state.user_rooms.contains_key(&user_id) {
            return Err("saia da sala atual antes de criar outra".into());
        }
        if state.rooms.contains_key(&room_key) {
            return Err("esse ID de sala já está em uso".into());
        }

        let participant = Participant::new(username, outbound);
        let info = participant.info(user_id);
        let mut participants = HashMap::new();
        participants.insert(user_id, participant);
        state.rooms.insert(
            room_key.clone(),
            Room {
                password_hash,
                participants,
            },
        );
        state.user_rooms.insert(user_id, room_key);
        Ok(RoomEntry {
            room_id: room_id.to_string(),
            participants: vec![info],
        })
    }

    pub async fn join_room(
        &self,
        user_id: Uuid,
        username: &str,
        room_id: &str,
        password: &str,
        outbound: Outbound,
    ) -> Result<RoomEntry, String> {
        validate_room_id(room_id)?;
        if password.is_empty() || password.len() > 128 {
            return Err("senha da sala inválida".into());
        }
        let room_key = room_id.to_lowercase();
        let hash = {
            let state = self.state.read().await;
            if state.user_rooms.contains_key(&user_id) {
                return Err("saia da sala atual antes de entrar em outra".into());
            }
            state
                .rooms
                .get(&room_key)
                .map(|room| room.password_hash.clone())
                .ok_or_else(|| "sala não encontrada".to_string())?
        };
        let expected_hash = hash.clone();
        let password_owned = password.to_owned();
        let valid = tokio::task::spawn_blocking(move || verify_password(&password_owned, &hash))
            .await
            .map_err(|_| "falha interna ao validar senha".to_string())??;
        if !valid {
            return Err("senha incorreta".into());
        }

        let mut state = self.state.write().await;
        if state.user_rooms.contains_key(&user_id) {
            return Err("saia da sala atual antes de entrar em outra".into());
        }
        let room = state
            .rooms
            .get_mut(&room_key)
            .ok_or_else(|| "sala não encontrada".to_string())?;
        if room.password_hash != expected_hash {
            return Err("a sala foi recriada durante a validação; tente novamente".into());
        }
        let participant = Participant::new(username, outbound);
        room.participants.insert(user_id, participant);
        let participants = room
            .participants
            .iter()
            .map(|(id, participant)| participant.info(*id))
            .collect();
        state.user_rooms.insert(user_id, room_key);
        Ok(RoomEntry {
            room_id: room_id.to_string(),
            participants,
        })
    }

    pub async fn broadcast_joined(&self, room_id: &str, user_id: Uuid, username: &str) {
        let message = ServerMessage::ParticipantJoined {
            user_id: user_id.to_string(),
            username: username.to_string(),
        };
        self.broadcast_except(room_id, user_id, message).await;
    }

    pub async fn leave_room(&self, user_id: Uuid) -> Option<LeaveResult> {
        let mut state = self.state.write().await;
        let room_key = state.user_rooms.remove(&user_id)?;
        let room = state.rooms.get_mut(&room_key)?;
        let participant = room.participants.remove(&user_id)?;
        let username = participant.username;
        let message = ServerMessage::ParticipantLeft {
            user_id: user_id.to_string(),
            username: username.clone(),
        };
        let remaining = room
            .participants
            .values()
            .map(|p| (p.outbound.clone(), message.clone()))
            .collect();
        if room.participants.is_empty() {
            state.rooms.remove(&room_key);
        }
        Some(LeaveResult {
            room_id: room_key,
            username,
            remaining,
        })
    }

    pub async fn set_media_state(
        &self,
        user_id: Uuid,
        microphone_enabled: bool,
        camera_enabled: bool,
        screen_sharing: bool,
    ) -> Result<(), String> {
        let mut state = self.state.write().await;
        let room_key = state
            .user_rooms
            .get(&user_id)
            .cloned()
            .ok_or_else(|| "você não está em uma sala".to_string())?;
        let room = state.rooms.get_mut(&room_key).ok_or("sala não encontrada")?;
        let participant = room
            .participants
            .get_mut(&user_id)
            .ok_or("participante não encontrado")?;
        participant.microphone_enabled = microphone_enabled;
        participant.camera_enabled = camera_enabled;
        participant.screen_sharing = screen_sharing;
        let message = ServerMessage::MediaStateChanged {
            user_id: user_id.to_string(),
            username: participant.username.clone(),
            microphone_enabled,
            camera_enabled,
            screen_sharing,
        };
        for peer in room.participants.values() {
            if peer.outbound.try_send(message.clone()).is_err() {
                // Clientes lentos serão removidos quando a conexão cair.
            }
        }
        Ok(())
    }

    pub async fn relay_signal(
        &self,
        sender_id: Uuid,
        target_user: &str,
        payload: serde_json::Value,
    ) -> Result<(), String> {
        let target_id = Uuid::parse_str(target_user)
            .map_err(|_| "target_user deve ser o user_id UUID do participante".to_string())?;
        let state = self.state.read().await;
        let sender_room = state
            .user_rooms
            .get(&sender_id)
            .ok_or_else(|| "você não está em uma sala".to_string())?;
        if state.user_rooms.get(&target_id) != Some(sender_room) {
            return Err("destinatário não está na sua sala".into());
        }
        let outbound = state
            .rooms
            .get(sender_room)
            .and_then(|room| room.participants.get(&target_id))
            .map(|target| target.outbound.clone())
            .ok_or_else(|| "destinatário não encontrado".to_string())?;
        drop(state);
        outbound
            .send(ServerMessage::WebrtcSignal {
                from_user: sender_id.to_string(),
                payload,
            })
            .await
            .map_err(|_| "não foi possível entregar a sinalização".to_string())
    }

    async fn broadcast_except(&self, room_id: &str, except: Uuid, message: ServerMessage) {
        let state = self.state.read().await;
        if let Some(room) = state.rooms.get(&room_id.to_lowercase()) {
            for (id, participant) in &room.participants {
                if *id != except {
                    let _ = participant.outbound.try_send(message.clone());
                }
            }
        }
    }
}

impl Participant {
    fn new(username: &str, outbound: Outbound) -> Self {
        Self {
            username: username.to_string(),
            outbound,
            microphone_enabled: true,
            camera_enabled: false,
            screen_sharing: false,
        }
    }

    fn info(&self, user_id: Uuid) -> ParticipantInfo {
        ParticipantInfo {
            user_id: user_id.to_string(),
            username: self.username.clone(),
            microphone_enabled: self.microphone_enabled,
            camera_enabled: self.camera_enabled,
            screen_sharing: self.screen_sharing,
        }
    }
}

fn validate_room_id(room_id: &str) -> Result<(), String> {
    let valid = !room_id.is_empty()
        && room_id.len() <= 64
        && room_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        Ok(())
    } else {
        Err("ID da sala deve ter de 1 a 64 caracteres: letras, números, - ou _".into())
    }
}

fn validate_password(password: &str) -> Result<(), String> {
    if (8..=128).contains(&password.len()) {
        Ok(())
    } else {
        Err("a senha da sala deve ter entre 8 e 128 bytes".into())
    }
}

fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| "não foi possível proteger a senha da sala".into())
}

fn verify_password(password: &str, encoded_hash: &str) -> Result<bool, String> {
    let parsed = PasswordHash::new(encoded_hash)
        .map_err(|_| "hash de senha inválido".to_string())?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}
