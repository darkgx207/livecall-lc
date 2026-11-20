# Fase 5A.1 — Seleção e diagnóstico de câmeras (Linux/Fedora)

## Implementado

- Ao entrar numa sala, procura câmeras disponíveis na interface V4L2 do Linux (`VIDIOC_QUERYCAP`). Exibe apenas nós que relatam capacidade de **captura de vídeo**, ignorando dispositivos somente de metadados.
- A lista usa o nome fornecido pelo dispositivo e o caminho (`/dev/videoN`), permitindo selecionar webcams físicas, externas e virtuais (incluindo OBS quando ele publica um dispositivo V4L2 de captura).
- Sem câmera: mensagem clara e botão de câmera desativado. O áudio permanece independente.
- Botão **Atualizar câmeras** para detectar dispositivos conectados durante a chamada, sem reiniciar.
- Com uma câmera, seleciona automaticamente; com várias, oferece seletor. A preferência escolhida é gravada em `$XDG_CONFIG_HOME/livecall/camera-device.txt` ou `~/.config/livecall/camera-device.txt`. A preferência usa barramento + nome fornecidos pelo V4L2, evitando depender apenas do índice `/dev/videoN`.
- A câmera selecionada é passada explicitamente ao FFmpeg. A mensagem de falha agora classifica erros frequentes: permissão, dispositivo ocupado, desconectado, encoder ausente e formato incompatível; detalhes adicionais continuam registrados no terminal.
- Não é permitido trocar ou atualizar a câmera enquanto ela estiver ligada: desligue antes. Layout da lista de participantes permanece fixo em 310 px. Estado de áudio e transmissão VP8 sob demanda da fase 5A foi preservado.

## Como testar no Fedora

```sh
cargo check --workspace
cargo test --workspace
cargo run --bin livecall-client
```

1. Entre na sala sem webcam: deve aparecer **Nenhuma câmera disponível**; **Câmera: desligada** fica desabilitado; o áudio deve continuar operacional.
2. Conecte uma webcam ou inicie uma câmera V4L2 do OBS e clique **Atualizar câmeras**; confira se o dispositivo aparece pelo nome.
3. Se houver mais de uma câmera, selecione outra. Saia e volte à sala: deve tentar restaurar a preferência, se a identificação do dispositivo continuar estável.
4. Ligue a câmera: o FFmpeg deve usar o caminho selecionado. Se falhar, verifique a mensagem de erro; nenhuma câmera deve ser marcada como ligada nesse caso.
5. Desligue a câmera e tente trocar o dispositivo. A captura antiga precisa parar antes de iniciar a nova.
6. Verifique que a lista de participantes continua com altura fixa, com botões de áudio, volume e Ensurdecer acessíveis.

## Restrições conhecidas

- **Detecção e seleção automáticas estão implementadas somente para Linux V4L2**. O tratamento de DirectShow/Media Foundation no Windows é uma próxima etapa. Não se deve interpretar o status da versão Windows como suporte funcional de seleção.
- A descoberta exige acesso de leitura aos nós `/dev/videoN`. Dispositivos não acessíveis são omitidos; o texto de falha após seleção depende do stderr do FFmpeg.
- A entrada de captura mantém a configuração **640x480, 15fps** da 5A. Certas webcams exigem outros formatos/resoluções, que serão configuráveis em fase futura.
- A transmissão VP8 da 5A ainda não tem decodificação/renderização; o botão de recebimento apenas pede pacotes de vídeo, que são contados no log. A renderização é a Fase 5B.
- O FFmpeg precisa estar instalado com libvpx. Não altera configurações do PipeWire/PulseAudio.
- Compilação e captura com dispositivos físicos dependem de validação local. Não foram executadas neste ambiente.
