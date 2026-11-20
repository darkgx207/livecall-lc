# LiveCall — Fase 4A: Peer connections WebRTC

## Implementado nesta entrega

- Dependência `webrtc-rs` (`webrtc = 0.11`) no cliente.
- Criação de uma `RTCPeerConnection` por participante remoto.
- Criação de um DataChannel `livecall-control` para que a negociação tenha transporte SCTP verificável mesmo antes de áudio/vídeo.
- Negociação automática Offer/Answer SDP.
- Coleta de ICE candidates (trickle ICE) e encaminhamento pelo WebSocket já existente.
- Aplicação de ICE candidates recebidos.
- Fila de ICE candidates recebidos antes da descrição remota, evitando a corrida comum entre ICE e SDP.
- Regra determinística por UUID: somente o participante cujo UUID é lexicograficamente menor inicia a oferta para aquele par.
- Fechamento de PeerConnections ao sair da sala, quando o peer sai ou quando o WebSocket cai.
- Log do estado de cada PeerConnection no terminal (`[WebRTC] peer ...`).
- STUN configurável e suporte opcional a TURN via variáveis de ambiente.
- O servidor envia a resposta de registro/entrada com o UUID antes de `room_state`, para o cliente conhecer sua identidade antes de iniciar offers.

## O que deve funcionar

Com dois clientes na mesma sala, os clientes devem negociar SDP e ICE automaticamente. Se a conectividade de rede permitir, o estado da PeerConnection deve avançar para `Connected`; isso aparece no terminal onde o cliente foi iniciado.

A entrega usa DataChannel somente como transporte de teste. Não há tracks de áudio ou vídeo nesta subfase.

## O que ainda não funciona

- Captura e transmissão de microfone.
- Reprodução de áudio remoto.
- Webcam e renderização de vídeo.
- Captura e compartilhamento de tela.
- TURN não é fornecido pelo projeto: é necessário configurar um servidor TURN próprio para redes que não aceitam caminho direto.
- Ainda não há interface visual para o estado ICE/PeerConnection; consulte o terminal.

## Configuração

O padrão usa STUN público:

```env
LIVECALL_STUN_URLS=stun:stun.l.google.com:19302
```

Múltiplos STUN podem ser separados por vírgula. TURN é opcional:

```env
LIVECALL_TURN_URL=turn:SEU_SERVIDOR:3478
LIVECALL_TURN_USERNAME=usuario
LIVECALL_TURN_PASSWORD=senha
```

As variáveis devem estar no ambiente do processo do cliente. O `.env.example` é apenas referência; o binário não carrega automaticamente arquivos `.env`.

## Teste manual

1. Execute o backend: `cargo run -p livecall-backend`.
2. Abra dois clientes em terminais separados: `cargo run -p livecall-client`.
3. Registre dois usernames e coloque ambos na mesma sala.
4. Aguarde alguns segundos para coleta ICE e negociação DTLS/SCTP.
5. Observe os terminais. Em ambos deve aparecer `[WebRTC] peer <uuid>: Connected` se a conectividade P2P foi estabelecida.
6. Feche um cliente e confirme que o outro recebe a saída e encerra a PeerConnection.
7. Repita em redes diferentes. Se ficar em `Checking` ou terminar em `Failed`, configure TURN e repita.

## Limitações de teste local

Dois processos na mesma máquina validam a negociação e a sinalização, mas não representam NATs e firewalls de redes diferentes. Para validar conectividade real, teste também com dois computadores em redes distintas.
