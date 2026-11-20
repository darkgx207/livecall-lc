use crate::{
    protocol::{ClientMessage, ServerMessage},
    room_manager::RoomManager,
};
use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, warn};
use uuid::Uuid;

const OUTBOUND_BUFFER: usize = 64;

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(manager): State<Arc<RoomManager>>,
) -> impl IntoResponse {
    ws.max_message_size(64 * 1024)
        .max_frame_size(64 * 1024)
        .on_upgrade(move |socket| handle_socket(socket, manager))
}

async fn handle_socket(socket: WebSocket, manager: Arc<RoomManager>) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<ServerMessage>(OUTBOUND_BUFFER);
    let mut identity: Option<(Uuid, String)> = None;

    loop {
        tokio::select! {
            outgoing = outbound_rx.recv() => {
                match outgoing {
                    Some(message) => {
                        let Ok(json) = serde_json::to_string(&message) else { continue; };
                        if ws_sender.send(Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            incoming = ws_receiver.next() => {
                let Some(Ok(message)) = incoming else { break; };
                match message {
                    Message::Text(text) => {
                        let parsed = serde_json::from_str::<ClientMessage>(text.as_str());
                        let message = match parsed {
                            Ok(message) => message,
                            Err(_) => {
                                send_response(&outbound_tx, "error", "JSON ou tipo de mensagem inválido", None).await;
                                continue;
                            }
                        };
                        if identity.is_none() {
                            match message {
                                ClientMessage::Register { username } => {
                                    match validate_username(&username) {
                                        Ok(()) => {
                                            let user_id = Uuid::new_v4();
                                            identity = Some((user_id, username.trim().to_string()));
                                            send_response(&outbound_tx, "success", "usuário registrado", Some(user_id.to_string())).await;
                                        }
                                        Err(error) => send_response(&outbound_tx, "error", &error, None).await,
                                    }
                                }
                                _ => send_response(&outbound_tx, "error", "envie register antes de qualquer outra operação", None).await,
                            }
                            continue;
                        }

                        let (user_id, username) = identity.as_ref().expect("identity validada");
                        match message {
                            ClientMessage::Register { .. } => {
                                send_response(&outbound_tx, "error", "usuário já registrado nesta conexão", None).await;
                            }
                            ClientMessage::CreateRoom { room_id, password } => {
                                match manager.create_room(*user_id, username, &room_id, &password, outbound_tx.clone()).await {
                                    Ok(entry) => {
                                        send_response(&outbound_tx, "success", "sala criada e conexão estabelecida", Some(user_id.to_string())).await;
                                        let _ = outbound_tx.send(ServerMessage::RoomState { room_id: entry.room_id, participants: entry.participants }).await;
                                    }
                                    Err(error) => send_response(&outbound_tx, "error", &error, None).await,
                                }
                            }
                            ClientMessage::JoinRoom { room_id, password } => {
                                match manager.join_room(*user_id, username, &room_id, &password, outbound_tx.clone()).await {
                                    Ok(entry) => {
                                        manager.broadcast_joined(&entry.room_id, *user_id, username).await;
                                        send_response(&outbound_tx, "success", "conectado à sala com sucesso", Some(user_id.to_string())).await;
                                        let _ = outbound_tx.send(ServerMessage::RoomState { room_id: entry.room_id, participants: entry.participants }).await;
                                    }
                                    Err(error) => send_response(&outbound_tx, "error", &error, None).await,
                                }
                            }
                            ClientMessage::LeaveRoom => {
                                if let Some(result) = manager.leave_room(*user_id).await {
                                    for (tx, event) in result.remaining {
                                        let _ = tx.try_send(event);
                                    }
                                    send_response(&outbound_tx, "success", "você saiu da sala", None).await;
                                } else {
                                    send_response(&outbound_tx, "error", "você não está em uma sala", None).await;
                                }
                            }
                            ClientMessage::SetMediaState { microphone_enabled, camera_enabled, screen_sharing } => {
                                match manager.set_media_state(*user_id, microphone_enabled, camera_enabled, screen_sharing).await {
                                    Ok(()) => send_response(&outbound_tx, "success", "status de mídia atualizado", None).await,
                                    Err(error) => send_response(&outbound_tx, "error", &error, None).await,
                                }
                            }
                            ClientMessage::WebrtcSignal { target_user, payload } => {
                                if let Err(error) = manager.relay_signal(*user_id, &target_user, payload).await {
                                    send_response(&outbound_tx, "error", &error, None).await;
                                }
                            }
                        }
                    }
                    Message::Ping(payload) => {
                        if ws_sender.send(Message::Pong(payload)).await.is_err() { break; }
                    }
                    Message::Close(_) => break,
                    Message::Pong(_) | Message::Binary(_) => {}
                }
            }
        }
    }

    if let Some((user_id, _)) = identity {
        if let Some(result) = manager.leave_room(user_id).await {
            for (tx, event) in result.remaining {
                let _ = tx.try_send(event);
            }
        }
    }
    debug!("conexão WebSocket encerrada");
}

async fn send_response(tx: &mpsc::Sender<ServerMessage>, status: &str, message: &str, user_id: Option<String>) {
    if tx.send(ServerMessage::Response {
        status: status.to_string(),
        message: message.to_string(),
        user_id,
    }).await.is_err() {
        warn!("não foi possível enviar resposta ao cliente");
    }
}

fn validate_username(username: &str) -> Result<(), String> {
    let trimmed = username.trim();
    if trimmed.is_empty() || trimmed.len() > 32 || trimmed.chars().any(char::is_control) {
        Err("username deve ter de 1 a 32 caracteres e não pode conter controles".into())
    } else {
        Ok(())
    }
}
