mod media;

use futures_util::{SinkExt, StreamExt};
use iced::{
    widget::{button, column, container, row, scrollable, slider, text, text_input, Space},
    Alignment, Element, Length, Subscription, Task, Theme,
};
use serde::{Deserialize, Serialize};
use std::{collections::{HashMap, VecDeque}, sync::{Arc, Mutex}};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

const DEFAULT_SERVER: &str = "ws://127.0.0.1:3000/ws";

fn main() -> iced::Result {
    iced::application("LiveCall", update, view)
        .subscription(subscription)
        .theme(|_| Theme::Dark)
        .window_size((1100.0, 720.0))
        .centered()
        .run()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen { Login, Lobby, Call }

#[derive(Debug, Clone)]
enum Message {
    UsernameChanged(String),
    ServerUrlChanged(String),
    RoomIdChanged(String),
    PasswordChanged(String),
    Continue,
    ChangeServer,
    ConnectionClosed,
    CreateRoom,
    JoinRoom,
    ToggleMicrophone,
    ToggleDeafen,
    TogglePeerVolumePanel(String),
    SetPeerVolume(String, u8),
    ToggleCamera,
    ToggleScreenShare,
    LeaveCall,
    Connected(Result<(Network, Arc<media::MediaEngine>), String>),
    ServerEvent(ServerEvent),
    PeerOperationResult(Result<(), String>),
    PollNetwork,
}

#[derive(Debug, Clone)]
struct Participant {
    user_id: String,
    username: String,
    microphone_enabled: bool,
    camera_enabled: bool,
    screen_sharing: bool,
}

#[derive(Debug, Clone)]
enum ServerEvent {
    Response { status: String, message: String, user_id: Option<String> },
    RoomState { room_id: String, participants: Vec<Participant> },
    ParticipantJoined { user_id: String, username: String },
    ParticipantLeft { user_id: String, username: String },
    MediaStateChanged { user_id: String, username: String, microphone_enabled: bool, camera_enabled: bool, screen_sharing: bool },
    WebrtcSignal { from_user: String, payload: serde_json::Value },
    Disconnected(String),
}

#[derive(Debug, Clone)]
struct Network {
    outbound: mpsc::UnboundedSender<String>,
    inbound: Arc<Mutex<VecDeque<ServerEvent>>>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Register { username: String },
    CreateRoom { room_id: String, password: String },
    JoinRoom { room_id: String, password: String },
    LeaveRoom,
    SetMediaState { microphone_enabled: bool, camera_enabled: bool, screen_sharing: bool },
    WebrtcSignal { target_user: String, payload: serde_json::Value },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum IncomingMessage {
    Response { status: String, message: String, user_id: Option<String> },
    RoomState { room_id: String, participants: Vec<IncomingParticipant> },
    ParticipantJoined { user_id: String, username: String },
    ParticipantLeft { user_id: String, username: String },
    MediaStateChanged { user_id: String, username: String, microphone_enabled: bool, camera_enabled: bool, screen_sharing: bool },
    WebrtcSignal { from_user: String, payload: serde_json::Value },
}

#[derive(Debug, Deserialize)]
struct IncomingParticipant {
    user_id: String,
    username: String,
    microphone_enabled: bool,
    camera_enabled: bool,
    screen_sharing: bool,
}

struct App {
    screen: Screen,
    username: String,
    server_url: String,
    room_id: String,
    password: String,
    active_room: String,
    participants: Vec<Participant>,
    peer_volumes: HashMap<String, u8>,
    open_volume_panel: Option<String>,
    network: Option<Network>,
    media_engine: Option<Arc<media::MediaEngine>>,
    local_user_id: Option<String>,
    microphone_enabled: bool,
    deafened: bool,
    microphone_before_deafen: Option<bool>,
    camera_enabled: bool,
    screen_sharing: bool,
    busy: bool,
    status: String,
    last_signal: Option<String>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            screen: Screen::Login,
            username: String::new(),
            server_url: DEFAULT_SERVER.into(),
            room_id: String::new(),
            password: String::new(),
            active_room: String::new(),
            participants: Vec::new(),
            peer_volumes: HashMap::new(),
            open_volume_panel: None,
            network: None,
            media_engine: None,
            local_user_id: None,
            microphone_enabled: true,
            deafened: false,
            microphone_before_deafen: None,
            camera_enabled: false,
            screen_sharing: false,
            busy: false,
            status: "Servidor local: 127.0.0.1:3000".into(),
            last_signal: None,
        }
    }
}

fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::UsernameChanged(value) => app.username = value,
        Message::ServerUrlChanged(value) => app.server_url = value,
        Message::RoomIdChanged(value) => app.room_id = value,
        Message::PasswordChanged(value) => app.password = value,
        Message::Continue => {
            if app.username.trim().is_empty() {
                app.status = "Informe um username para continuar.".into();
            } else {
                app.busy = true;
                app.status = "Conectando ao servidor...".into();
                let url = app.server_url.trim().to_string();
                let username = app.username.trim().to_string();
                return Task::perform(connect_and_register(url, username), Message::Connected);
            }
        }
        Message::Connected(result) => {
            app.busy = false;
            match result {
                Ok((network, engine)) => {
                    app.network = Some(network);
                    let audio_issue = engine.audio_error().map(str::to_string);
                    app.microphone_enabled = audio_issue.is_none();
                    app.deafened = false;
                    app.microphone_before_deafen = None;
                    app.media_engine = Some(engine);
                    app.screen = Screen::Lobby;
                    app.status = match audio_issue {
                        Some(issue) => format!("Conectado como {} · áudio indisponível: {issue}", app.username.trim()),
                        None => format!("Conectado como {} · microfone e reprodução preparados", app.username.trim()),
                    };
                }
                Err(error) => app.status = format!("Falha na conexão: {error}"),
            }
        }
        Message::ChangeServer => {
            // The lobby has an established connection; dispose of it before editing URL.
            let old_engine = app.media_engine.take();
            app.network = None;
            app.local_user_id = None;
            app.active_room.clear();
            app.participants.clear();
            app.peer_volumes.clear();
            app.open_volume_panel = None;
            app.microphone_before_deafen = None;
            app.deafened = false;
            app.camera_enabled = false;
            app.screen_sharing = false;
            app.busy = false;
            app.screen = Screen::Login;
            app.status = "Conexão anterior encerrada. Você pode alterar o servidor.".into();
            if let Some(engine) = old_engine {
                return Task::perform(async move { engine.close_all().await; }, |_| Message::ConnectionClosed);
            }
        }
        Message::ConnectionClosed => {}
        Message::CreateRoom => {
            if app.room_id.trim().is_empty() || app.password.is_empty() {
                app.status = "Informe o ID e a senha da sala.".into();
            } else if send(&app.network, ClientMessage::CreateRoom { room_id: app.room_id.trim().into(), password: app.password.clone() }).is_err() {
                app.status = "Não foi possível enviar a solicitação ao servidor.".into();
            } else {
                app.status = "Criando sala...".into();
            }
        }
        Message::JoinRoom => {
            if app.room_id.trim().is_empty() || app.password.is_empty() {
                app.status = "Informe o ID e a senha da sala.".into();
            } else if send(&app.network, ClientMessage::JoinRoom { room_id: app.room_id.trim().into(), password: app.password.clone() }).is_err() {
                app.status = "Não foi possível enviar a solicitação ao servidor.".into();
            } else {
                app.status = "Entrando na sala...".into();
            }
        }
        Message::ToggleMicrophone => {
            if app.deafened {
                app.status = "Desative Ensurdecido antes de ligar o microfone.".into();
                return Task::none();
            }
            if let Some(engine) = &app.media_engine {
                if let Some(error) = engine.audio_error() {
                    app.status = format!("Áudio indisponível: {error}");
                    return Task::none();
                }
                app.microphone_enabled = !app.microphone_enabled;
                engine.set_microphone_enabled(app.microphone_enabled);
                update_media(app);
                app.status = if app.microphone_enabled { "Microfone transmitindo áudio Opus.".into() } else { "Microfone mutado: silêncio enviado aos participantes.".into() };
            }
        }
        Message::ToggleDeafen => {
            if let Some(engine) = &app.media_engine {
                if let Some(error) = engine.audio_error() {
                    app.status = format!("Áudio indisponível: {error}");
                    return Task::none();
                }
                if !app.deafened {
                    // Remember the user's prior mute choice, including an already-muted mic.
                    app.microphone_before_deafen = Some(app.microphone_enabled);
                    app.deafened = true;
                    app.microphone_enabled = false;
                    engine.set_microphone_enabled(false);
                    engine.set_deafened(true);
                    app.status = "Ensurdecido: microfone mutado e áudio recebido silenciado.".into();
                } else {
                    app.deafened = false;
                    // Restore the user's previous choice instead of always enabling the mic.
                    app.microphone_enabled = app.microphone_before_deafen.take().unwrap_or(false);
                    engine.set_microphone_enabled(app.microphone_enabled);
                    engine.set_deafened(false);
                    app.status = if app.microphone_enabled {
                        "Áudio recebido restaurado; microfone novamente ligado.".into()
                    } else {
                        "Áudio recebido restaurado; microfone permanece mutado.".into()
                    };
                }
                // Inform all participants that the real transmitted microphone state changed.
                update_media(app);
            }
        }
        Message::TogglePeerVolumePanel(peer_id) => {
            if !is_remote_participant(app, &peer_id) { return Task::none(); }
            if app.open_volume_panel.as_deref() == Some(peer_id.as_str()) {
                app.open_volume_panel = None;
            } else {
                app.open_volume_panel = Some(peer_id);
            }
        }
        Message::SetPeerVolume(peer_id, percent) => {
            if !is_remote_participant(app, &peer_id) { return Task::none(); }
            let volume = percent.min(100);
            app.peer_volumes.insert(peer_id.clone(), volume);
            if let Some(engine) = &app.media_engine {
                engine.set_peer_volume(&peer_id, volume);
            }
            app.status = format!("Volume local do participante: {volume}%");
        }
        Message::ToggleCamera => {
            app.camera_enabled = !app.camera_enabled;
            update_media(app);
        }
        Message::ToggleScreenShare => {
            app.screen_sharing = !app.screen_sharing;
            update_media(app);
        }
        Message::LeaveCall => {
            let _ = send(&app.network, ClientMessage::LeaveRoom);
            if let Some(engine) = app.media_engine.clone() {
                let close_task = Task::perform(async move { engine.close_all().await; }, |_| Message::PeerOperationResult(Ok(())));
                app.screen = Screen::Lobby;
                app.active_room.clear();
                app.participants.clear();
                app.peer_volumes.clear();
                app.open_volume_panel = None;
                app.screen_sharing = false;
                app.camera_enabled = false;
                if let Some(previous) = app.microphone_before_deafen.take() { app.microphone_enabled = previous; }
                app.deafened = false;
                app.status = "Você saiu da chamada; conexões P2P encerradas.".into();
                return close_task;
            }
            app.screen = Screen::Lobby;
            app.active_room.clear();
            app.participants.clear();
            app.peer_volumes.clear();
            app.open_volume_panel = None;
            app.screen_sharing = false;
            app.camera_enabled = false;
            if let Some(previous) = app.microphone_before_deafen.take() { app.microphone_enabled = previous; }
            app.deafened = false;
            app.status = "Você saiu da chamada.".into();
        }
        Message::ServerEvent(event) => match event {
            ServerEvent::Response { status, message, user_id } => {
                if status == "success" { if let Some(id) = user_id { app.local_user_id = Some(id); } }
                app.status = message;
                if status == "error" { app.busy = false; }
            }
            ServerEvent::RoomState { room_id, participants } => {
                app.active_room = room_id;
                app.participants = participants.clone();
                app.peer_volumes.clear();
                app.open_volume_panel = None;
                app.screen = Screen::Call;
                app.status = "Sala conectada; preparando áudio Opus P2P...".into();
                if let Some(engine) = &app.media_engine {
                    engine.set_microphone_enabled(app.microphone_enabled);
                    engine.set_deafened(app.deafened);
                }
                update_media(app);
                let local_id = app.local_user_id.clone();
                let engine = app.media_engine.clone();
                let tasks = participants.iter().filter_map(|p| {
                    let local = local_id.as_deref()?;
                    if p.user_id != local && local < p.user_id.as_str() {
                        let engine = engine.clone()?; let peer = p.user_id.clone();
                        Some(Task::perform(async move { engine.create_offer(&peer).await }, Message::PeerOperationResult))
                    } else { None }
                }).collect::<Vec<_>>();
                return Task::batch(tasks);
            }
            ServerEvent::ParticipantJoined { user_id, username } => {
                if !app.participants.iter().any(|p| p.user_id == user_id) {
                    app.participants.push(Participant { user_id: user_id.clone(), username, microphone_enabled: true, camera_enabled: false, screen_sharing: false });
                }
                if app.local_user_id.as_deref().is_some_and(|local| local < user_id.as_str()) {
                    if let Some(engine) = app.media_engine.clone() {
                        return Task::perform(async move { engine.create_offer(&user_id).await }, Message::PeerOperationResult);
                    }
                }
            }
            ServerEvent::ParticipantLeft { user_id, .. } => {
                app.participants.retain(|p| p.user_id != user_id);
                app.peer_volumes.remove(&user_id);
                if app.open_volume_panel.as_deref() == Some(user_id.as_str()) { app.open_volume_panel = None; }
                if let Some(engine) = app.media_engine.clone() {
                    return Task::perform(async move { engine.close_peer(&user_id).await; Ok(()) }, Message::PeerOperationResult);
                }
            }
            ServerEvent::MediaStateChanged { user_id, username, microphone_enabled, camera_enabled, screen_sharing } => {
                if let Some(p) = app.participants.iter_mut().find(|p| p.user_id == user_id) {
                    p.username = username;
                    p.microphone_enabled = microphone_enabled;
                    p.camera_enabled = camera_enabled;
                    p.screen_sharing = screen_sharing;
                }
            }
            ServerEvent::WebrtcSignal { from_user, payload } => {
                app.last_signal = Some(format!("Sinalização recebida de {from_user}: {}", signal_kind(&payload)));
                app.status = format!("Negociando WebRTC com {from_user}...");
                if let Some(engine) = app.media_engine.clone() {
                    return Task::perform(async move { engine.handle_signal(&from_user, payload).await }, Message::PeerOperationResult);
                }
            }
            ServerEvent::Disconnected(reason) => {
                if let Some(previous) = app.microphone_before_deafen.take() { app.microphone_enabled = previous; }
                app.deafened = false;
                app.peer_volumes.clear();
                app.open_volume_panel = None;
                app.network = None;
                app.screen = Screen::Login;
                app.status = format!("Conexão encerrada: {reason}");
                if let Some(engine) = app.media_engine.take() {
                    return Task::perform(async move { engine.close_all().await; }, |_| Message::ConnectionClosed);
                }
            }
        },
        Message::PeerOperationResult(result) => {
            match result {
                Ok(()) => app.status = "Sinalização P2P processada; aguardando ICE/DTLS.".into(),
                Err(error) => app.status = format!("Falha na conexão WebRTC: {error}"),
            }
        }
        Message::PollNetwork => {
            if let Some(network) = &app.network {
                if let Ok(mut queue) = network.inbound.lock() {
                    if let Some(event) = queue.pop_front() { return Task::done(Message::ServerEvent(event)); }
                }
            }
        }
    }
    Task::none()
}

fn is_remote_participant(app: &App, peer_id: &str) -> bool {
    app.local_user_id.as_deref() != Some(peer_id)
        && app.participants.iter().any(|p| p.user_id == peer_id)
}

fn signal_kind(payload: &serde_json::Value) -> &str {
    payload.get("kind").and_then(serde_json::Value::as_str)
        .or_else(|| payload.get("type").and_then(serde_json::Value::as_str))
        .unwrap_or("mensagem")
}

// Ponto de entrada usado pela camada WebRTC na próxima etapa: encaminha offer,
// answer e ICE candidate sem acoplar o protocolo de sinalização à UI.
fn send_webrtc_signal(network: &Option<Network>, target_user: String, payload: serde_json::Value) -> Result<(), ()> {
    send(network, ClientMessage::WebrtcSignal { target_user, payload })
}

fn update_media(app: &mut App) {
    if send(&app.network, ClientMessage::SetMediaState {
        microphone_enabled: app.microphone_enabled,
        camera_enabled: app.camera_enabled,
        screen_sharing: app.screen_sharing,
    }).is_err() {
        app.status = "Entre em uma sala antes de alterar os estados de mídia.".into();
    }
}

fn send(network: &Option<Network>, message: ClientMessage) -> Result<(), ()> {
    let network = network.as_ref().ok_or(())?;
    let json = serde_json::to_string(&message).map_err(|_| ())?;
    network.outbound.send(json).map_err(|_| ())
}

async fn connect_and_register(url: String, username: String) -> Result<(Network, Arc<media::MediaEngine>), String> {
    let (socket, _) = connect_async(&url).await.map_err(|e| e.to_string())?;
    let (mut writer, mut reader) = socket.split();
    let (outbound, mut outgoing) = mpsc::unbounded_channel::<String>();
    let inbound = Arc::new(Mutex::new(VecDeque::new()));
    let inbound_worker = inbound.clone();
    let register = serde_json::to_string(&ClientMessage::Register { username }).map_err(|e| e.to_string())?;
    outbound.send(register).map_err(|e| e.to_string())?;

    tokio::spawn(async move {
        loop {
            tokio::select! {
                outgoing_message = outgoing.recv() => {
                    match outgoing_message {
                        Some(json) => if writer.send(WsMessage::Text(json.into())).await.is_err() { break; },
                        None => break,
                    }
                }
                incoming_message = reader.next() => {
                    match incoming_message {
                        Some(Ok(WsMessage::Text(text))) => {
                            if let Ok(parsed) = serde_json::from_str::<IncomingMessage>(&text) {
                                let event = match parsed {
                                    IncomingMessage::Response { status, message, user_id } => Some(ServerEvent::Response { status, message, user_id }),
                                    IncomingMessage::RoomState { room_id, participants } => Some(ServerEvent::RoomState { room_id, participants: participants.into_iter().map(|p| Participant { user_id: p.user_id, username: p.username, microphone_enabled: p.microphone_enabled, camera_enabled: p.camera_enabled, screen_sharing: p.screen_sharing }).collect() }),
                                    IncomingMessage::ParticipantJoined { user_id, username } => Some(ServerEvent::ParticipantJoined { user_id, username }),
                                    IncomingMessage::ParticipantLeft { user_id, username } => Some(ServerEvent::ParticipantLeft { user_id, username }),
                                    IncomingMessage::MediaStateChanged { user_id, username, microphone_enabled, camera_enabled, screen_sharing } => Some(ServerEvent::MediaStateChanged { user_id, username, microphone_enabled, camera_enabled, screen_sharing }),
                                    IncomingMessage::WebrtcSignal { from_user, payload } => Some(ServerEvent::WebrtcSignal { from_user, payload }),
                                };
                                if let Some(event) = event { if let Ok(mut queue) = inbound_worker.lock() { queue.push_back(event); } }
                            }
                        }
                        Some(Ok(WsMessage::Ping(_))) | Some(Ok(WsMessage::Pong(_))) => {}
                        Some(Ok(WsMessage::Close(_))) | None => break,
                        Some(Err(error)) => {
                            if let Ok(mut queue) = inbound_worker.lock() { queue.push_back(ServerEvent::Disconnected(error.to_string())); }
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
        if let Ok(mut queue) = inbound_worker.lock() { queue.push_back(ServerEvent::Disconnected("servidor encerrou a conexão".into())); }
    });
    let engine = media::MediaEngine::new(media::MediaConfig::default(), outbound.clone()).await?;
    Ok((Network { outbound, inbound }, Arc::new(engine)))
}

fn subscription(_app: &App) -> Subscription<Message> {
    iced::time::every(std::time::Duration::from_millis(100)).map(|_| Message::PollNetwork)
}

fn view(app: &App) -> Element<'_, Message> {
    let content: Element<'_, Message> = match app.screen {
        Screen::Login => login_view(app),
        Screen::Lobby => lobby_view(app),
        Screen::Call => call_view(app),
    };
    container(column![
        row![text("LIVE").size(15), text("CALL").size(15)].spacing(2).align_y(Alignment::Center),
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        text(&app.status).size(13).color(if app.status.contains("Falha") || app.status.contains("indisponível") || app.status.contains("Não foi possível") || app.status.contains("Informe") || app.status.contains("encerrada") {
            iced::Color::from_rgb(0.89, 0.43, 0.43)
        } else { iced::Color::from_rgb(0.21, 0.72, 0.47) }),
    ].spacing(16).align_x(Alignment::Center))
        .width(Length::Fill).height(Length::Fill)
        .padding(24)
        .into()
}

fn login_view(app: &App) -> Element<'_, Message> {
    container(column![
        text("Sua conversa começa aqui").size(32),
        text("Entre com um nome. Sem conta, sem cadastro.").size(15),
        Space::with_height(12),
        text_input("Seu username", &app.username).on_input(Message::UsernameChanged).padding(12).size(16),
        text_input("ws://127.0.0.1:3000/ws", &app.server_url).on_input(Message::ServerUrlChanged).padding(10),
        button(if app.busy { "Conectando..." } else { "Avançar" }).on_press_maybe((!app.busy).then_some(Message::Continue)).padding([10, 24]),
    ].spacing(14).width(Length::Fixed(380.0)).align_x(Alignment::Center))
        .padding(28).style(container::rounded_box).into()
}

fn lobby_view(app: &App) -> Element<'_, Message> {
    container(column![
        text("Lobby").size(30),
        text(format!("Olá, {}", app.username)).size(16),
        Space::with_height(8),
        text_input("ID da sala", &app.room_id).on_input(Message::RoomIdChanged).padding(12),
        text_input("Senha da sala", &app.password).on_input(Message::PasswordChanged).secure(true).padding(12),
        row![
            button("Criar sala").on_press(Message::CreateRoom).padding([10, 20]),
            button("Entrar na sala").on_press(Message::JoinRoom).padding([10, 20]),
        ].spacing(12),
        button("Alterar servidor").on_press(Message::ChangeServer).padding([8, 14]),
        text("As salas existem enquanto houver participantes conectados.").size(13),
    ].spacing(14).width(Length::Fixed(440.0)).align_x(Alignment::Center))
        .padding(28).style(container::rounded_box).into()
}

fn call_view(app: &App) -> Element<'_, Message> {
    let participant_list = app.participants.iter().fold(column![].spacing(8), |list, participant| {
        let mic = if participant.microphone_enabled { "Mic ligado" } else { "Mic mudo" };
        let camera = if participant.camera_enabled { "Câmera ligada (visual)" } else { "Sem câmera" };
        let screen = if participant.screen_sharing { " · Tela ativa (visual)" } else { "" };
        let is_local = app.local_user_id.as_deref() == Some(participant.user_id.as_str());
        let volume = *app.peer_volumes.get(&participant.user_id).unwrap_or(&100);
        let is_open = app.open_volume_panel.as_deref() == Some(participant.user_id.as_str());
        let participant_row = row![
            text("●").size(18).color(if participant.microphone_enabled { iced::Color::from_rgb(0.21, 0.72, 0.47) } else { iced::Color::from_rgb(0.89, 0.43, 0.43) }),
            column![
                text(&participant.username).size(17),
                text(format!("{mic} · {camera}{screen}")).size(12)
                    .color(if participant.microphone_enabled { iced::Color::from_rgb(0.21, 0.72, 0.47) } else { iced::Color::from_rgb(0.89, 0.43, 0.43) })
            ].width(Length::Fill),
        ].spacing(12).align_y(Alignment::Center);
        let participant_row = if is_local {
            participant_row.push(text("Você").size(12))
        } else {
            // The watch button is intentionally inactive until a genuine video track exists.
            participant_row
                .push(button("Assistir (em breve)").padding([5, 9]))
                .push(button(text(format!("🔊 {volume}%")).size(12)
                    .color(if volume == 0 { iced::Color::from_rgb(0.89, 0.43, 0.43) } else { iced::Color::from_rgb(0.21, 0.72, 0.47) }))
                    .on_press(Message::TogglePeerVolumePanel(participant.user_id.clone()))
                    .padding([5, 9]))
        };
        let card = container(participant_row)
            .padding([8, 14])
            .height(Length::Fixed(54.0))
            .width(Length::Fill)
            .style(dark_card);
        if !is_local && is_open {
            let id = participant.user_id.clone();
            let volume_control = container(column![
                row![text("Volume individual").size(12), Space::with_width(Length::Fill), text(format!("{volume}%")).size(12)].align_y(Alignment::Center),
                slider(0..=100, volume, move |value| Message::SetPeerVolume(id.clone(), value)).step(1),
            ].spacing(6)).padding([10, 14]).width(Length::Fixed(260.0)).style(dark_popup);
            list.push(column![card, row![Space::with_width(Length::Fill), volume_control].spacing(0)].spacing(4))
        } else {
            list.push(card)
        }
    });
    let mic_button = button(text(if app.microphone_enabled { "Microfone: ligado" } else { "Microfone: mudo" })
        .color(if app.microphone_enabled { iced::Color::from_rgb(0.21, 0.72, 0.47) } else { iced::Color::from_rgb(0.89, 0.43, 0.43) }));
    let mic_button = if app.deafened { mic_button } else { mic_button.on_press(Message::ToggleMicrophone) };
    container(column![
        container(row![
            column![text("Sala").size(13), text(&app.active_room).size(27)],
            Space::with_width(Length::Fill),
            text(format!("{} participante(s)", app.participants.len())).size(14),
        ].align_y(Alignment::Center))
            .padding(14)
            .width(Length::Fill)
            .style(dark_header),
        // Altura fixa: comporta cinco cartões de participante; o excedente usa scroll.
        container(scrollable(participant_list).height(Length::Fixed(310.0)))
            .width(Length::Fill)
            .padding(8),
        row![
            mic_button,
            button(text(if app.camera_enabled { "Câmera: ligada (visual)" } else { "Câmera: desligada (visual)" })
                .color(if app.camera_enabled { iced::Color::from_rgb(0.21, 0.72, 0.47) } else { iced::Color::from_rgb(0.89, 0.43, 0.43) }))
                .on_press(Message::ToggleCamera),
            button(text(if app.screen_sharing { "Parar compartilhamento (visual)" } else { "Compartilhar tela (visual)" }).color(if app.screen_sharing { iced::Color::from_rgb(0.21, 0.72, 0.47) } else { iced::Color::from_rgb(0.89, 0.43, 0.43) })).on_press(Message::ToggleScreenShare),
            button("Sair").on_press(Message::LeaveCall),
        ].spacing(10).align_y(Alignment::Center),
        row![
            button(text(if app.deafened { "Ensurdecido: ligado" } else { "Ensurdecer" })
                .color(if app.deafened { iced::Color::from_rgb(0.89, 0.43, 0.43) } else { iced::Color::from_rgb(0.21, 0.72, 0.47) }))
                .on_press(Message::ToggleDeafen),
            text("Silencia a recepção e muta o microfone; ao voltar, restaura o mute anterior.").size(12),
        ].spacing(12).align_y(Alignment::Center),
        text(if app.media_engine.as_ref().is_some_and(|e| e.audio_error().is_none()) { "Áudio: Opus 48 kHz P2P preparado (aguardando fluxo remoto)" } else { "Áudio indisponível: verifique dispositivos e permissões" }).size(13)
            .color(if app.media_engine.as_ref().is_some_and(|e| e.audio_error().is_none()) { iced::Color::from_rgb(0.21, 0.72, 0.47) } else { iced::Color::from_rgb(0.89, 0.43, 0.43) }),
        text(app.last_signal.as_deref().unwrap_or("Aguardando sinalização dos participantes...")).size(11)
            .color(iced::Color::from_rgb(0.62, 0.66, 0.72)),
    ].spacing(12).width(Length::Fill).height(Length::Shrink))
        .padding(20).width(Length::Fill).style(dark_call_background).into()
}

fn dark_header(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgb8(36, 40, 47))),
        text_color: Some(iced::Color::from_rgb8(235, 239, 244)),
        border: iced::Border { radius: 8.0.into(), ..Default::default() },
        ..Default::default()
    }
}

fn dark_card(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgb8(46, 51, 60))),
        border: iced::Border { radius: 7.0.into(), ..Default::default() },
        ..Default::default()
    }
}

fn dark_popup(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgb8(39, 43, 51))),
        border: iced::Border { radius: 7.0.into(), ..Default::default() },
        ..Default::default()
    }
}

fn dark_call_background(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgb8(31, 34, 40))),
        border: iced::Border { radius: 9.0.into(), ..Default::default() },
        ..Default::default()
    }
}

#[cfg(test)]
mod volume_tests {
    #[test]
    fn volume_slider_bounds_are_inclusive() {
        assert_eq!(0u8.min(100), 0);
        assert_eq!(50u8.min(100), 50);
        assert_eq!(100u8.min(100), 100);
        assert_eq!(255u8.min(100), 100);
    }
}
