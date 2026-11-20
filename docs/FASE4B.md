# Fase 4B — Implementação inicial de áudio bidirecional

## Implementado no código

- Captura do dispositivo CPAL de entrada padrão nos formatos `f32`, `i16` ou `u16`.
- Conversão para mono e taxa de 48 kHz; bufferiza blocos de 20 ms (960 amostras).
- Codificação de cada bloco para Opus VoIP e envio em `TrackLocalStaticSample` compartilhada pelas conexões WebRTC P2P.
- Registro de `on_track`, leitura RTP, decodificação Opus por usuário remoto e mixer de áudio para dispositivo de saída CPAL (`f32`, `i16` ou `u16`).
- Volume misturado é limitado a [-1,+1], filas por peer e drenagem de buffers quando peer sai.
- Botão de mute controla captura transmitida: o codificador continua enviando silêncio, evitando renegociação.
- Falha de inicialização de dispositivo não impede login/sinalização (a UI exibe erro de áudio).
- Configuração STUN/TURN do `MediaConfig` agora é aplicada à PeerConnection (antes era recriada via Default em cada peer).
- Indicadores textuais verde/vermelho suave de dispositivos e estado de mute; preservação do scroll fixo de participantes.

## Limitações e pontos a validar

- **Sem compilação ou teste de escuta neste ambiente:** Rust/Cargo não estão instalados; o pacote é uma implementação para validação local, não um release testado.
- Streams CPAL são inicializados quando a conexão ao servidor é criada e permanecem até o engine ser descartado; ao sair da chamada, tracks P2P são fechadas e o microfone passa a transmitir silêncio, mas os dispositivos CPAL ainda ficam abertos durante o lobby.
- Mudanças de dispositivo durante chamada não são reconectadas automaticamente; reiniciar o cliente é a recuperação recomendada.
- Erros assíncronos do dispositivo são registrados no terminal, sem atualizar continuamente o rodapé.
- A reamostragem de taxa usada inicialmente é simples (nearest-neighbor na captura e linear na saída): pode introduzir artefatos/latência em dispositivos que não suportam 48 kHz nativos. Não há supressão de eco ou de ruído.
- O uso de buffers/locks no callback CPAL ainda requer perfilamento sob carga e redes com perdas. Jitter buffer/RTP recovery não está implementado além da fila limitada da reprodução.
- A presença de câmera ou compartilhamento de tela é ainda apenas estado de UI/sinalização. Nenhum vídeo é transmitido.

## Testes de aceite manuais

1. Execute `cargo check --workspace` e corrija eventuais problemas dependentes do sistema/versões do Rust.
2. Inicie servidor e duas instâncias de cliente em dispositivos com entrada e saída de áudio acessíveis.
3. Entre na mesma sala, confirme estado WebRTC `Connected` em ambos; fale em A e confirme áudio em B; inverta.
4. Mute A: B não deve mais ouvir a voz de A. Unmute A: voz deve retornar sem criar outra offer/answer.
5. Saia e entre de novo; o peer P2P antigo deve ser removido, e o novo conectar.
6. Simule ausência de microfone/saída e confirme que o cliente ainda consegue conectar ao servidor, mas informa falha de áudio.
7. Teste entrada/saída de 44,1 kHz e 48 kHz, formatos float/int e Linux/Windows. Use fones para evitar feedback.
8. Confira conexões em redes diferentes e TURN quando necessário.

## Próximos endurecimentos antes de considerar 4B validada

- Corrigir possíveis erros de compilação constatados em `cargo check` (não disponível neste ambiente).
- Desativação real dos streams CPAL ao sair da sala e reinicialização sem reconectar ao servidor.
- Eventos de erro de dispositivo na UI; reconexão; buffer de jitter; melhor resampling; cancelamento de eco.
