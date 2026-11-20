# Fase 5B — reprodução opcional da câmera VP8

## Entrega experimental (não compilada neste ambiente)

A interface central da Fase 5A.3 passa a oferecer a visualização de vídeo remoto solicitado individualmente. O transporte WebRTC e a faixa Opus existente são preservados. O processamento de vídeo fica separado do áudio.

### Caminho de vídeo

1. O transmissor captura a câmera V4L2 usando FFmpeg com VP8 640x480 a 15 FPS, conforme a Fase 5A.
2. O usuário receptor pressiona **Assistir** no cartão do transmissor; o cliente envia sinal `video_subscribe: true`.
3. O transmissor habilita o envio na faixa VP8 daquele receptor, sem afetar os demais.
4. No receptor, os pacotes RTP são remontados em quadros VP8 (RFC 7741). Pacotes faltantes descartam quadros incompletos.
5. Cada track remota tem um processo FFmpeg que recebe IVF/VP8 e produz RGBA 640x480. Os últimos pixels ficam em memória, atualizados no Iced a cada tick de 100 ms.
6. **Fechar** ou **Parar de assistir** envia `video_subscribe: false` e remove o quadro local; áudio e volumes individuais são preservados.

### Pré-requisitos

- `ffmpeg` instalado no **transmissor e no receptor**, com codificação VP8 (`libvpx`) no transmissor e decodificação VP8 no receptor.
- Webcam física/virtual V4L2 no transmissor. **O receptor não precisa de webcam.**
- Dependências de compilação das fases anteriores; a funcionalidade de imagem do Iced está habilitada em `client/Cargo.toml`.

### Validação

```bash
cargo check --workspace
cargo test --workspace
cargo run --bin livecall-client
```

Com dois computadores ou clientes: ligue a câmera no A, entre na mesma sala com B, clique **Assistir** para A em B. A imagem deverá aparecer na área central; feche-a e verifique que o áudio permanece ativo. Teste ainda câmera desligada enquanto aberta, entrada/saída de participante e 2 ou 3 fluxos simultâneos.

### Restrições e pontos pendentes

- **Nenhum teste de compilação ou transmissão WebRTC real foi realizado neste ambiente.** O FFmpeg foi testado isoladamente com um arquivo VP8 sintético, não com RTP ao vivo.
- 640x480 e 15 FPS são fixos. Alterações dinâmicas de resolução ou formatos não são aceitas pelo decodificador atual.
- O decodificador FFmpeg é criado ao surgir uma faixa VP8, mesmo antes de o usuário solicitar a imagem; porém, fica sem quadros enquanto não houver assinatura. O encerramento integral do decoder quando desinscrito e a reinicialização sob demanda poderão ser aperfeiçoados depois.
- Fluxos com perda de RTP podem exigir novo keyframe. Não há solicitação explícita de PLI nesta etapa; o transmissor produz keyframes periódicos (~2 s).
- Não há tela compartilhada nem controles de qualidade/bitrate adaptativos nesta etapa.
- A renderização em Iced com pixels RGBA gera uploads frequentes; medir CPU/GPU e suavidade antes de considerar a fase validada.
