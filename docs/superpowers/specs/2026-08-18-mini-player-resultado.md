# Mini player: resultado da execução

Data: 2026-08-19
Spec: `docs/superpowers/specs/2026-08-18-mini-player-design.md`
Plano: `docs/superpowers/plans/2026-08-18-mini-player.md`
Branch: `feat/mini-player` — 50 commits, 27 arquivos, +4.333 / −452

Este documento existe porque a execução tomou 47 decisões que a spec não previa. Elas foram
tomadas para não travar o trabalho, e ficam registradas aqui para poderem ser revistas.

## O que foi entregue

Um mini player de faixa única com interface nativa, sobre o núcleo Rust que toma o DAC em
modo exclusivo. Play, pause, controle de volume, metadados com capa, tempo decorrido e uma
linha técnica que mostra taxa, bit depth, codec e se o device está sequestrado.

| Verificação | Estado final |
|---|---|
| Testes Rust | 77 |
| Testes Rust de hardware | 10 |
| Testes Swift | 32 |
| `make verify` | BIT-PERFECT (C++ e Rust contra o oráculo ffmpeg, dumps idênticos) |
| `make verify-pause` | fluxo idêntico com e sem pausa |
| `make verify-volume` | amostras idênticas a 20% e 90% |
| Verificação humana | o usuário confirmou janela, play, pause e volume funcionando |

## Os três defeitos mais caros, e como apareceram

**1. Posse duplicada do device no replay.** Terminada a faixa, apertar play criava um
`HoggedDevice` novo com o antigo ainda vivo. O antigo só era descartado ao ser sobrescrito,
depois de o IOProc novo já ter iniciado — e seu `Drop` devolvia o rate, desfazia o formato e
liberava o hog por baixo de uma sessão que se julgava exclusiva. Toda chamada retornava
`noErr`. Encontrado por revisão especializada; corrigido e travado por teste de hardware que
lê o dono do hog e o rate direto do device.

**2. O `load` fotografava o device ainda sequestrado.** A consulta ao "default output" rodava
antes da liberação, e o `acquire` confiava nesse valor cacheado. Duas consequências, ambas
silenciosas: a restauração usava como "original" o rate que nós mesmos tínhamos colocado, e
uma segunda faixa de mesmo rate pulava a troca e a espera com o hardware em outro valor —
resample silencioso. **Origem: uma decisão minha durante a execução**, que consertou um bug
real e criou este.

**3. A segunda faixa saía pelo hardware errado.** Descoberto durante a própria onda de
correção: com o fone sequestrado (id 84), a consulta ao default devolve os alto-falantes
embutidos (id 72). Abrir uma segunda faixa tocando fazia a nova ser adquirida no device
errado. Medido e provado por mutação.

Os três só apareceram no review final de branch inteira, quando alguém teve as camadas à
vista de uma vez. Nenhuma revisão de task podia tê-los encontrado — cada uma via um pedaço.

## Testes que não conseguiam reprovar

Doze, ao longo do plano. As formas foram diferentes e vale registrá-las, porque são
reutilizáveis:

- **Valor de fixture que mascara o defeito.** `bytes_per_sample = 1` fazia `ch * sample` ser
  idêntico a `ch`; `sampleRate = 96000` dividido por 1000 dá inteiro e escondia a truncagem
  que exibia "44 kHz" para 44,1 kHz.
- **Simetria.** Escrita e leitura cometendo o mesmo erro, e o dado "batendo".
- **Cenário que não discrimina.** Asserção de contagem num caso em que pedido e lido são
  iguais.
- **Timing estrutural.** Asserção rodando antes de o código sob teste ter chance de executar.
- **Guard silencioso.** Fixture ausente fazendo o teste retornar cedo e contar como verde —
  uma árvore sem fixtures reportava 28/28 em 0,003 s.
- **Teste decorativo.** O `init` já calculava o resultado; o método sob teste podia ser
  apagado inteiro e o teste passava.

A disciplina que funcionou: **toda correção acompanhada de mutação de controle**, quebrando o
código de propósito para ver o teste ficar vermelho antes de aceitá-lo.

## As 47 decisões, por tema

**Sobre o que os gates provam (8, 19, 42 e o review final).** Descobriu-se que `make verify`
não executa o corpo do IOProc — o modo dump lê direto do ring buffer. Isso vale para as três
provas. Decisão: fechar a lacuna com 8 testes diretos do callback, e **corrigir o README para
não superestimar** o que cada prova cobre. Custo se errado: ganho aplicado dentro do IOProc
ainda passaria nos três gates.

**Sobre testes que não reprovam (7, 9, 10, 19, 23, 27, 31, 43, 45).** Regra adotada: cobertura
falsa é pior que nenhuma. Testes que não discriminam foram removidos ou reescritos; onde a
observação por software era impossível, registrou-se como limitação em vez de fingir cobertura.
Recusou-se teste dependente de relógio em favor de sincronização determinística.

**Sobre defeitos do próprio plano (11, 12, 13, 15, 20, 22, 24, 35, 36, 39, 41).** Onze vezes o
brief continha erro meu — referência antecipada de tipo, código que não compilava,
identificadores em português contra a regra do projeto, contradição entre preservar mensagens
da CLI e entregar um relatório estreito. Corrigidos na origem, com os briefs regenerados.

**Sobre proporcionalidade (3, 4, 34, 45).** Não foram introduzidos: enforcement de `fmt`,
limpeza dos 27 avisos pré-existentes, identidade de origem para mensagens de erro, e
substituição de uma margem de tempo por semáforo num teste estruturalmente determinístico.
Custo se errado: dívida pré-existente permanece.

**Sobre concorrência e tempo real (2, 16, 17, 18, 29, 32, 44).** `poll_finished` usa `try_lock`
para o `snapshot()` nunca bloquear; produtora órfã fechada; `deinit` isolado invalidando o
timer; comandos de volume encadeados depois que tirá-los da thread principal removeu a
serialização que a main actor dava de graça.

**Sobre seleção de agentes (5).** A pedido do usuário, revisão por especialistas do ecc em vez
de generalistas. Foi o que encontrou os três defeitos críticos.

## Limitações conhecidas

- Tirar o fone durante a reprodução não é tratado: o device some, o tempo congela, e a saída é
  fechar o player.
- Encerramento forçado (`kill -9`) deixa o sample rate trocado. O sistema solta o hog; não
  desfaz a configuração. Os quatro sinais tratáveis agora passam pelo `shutdown`.
- Terminada a faixa, o device continua retido até carregar outra ou fechar o app. É decisão de
  desenho (resume instantâneo), e a linha técnica mostra "hog ativo" também nesse estado.
- O teste do device errado depende da topologia desta máquina (fone + alto-falantes). Numa
  máquina com uma saída só, passaria mesmo com o bug reintroduzido.
- `PlayerState::Failed` existe, é testado e nunca é atribuído: erro de decodificação no meio do
  arquivo é hoje indistinguível de fim de faixa. Registrado, não corrigido.
- Os alvos `info` e `play` do Makefile ainda usam o binário C++.
