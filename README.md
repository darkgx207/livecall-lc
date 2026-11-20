# LiveCall — Fase 4B (áudio P2P Opus)

Cliente desktop Rust/Iced + backend Rust/Axum. Esta entrega adiciona código de captura CPAL, codificação Opus, track WebRTC, decodificação e saída CPAL. **Código ainda não compilado/validado em máquinas reais nesta entrega**. O backend retransmite apenas SDP/ICE; não recebe áudio.

## Como testar

```bash
cargo check --workspace
cargo run -p livecall-backend
# Em outros dois terminais / máquinas:
cargo run -p livecall-client
```

Faça dois clientes entrarem na mesma sala. Permita acesso ao microfone e use fones de ouvido para evitar realimentação. Verifique no console `[WebRTC] peer ... Connected` e fale nos dois sentidos. Clique em **Microfone: mudo** no cliente A e confirme que B deixa de ouvir A; reative e confirme retorno. Sair e reentrar na sala deve renegociar peers. Os controles de **câmera e tela permanecem somente visuais**.

Requisitos do sistema: dispositivo de entrada e de saída acessível e permissão de microfone; no Linux, CPAL pode exigir bibliotecas de desenvolvimento ALSA (por exemplo, `libasound2-dev` em sistemas Debian/Ubuntu). O crate `opus` compila a biblioteca nativa Opus conforme sua cadeia de build; confira as dependências do sistema se houver falhas de link. Em redes distintas, configuração TURN pode ser necessária (veja `docs/FASE4A.md`).

Leia `docs/FASE4B.md` para implementação, limitações e roteiro detalhado de validação.

## Fase 4C — Controle de recepção

Consulte `docs/FASE4C.md` para o controle local de áudio recebido (ensurdecer) e o limite da fila Opus. Câmera e tela permanecem visuais.

## Fase 4E — Volume individual de participantes

Na tela de chamada, use o botão **Volume** ao lado de cada participante remoto para alternar 100% → 50% → 0% → 100%. Isso modifica somente a reprodução local. Consulte `docs/FASE4E.md`.
