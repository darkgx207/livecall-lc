# Fase 4C — Controle local de reprodução e estabilidade do áudio

Base: pacote da Fase 4B com as correções de compilação validadas pelo usuário.

## Implementado

- Controle **Recepção: ligada/silenciada** na tela de chamada. Atua no mixer CPAL local e não altera o estado do microfone enviado para a sala.
- A saída continua consumindo amostras enquanto silenciada, evitando que a retomada reproduza áudio antigo.
- Ao silenciar, as filas de reprodução dos participantes são limpas.
- Ao sair da chamada, o estado de silenciamento da recepção é reiniciado.
- O canal de quadros Opus produzidos pelo encoder passa a ter capacidade limitada a 12 quadros (~240 ms). Sob atraso no consumo, novos quadros são descartados em vez de acumular memória indefinidamente.
- A lista de participantes conserva seus 310 px, rolagem e botões originais.

## Limitações

- A fila de áudio limitada protege contra crescimento de memória, mas, em sobrecarga, podem ocorrer cortes de voz. Não é um jitter buffer adaptativo de rede.
- Câmera e compartilhamento de tela continuam apenas controles visuais. Ainda não transmitem imagem.
- O novo controle da recepção não é comunicado ao servidor porque é um estado estritamente local.
- Compilação e testes de dispositivo dependem do ambiente do usuário; este pacote não foi compilado neste ambiente.

## Validação sugerida

1. `cargo check --workspace`
2. `cargo run --bin livecall-backend` e `cargo run --bin livecall-client` em duas máquinas/instâncias, conforme configuração anterior.
3. Entrar na mesma sala e confirmar voz bidirecional.
4. Em A, clicar **Recepção: silenciada**. A deixa de ouvir B; B continua ouvindo A.
5. Em A, clicar **Recepção: ligada**. A volta a ouvir B sem reprodução de falas acumuladas.
6. Confirmar que o botão **Microfone** permanece independente e que sair/reentrar na chamada restaura a recepção.
