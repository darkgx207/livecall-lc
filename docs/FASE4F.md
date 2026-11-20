# Fase 4F — Navegação, volume e preparação de vídeo

## Alterações entregues

1. **Alterar servidor** no lobby: volta à tela de username/URL. Encerra a conexão e os peers da sessão anterior, limpa lista de participantes, IDs locais e volumes. O endereço já preenchido permanece editável. O servidor é reconectado somente ao usar Avançar.
2. **Volume individual de 0–100%**: botão no cartão remoto exibe um painel compacto com slider em passos de 1%; o ajuste é aplicado imediatamente no mixer CPAL local via `set_peer_volume`; não muda mute nem volume transmitido. Clicar novamente no botão fecha o painel; só um painel fica aberto. Ao sair da sala, os ajustes da sala são limpos.
3. **Nova paleta**: cabeçalho da sala grafite, fundo da chamada escuro, cartões dos participantes escuros, mantendo textos indicativos verde/vermelho suave e altura fixa de 310 px para a lista com scroll.
4. **Botão Assistir**: aparece em cartões remotos, porém sem ação (`on_press` ausente). O cliente atual **não transmite/recebe vídeo**; as flags de câmera e tela continuam apenas visuais. Logo, não existe vídeo disponível para visualizar nem fluxo de vídeo a suspender nesta versão.

## Arquitetura das próximas fases (não implementado nesta entrega)

O controle `Assistir` deverá permitir **assinatura por participante e fonte** (`camera` / `screen`), não apenas ocultar a janela. Ao fechar a visualização, cliente receptor solicitará suspensão do fluxo específico ao emissor por sinalização/controle; o emissor deixará de enviar pacotes de vídeo àquele peer ou negociará direção `inactive` conforme a implementação WebRTC. Reativação reiniciará fluxo e pedirá keyframe para decodificação rápida. **O áudio deve continuar independente**, inclusive com Ensurdecer e mute individuais. Isso exige track de vídeo real e coordenação entre emissor/receptor; **não está ativo na Fase 4F**.

## Testes sugeridos no Fedora

```bash
cargo check --workspace
cargo test --workspace
cargo run --bin livecall-client
```

- Conecte ao servidor A, no lobby selecione `Alterar servidor`, altere URL para servidor B e conecte novamente. Verifique que a sessão A foi encerrada.
- Com dois clientes numa sala, abra o controle de volume do cliente remoto. Teste 0%, 25%, 70%, 100%, com fala contínua, e verifique que o áudio local de outras pessoas não é alterado.
- Teste Ensurdecer e mute antes/depois de ajustar volume.
- Confirme que lista de participantes mantém **altura de 310 px** e controles principais continuam visíveis; o painel de volume expande **dentro da lista rolável** (não é popup flutuante sobreposto).
- O botão Assistir deve ficar inerte por ora.

## Limitações

Esta entrega foi inspecionada estaticamente, mas **não compilada nem testada** no ambiente de geração (sem cargo/rustc). O painel é uma expansão compacta dentro da lista, em vez de uma janela flutuante; uma futura integração de overlay pode refinar esse comportamento.
