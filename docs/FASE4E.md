# Fase 4E — Controle local de volume por participante

## Implementado

- Cada cartão de participante remoto possui botão de volume com ciclo **100% → 50% → 0% → 100%**.
- O volume é aplicado à amostra decodificada de cada participante **antes** da soma no mixer CPAL. Não muda o volume de transmissão, o microfone de ninguém nem mensagens enviadas ao backend.
- 0% silencia apenas o participante escolhido. Outros participantes permanecem audíveis.
- O controle de volume é independente de **Ensurdecer**: ensurdecer continua cortando toda a recepção e mutando o próprio microfone; ao desativá-lo, os volumes individuais persistem.
- Ao sair da sala, perder conexão ou quando um participante sai, seus ajustes locais são descartados; em uma nova entrada, o volume volta a 100%.
- Cartões preservam a altura fixa de 54 px; lista preserva altura de 310 px e scroll.
- Testes unitários adicionados para ciclo de volume e mixer individual.

## Validação manual

1. `cargo check --workspace` e `cargo test --workspace`.
2. Inicie três clientes na mesma sala. Em um cliente, ajuste um participante para 50% e outro para 100% e compare os níveis.
3. Ajuste o primeiro para 0%: apenas ele deverá ficar inaudível. Ajuste novamente para 100%.
4. Ative **Ensurdecer**: todos devem ficar inaudíveis, e o seu microfone será mutado. Desative e verifique que os ajustes individuais permanecem.
5. Saia da sala e volte: os volumes individuais devem retornar a 100%.

## Limitações

- O controle é propositalmente de três níveis, sem slider, para preservar a interface compacta.
- Não são transmitidos ajustes de volume pela rede; são apenas locais.
- Câmera e compartilhamento de tela permanecem visuais e não são implementados aqui.
- Não foi possível executar `cargo check` ou `cargo test` neste ambiente, sem Rust/Cargo e dispositivos de áudio.
