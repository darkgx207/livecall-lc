# Fase 5A — Câmera real (captura VP8 e transporte opcional)

## Escopo realmente implementado

- Botão **Câmera** tenta iniciar captura física via **FFmpeg** (processo externo) usando `v4l2` no Linux ou `dshow` no Windows. Só marca a câmera como ligada depois de receber um cabeçalho válido IVF/VP8.
- Saída codificada em VP8, resolução solicitada 640×480 e 15 fps, bitrate alvo 450 kb/s; os frames são extraídos do contêiner IVF e encaminhados a uma track WebRTC VP8 **individual por peer**. O áudio Opus continua independente.
- Por padrão, a track VP8 **não envia frames** a outros peers. O usuário remoto clica em **Receber VP8 (sem imagem)** para autorizar o envio da track daquele participante. **Parar VP8** interrompe seu envio a esse peer, sem mutar áudio.
- No receptor, os pacotes RTP VP8 são consumidos para diagnóstico, com contagem no stderr a cada 100 pacotes. **Não existe decodificação nem visualização de vídeo nesta fase**; isso pertence à Fase 5B.
- Ao sair da sala/desconectar, a captura é interrompida, os workers e peers são encerrados. Mudanças de câmera são comunicadas ao servidor apenas após o início/parada efetivos.
- O botão de compartilhar tela permanece **apenas visual**.

## Dependência adicional (programa externo, não pacote Rust)

FFmpeg acessível no PATH, incluindo encoder `libvpx`, e webcam com permissões para o usuário. Verifique:

```sh
ffmpeg -version
ffmpeg -hide_banner -encoders | grep libvpx
ls /dev/video*  # Linux
```

O comando da câmera usa `/dev/video0` por padrão no Linux. Personalize antes de iniciar o cliente, por exemplo:

```sh
export LIVECALL_CAMERA_DEVICE=/dev/video2
cargo run --bin livecall-client
```

No Windows, o padrão é `video=Integrated Camera`; identifique o nome correto no FFmpeg com `ffmpeg -list_devices true -f dshow -i dummy` e configure `LIVECALL_CAMERA_DEVICE="video=NOME DA CAMERA"`.

A câmera precisa suportar a configuração de entrada pedida (640×480 a 15fps). Em webcams com outros formatos, o FFmpeg poderá falhar; a fase futura pode incluir seleção/sondagem dinâmica de formatos.

## Validação

```sh
cargo check --workspace
cargo test --workspace
cargo run --bin livecall-client
```

1. Inicie backend e dois clientes; entre na mesma sala; confirme que o áudio continua funcional.
2. No cliente A, clique em `Câmera: desligada`. Verifique que o botão muda para `Câmera: ligada (VP8)` e o participante B recebe o status.
3. Em B, clique `Receber VP8 (sem imagem)` em A. Aguarde a contagem de pacotes no terminal de B (`[Vídeo] ... pacotes VP8 recebidos`). Nenhuma imagem será renderizada ainda.
4. Em B, clique `Parar VP8`: a contagem deixa de aumentar. Faça/receba áudio nos dois sentidos para confirmar independência.
5. Desligue a câmera em A: a captura deve parar; o status de B deve refletir câmera desligada. Saia da sala e confira que a câmera é liberada.
6. Teste câmera ausente/sem permissão e FFmpeg inexistente: o botão deve permanecer desligado com erro no rodapé.

## Limitações

- Implementação escrita e revisão estática; o ambiente de geração não possui Cargo/rustc nem webcam física para validação.
- Não há visualização/renderização, seleção de câmera pela interface ou suspensão da **captura** quando ninguém assiste: apenas envio WebRTC é suspenso por peer; a câmera local permanece ativa enquanto o botão estiver ligado.
- Ao iniciar a captura e obter cabeçalho IVF válido não há garantia de que a webcam continuará funcionando. Se FFmpeg terminar de forma inesperada, o log identifica o erro; o estado visual poderá precisar ser atualizado manualmente.
- O FFmpeg é um processo externo obrigatório para a webcam nesta implementação. Suporte dshow do Windows é preliminar e não foi testado.
- A lógica existente de permissões de servidor e senhas de sala não foi alterada.
