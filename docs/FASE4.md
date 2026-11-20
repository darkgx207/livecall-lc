# Fase 4 — Mídia WebRTC (plano incremental)

A Fase 3 validou o transporte de mensagens de sinalização. Isso ainda não cria uma conexão WebRTC nem transmite mídia. A Fase 4 será implementada em incrementos pequenos, com validação entre eles.

## Decisão de arquitetura

- Manter o backend Axum como servidor de sinalização, sem transportar áudio/vídeo.
- Começar com malha P2P WebRTC para o grupo pequeno do LiveCall.
- Usar DTLS-SRTP para criptografia do transporte de mídia.
- Usar STUN para descoberta de conectividade e adicionar TURN quando houver redes que bloqueiem conexões diretas.
- Limitar inicialmente a malha a seis peers por cliente; cada participante adicional aumenta a carga de upload/download.
- Manter captura de dispositivos fora da UI Iced, atrás da fachada `client/src/media`.

## Subfases

### 4A — Peer connections e negociação

- Integrar `webrtc-rs` no cliente.
- Criar uma `RTCPeerConnection` por participante remoto.
- Implementar Offer/Answer SDP e callbacks de ICE.
- Converter eventos ICE em `MediaSignal::IceCandidate` e enviá-los pelo WebSocket.
- Tratar renegociação, saída de peer e fechamento de conexões.

### 4B — Áudio

- Enumerar dispositivos com CPAL.
- Capturar PCM do microfone, adaptar taxa/canais e alimentar uma track WebRTC Opus.
- Receber áudio remoto e reproduzir no dispositivo de saída.
- Implementar mute real (parar/zerar a fonte), não apenas alterar o indicador visual.

### 4C — Webcam

- Capturar câmera com backend multiplataforma validado em Linux e Windows.
- Negociar track de vídeo e renderizar frames recebidos na área central da chamada.
- Tratar permissões, ausência de câmera e mudança de dispositivo.

### 4D — Compartilhamento de tela

- Capturar monitor ou janela, respeitando as diferenças de portal/Wayland/X11 no Linux e APIs do Windows.
- Publicar a captura como track de vídeo separada da webcam.
- Parar a captura ao interromper compartilhamento ou sair da sala.

## Critérios de aceite

- Dois clientes na mesma sala estabelecem conexão P2P e trocam SDP/ICE.
- Participantes fora da sala não recebem sinalização.
- O áudio funciona nos dois sentidos, com mute efetivo.
- Webcam e tela são tracks distintas.
- Fechar ou sair da chamada encerra tracks e peer connections.
- Testes incluem LAN, redes diferentes e cenário com TURN.

## Estado desta entrega

Esta entrega prepara a fronteira arquitetural (`MediaEngine`, `MediaConfig`, `MediaSignal` e `MediaState`) e documenta os incrementos. **Ainda não há transmissão real de áudio, vídeo ou tela nesta versão.** A próxima implementação funcional é a subfase 4A, seguida imediatamente pela 4B (áudio), antes de adicionar vídeo e compartilhamento.
