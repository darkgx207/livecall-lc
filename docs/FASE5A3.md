# Fase 5A.3 — Layout lateral e seletores independentes

- Sala e controles no painel lateral fixo de 290 px.
- Região de participantes com 310 px e rolagem. Os cartões remotos mostram ações VP8 e volume.
- Câmera e microfone lado a lado na região principal, cada qual com sua própria seleção e atualização.
- Área principal reservada para renderização futura de transmissão (não há player na fase 5A.3).
- Removidos emojis dos textos de interface por incompatibilidade de fonte em alguns ambientes Linux/Iced.
- Captura e seleção de vídeo, áudio Opus e sinalização mantidos da 5A.2.

## Validação

```bash
cargo check --workspace
cargo test --workspace
cargo run --bin livecall-client
```

Verifique a seleção de câmera sem troca de microfone, o mute, Ensurdecer, scroll da lista, áudio de participantes e fluxo VP8 quando houver câmera.

### Limitações

A renderização VP8 continua pendente. Sem compilador Rust e hardware de captura neste ambiente, o código não foi executado aqui.
