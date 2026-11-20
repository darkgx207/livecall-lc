# Fase 4D — Ensurdecer (recepção + microfone)

A opção **Ensurdecer** substitui o antigo controle que silenciava apenas a recepção.

- Ao ativar, a reprodução dos participantes remotos é silenciada, e o microfone local envia silêncio. O estado transmitido ao servidor indica microfone desativado.
- Durante o modo ensurdecido, o botão do microfone não pode ser acionado separadamente.
- Ao desativar, a recepção volta, e o microfone retorna ao estado que tinha antes de ensurdecer (ligado ou mutado).
- Ao sair da sala ou perder a conexão, o estado de microfone anterior é restaurado na interface. A próxima sala não herda o ensurdecimento.
- A lógica é local ao cliente e não exige alteração no protocolo do backend.

## Validação manual

1. Entre com dois clientes na mesma sala; confirme a transmissão bidirecional.
2. Com microfone ligado, ative **Ensurdecer**: não deve ouvir nem ser ouvido; o outro cliente deve ver o microfone desativado.
3. Desative **Ensurdecer**: recepção e microfone devem voltar.
4. Mute o microfone manualmente, ative e desative **Ensurdecer**: a recepção deve voltar, mas o microfone deve permanecer mutado.
5. Ative **Ensurdecer**, saia da sala e entre novamente: não deve permanecer ensurdecido.

## Estado da verificação

Alteração de código e pacote gerados; não foi possível rodar `cargo check` neste ambiente (Rust/Cargo ausentes). Os testes de funcionamento precisam ser executados na máquina de desenvolvimento.
