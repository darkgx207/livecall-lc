use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Register { username: String },
    CreateRoom { room_id: String, password: String },
    JoinRoom { room_id: String, password: String },
    LeaveRoom,
    SetMediaState {
        microphone_enabled: bool,
        camera_enabled: bool,
        screen_sharing: bool,
    },
    WebrtcSignal { target_user: String, payload: Value },
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Response {
        status: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        user_id: Option<String>,
    },
    RoomState {
        room_id: String,
        participants: Vec<ParticipantInfo>,
    },
    ParticipantJoined { user_id: String, username: String },
    ParticipantLeft { user_id: String, username: String },
    MediaStateChanged {
        user_id: String,
        username: String,
        microphone_enabled: bool,
        camera_enabled: bool,
        screen_sharing: bool,
    },
    WebrtcSignal { from_user: String, payload: Value },
}

#[derive(Debug, Serialize, Clone)]
pub struct ParticipantInfo {
    pub user_id: String,
    pub username: String,
    pub microphone_enabled: bool,
    pub camera_enabled: bool,
    pub screen_sharing: bool,
}
