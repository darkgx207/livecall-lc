# Fase 5A.2 — Seleção independente de microfone e câmera

## Implementação

- Seleção de dispositivos de áudio **de entrada** pela lista CPAL. Câmeras continuam descobertas via V4L2.
- 🎙️ Menu de microfones na tela da chamada, independente do menu 📷 de câmeras. 🔄 Atualizar microfones permite redescobrir interfaces externas ou virtuais conectadas durante a execução.
- Troca do microfone na chamada, **sem recriar a track Opus/WebRTC nem interromper o mixer de saída**. A captura nova é iniciada antes de substituir a antiga; se falhar, a anterior permanece ativa. O mute/Ensurdecer e volumes individuais continuam preservados.
- Preferência de entrada em `~/.config/livecall/microphone.txt` pelo nome do dispositivo. Ao conectar novamente, a escolha é usada se encontrada; caso contrário, o cliente usa a entrada padrão do sistema.
- Emojis adicionados aos controles principais como ícones de texto, sem novas dependências nem alterações no tamanho fixo (310 px) da lista de participantes.

## Testes no Fedora

```
cargo check --workspace
cargo test --workspace
cargo run --bin livecall-client
```

1. Conecte dois dispositivos de captura, por exemplo headset USB e microfone da webcam. Abra uma chamada e use 🎙️ Microfone para escolher o headset. Confirme que o outro participante recebe **som do dispositivo selecionado**.
2. Alterne para o outro dispositivo durante a chamada. Confirme que a conexão WebRTC e o som recebido continuam ativos.
3. Mute e Ensurdecer antes/depois da troca. O estado não deve ser alterado pela seleção.
4. Com a câmera indisponível, selecione o microfone normalmente: câmera e microfone não dependem um do outro.
5. Selecione um dispositivo, feche o app e reconecte. A preferência deve ser reaplicada quando o mesmo nome estiver presente.
6. Remova um dispositivo e clique Atualizar microfones: a lista deve mudar, e uma tentativa de selecionar um dispositivo inexistente deve exibir erro sem desligar o stream anterior.

## Limitações

- A lista CPAL pode expor fontes ALSA, PipeWire, PulseAudio ou virtuais como endpoints separados; duplicatas com o mesmo nome são desambiguadas pelo índice somente durante a sessão.
- A preferência salva é baseada no **nome** do dispositivo. Se existirem dois dispositivos com o mesmo nome, na próxima conexão será selecionada a primeira correspondência. A persistência por ID estável requer suporte específico de cada backend/sistema.
- Não há troca automática ao desconectar a fonte ativa; o usuário deve atualizar a lista e escolher outra. Erros assíncronos do driver continuam registrados no terminal.
- Seleção de microfone está disponível durante a chamada; seleção de câmera exige que a câmera esteja desligada. A saída de áudio continua no padrão do sistema.
- Esta entrega não implementa o render de vídeo (planejado para 5B) nem a detecção de câmera no Windows.
- Compilação e teste real em hardware ainda são necessários.
