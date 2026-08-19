# Playlist: fila de faixas com navegação

Data: 2026-08-19

## Objetivo

Transformar o mini player de faixa única numa fila de faixas. Adicionar várias músicas de uma
vez — arrastando do Finder ou selecionando no painel —, navegar entre elas com Previous/Next,
e avançar sozinho quando a faixa termina.

A prioridade declarada continua sendo **funcionalidade correta e bem testada**, não interface
bonita. A lista é utilitária.

A garantia de bit-perfect **não pode regredir**. Nada nesta feature toca no caminho do áudio:
a fila decide *qual arquivo* carregar, nunca *como* os bytes chegam ao DAC.

## Contexto

O player de hoje carrega um arquivo por vez. `HogPlayer::load(path)` decodifica o cabeçalho e
negocia o formato; `play()` toma o device em hog mode e trava o rate. O fim da faixa é
detectado pelo IOProc, que só marca um átomo — quem traduz isso em estado é o
`poll_finished()`, chamado a cada `snapshot()`.

O drag & drop e o painel de abrir **já existem**, ambos limitados a um arquivo:
`ContentView.handleDrop` descarta tudo além de `providers.first`, e o `NSOpenPanel` roda com
`allowsMultipleSelection = false`. Esta feature os estende; não os cria.

### A restrição que o hardware impõe: não existe gapless

Trocar de faixa passa por `load()`, que chama `stop_and_release` — solta o hog, restaura o
sample rate e o formato originais do device — e depois `play()`, que readquire tudo. Se a
faixa seguinte tiver outro sample rate, o DAC **muda fisicamente de frequência**.

Cada troca custa centenas de milissegundos de silêncio. Isso é consequência direta do que o
projeto existe para fazer, e não é negociável sem reescrever o núcleo. Duas consequências
menores, ambas declaradas em vez de escondidas:

- Existe uma janela, entre soltar e readquirir, em que o Mac volta a ter som normal. Outro app
  pode emitir áudio nessa fração de segundo.
- Nessa mesma janela, outro processo pode tomar o device. A reaquisição então falha — e essa
  falha **não é culpa da faixa** (ver "Falha de faixa e falha de device", adiante).

### Decisões tomadas com o usuário

| Decisão | Escolha |
|---|---|
| Onde a fila mora | **Rust**, exposta por uniffi (abordagem B) |
| Quem dispara o auto-avanço | **Swift**, pelo poll que já existe; **Rust decide** qual é a próxima (variante B2) |
| Fim da faixa | Avança sozinho; ao terminar a última, para |
| Faixa que não carrega no auto-avanço | Pula para a próxima que tocar, com o erro visível; no Next/Previous manual, para e reporta |
| Operações na lista | Remover item e limpar tudo. **Sem reordenar** |
| Previous | Padrão de player: passou de 3 s, reinicia a faixa; antes disso, vai para a anterior |
| Ingestão | Pastas expandem recursivamente; ordem alfabética por caminho |
| Linha da lista | Nome do arquivo. Título, artista, álbum e capa continuam só para a faixa atual |
| Remover a faixa que está tocando | Tratado como Next |

### Fatos verificados antes de decidir

- **`Engine` já é `Send + Sync`.** `HogPlayer` é `uniffi::Object`, que exige as duas, e o crate
  compila hoje. Uma thread de vigília em Rust seria possível — foi descartada por custo, não
  por impedimento (ver "O gatilho do auto-avanço").
- **`snapshot()` roda na main actor.** O `Timer` do `PlayerViewModel` chama `refresh()` dez
  vezes por segundo dentro do `@MainActor`, e `snapshot()` chama `poll_finished()`. Qualquer
  trabalho bloqueante colocado nesse caminho congela a janela.
- **`main.rs` não passa por `api.rs`.** A CLI fala com o `Engine` direto, então mexer na
  superfície uniffi não a afeta.
- **`load()` já reusa o device retido.** Quando há hog ativo, a negociação da faixa nova corre
  contra o `OutputDevice` que já está na mão, nunca contra o "default output" — que sob hog
  aponta para outro hardware. A troca de faixa herda essa correção de graça.

## Arquitetura

### Onde a fila mora, e por quê

O Rust é dono da **lista de caminhos, da ordem, do índice atual, de quais faixas falharam e da
regra de sucessão**. O Swift é dono da **ingestão** (arrastar, painel, expandir pasta,
ordenar), dos **metadados de exibição** — só o AVFoundation lê tags — e do **desenho**.

A objeção natural a pôr a fila em Rust é testabilidade: navegação em Rust tenderia a exigir
arquivos de áudio reais. O projeto já resolveu esse problema uma vez, com `transitions.rs`, e a
solução se aplica igual aqui.

### `queue.rs` — as regras puras

Módulo novo, sem I/O, sem device, sem thread. Recebe índice, tamanho, decorrido e quais faixas
falharam; devolve a ação a executar. É onde mora toda a regra de navegação, e onde ela é
testada.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueAction {
    Load(usize),   // carregar a faixa deste índice
    Restart,       // recarregar a faixa atual do início
    Stop,          // acabou a lista
}

/// Limite do padrão de player: antes disso, Previous navega; depois, rebobina.
pub const RESTART_THRESHOLD_SECONDS: f64 = 3.0;

pub fn on_next(current: Option<usize>, len: usize) -> Option<QueueAction>;
pub fn on_previous(current: Option<usize>, elapsed: f64) -> Option<QueueAction>;
pub fn on_finished(current: Option<usize>, len: usize) -> Option<QueueAction>;

/// Primeiro índice a partir de `from` (inclusive) que ainda não falhou.
pub fn next_playable(from: usize, failed: &[bool], len: usize) -> Option<usize>;

/// Para onde o índice atual vai depois de remover `removed`.
pub fn after_removal(current: Option<usize>, removed: usize, len_after: usize)
    -> (Option<usize>, bool);  // (novo índice, precisa recarregar)
```

`after_removal` devolve um par porque as duas informações são independentes: remover uma faixa
acima da atual só desloca o índice (sem recarregar nada), enquanto remover a atual desloca *e*
exige carregar quem assumiu a posição.

### Superfície uniffi

```rust
#[derive(uniffi::Record)]
pub struct QueueItem {
    pub path: String,
    /// Motivo, quando a faixa foi marcada como não-tocável. `None` = nunca falhou.
    pub failure: Option<String>,
}

#[uniffi::export]
impl HogPlayer {
    pub fn append_tracks(&self, paths: Vec<String>) -> u32;   // devolve quantas entraram
    pub fn remove_track(&self, index: u32) -> Result<(), PlayerError>;
    pub fn clear_queue(&self) -> Result<(), PlayerError>;
    pub fn queue_items(&self) -> Vec<QueueItem>;
    pub fn select_track(&self, index: u32) -> Result<TrackFormat, PlayerError>;
    pub fn next_track(&self) -> Result<TrackFormat, PlayerError>;
    pub fn previous_track(&self) -> Result<TrackFormat, PlayerError>;
    /// Auto-avanço. `None` = a lista acabou. Pula sozinho as faixas que falharem.
    pub fn advance(&self) -> Result<Option<TrackFormat>, PlayerError>;
}
```

**`load(path)` sai da fronteira uniffi.** Com a fila em Rust, o Swift enfileira e seleciona por
índice; carregar por caminho criaria um segundo dono do "o que está carregado", que é
exatamente o que esta arquitetura existe para evitar. `Engine::load` continua existindo — é o
que a CLI usa e o que `select_track` chama por baixo.

`play()`, `pause()`, `set_volume()`, `snapshot()` e `shutdown()` não mudam.

### O custo do poll, e o `queue_version`

`queue_items()` com 500 faixas atravessando o FFI dez vezes por segundo é desperdício puro. A
lista, portanto, **não viaja no poll**. O `Snapshot` barato ganha três campos escalares:

```rust
pub struct Snapshot {
    // ... campos atuais ...
    pub current_index: Option<u32>,
    pub queue_len: u32,
    /// Incrementa a cada mudança na fila: adição, remoção, limpeza, marca de falha.
    pub queue_version: u64,
}
```

O Swift guarda a última versão que leu e só chama `queue_items()` quando ela muda. O destaque
da faixa atual acompanha o auto-avanço sem custo nenhum, porque `current_index` já vem no
snapshot barato.

`queue_version` mora no `SharedStatus`, como átomo, pelo mesmo motivo dos outros campos: é lido
pela interface fora do mutex do engine.

### O gatilho do auto-avanço

`poll_finished()` roda dentro de `snapshot()`, na main actor. Carregar a próxima faixa custa
`stop_and_release` + `acquire`, com espera de hardware de até 2 s. **Isso não pode rodar ali** —
travaria a janela inteira.

Duas saídas foram consideradas. Uma thread de vigília em Rust (`B1`) daria autonomia total ao
lado Rust, ao custo de uma thread com ciclo de vida próprio disputando o mutex do engine com os
comandos do usuário: apertar Next no instante em que a faixa termina vira uma corrida que
precisa ser resolvida à mão. Neste projeto, concorrência com o Core Audio já rendeu cinco
armadilhas que retornam sucesso e falham caladas.

A escolhida (`B2`): o `PlayerViewModel` vê `Finished` no poll que já existe e chama `advance()`
fora da main actor, pelo mesmo caminho serializado que `play`/`pause`/`select` usam. Não há
thread nova e não há corrida — o gatilho chega até 100 ms mais tarde, ruído dentro de um evento
que já custa centenas de ms.

O Swift não ganha lógica de playlist com isso. Ele ganha uma condição de guarda: *acabou, e a
fila tem mais — prossiga*. Quem é a próxima, se ela existe e o que fazer quando ela falha é
decisão inteiramente do Rust.

### Falha de faixa e falha de device

Distinção que o auto-avanço torna obrigatória. O `advance()` marca uma faixa como não-tocável e
pula para a seguinte **apenas** quando a falha é da faixa: arquivo sumiu, está corrompido, ou o
formato foi recusado na negociação.

Quando a falha é do **device** — outro processo tomou o hardware na janela entre soltar e
readquirir —, a faixa é boa e marcá-la seria mentira. Nesse caso o `advance()` para, preserva a
fila intacta e propaga `PlayerError::Device`.

O engine já carrega essa distinção: `PlayFailure::State` contra `PlayFailure::Device`, criada
para a fronteira uniffi. Esta feature passa a depender dela para uma decisão de comportamento,
não só para a mensagem de erro.

## Ingestão

Toda em Swift, num módulo próprio de `HogPlayerKit` para ser testável sem interface:

```swift
/// Expande pastas recursivamente, filtra o que não é áudio e ordena por caminho.
public func audioFiles(from urls: [URL]) -> [URL]
```

- **Pastas expandem recursivamente.** Arrastar a pasta de um álbum joga o álbum inteiro na fila,
  CD1/CD2 inclusos.
- **Ordem alfabética por caminho.** A ordem que o Finder entrega numa multi-seleção é a ordem em
  que os itens foram clicados, não a que aparece na tela; ordenar por caminho põe `01-`, `02-`,
  `03-` na sequência certa.
- **Filtragem silenciosa.** Arquivos que não são áudio são descartados sem mensagem — arrastar
  uma pasta com capas em JPEG dentro é o caso comum, não um erro.
- `ContentView.handleDrop` passa a percorrer **todos** os `providers`, não só o primeiro; o
  `NSOpenPanel` ganha `allowsMultipleSelection = true` e `canChooseDirectories = true`.

Se nada de áudio for encontrado no que foi arrastado, a mensagem diz isso — é o único caso em
que a ingestão fala.

## Interface

A janela cresce para acomodar a lista abaixo dos controles. O painel de cima (capa, título,
artista, álbum, relógio, linha técnica) não muda.

- **Lista**: uma linha por faixa, com o nome do arquivo sem extensão. A faixa atual fica
  destacada; as que falharam ficam esmaecidas, com o motivo em `help` (tooltip).
- **Clicar numa linha** carrega aquela faixa. **Delete** remove a selecionada.
- **Previous / Play-Pause / Next** na mesma fileira, com Previous e Next desabilitados nas
  pontas em vez de virarem erro.
- **Limpar lista** ao lado de "Abrir…".

O `DisplayState` — hoje uma função pura de `(Snapshot, TrackMetadata?, TrackFormat?)` — ganha
`canGoNext` e `canGoPrevious`, calculados da mesma forma que `canPlay`/`canPause` já são. A
regra de habilitar botão continua sendo função pura, testável sem hardware.

`canGoPrevious` depende do **decorrido**, não só do índice: na primeira faixa ele é falso
enquanto o relógio não passa de 3 s e verdadeiro depois disso, porque a partir dali Previous
rebobina em vez de navegar. O botão, portanto, habilita sozinho durante a reprodução da
primeira faixa. `elapsedSeconds` já vem no `Snapshot`, então a função continua pura.

## Comportamentos de borda

| Situação | Comportamento |
|---|---|
| Adicionar numa lista vazia | Seleciona e **carrega** a primeira. **Não toca**: tomar o hog silencia os outros apps do Mac, e isso nunca acontece sem pedido explícito |
| Adicionar com algo tocando | Vai para o fim da fila; a reprodução não é interrompida |
| Clicar numa linha da lista | Mesma regra de Next/Previous: herda o estado atual |
| Next/Previous tocando | A faixa nova já entra tocando |
| Next/Previous carregado ou pausado | Só carrega; não começa a tocar |
| Next na última | Botão desabilitado |
| Previous na primeira, antes de 3 s | Botão desabilitado |
| Previous na primeira, depois de 3 s | Reinicia a faixa (a regra dos 3 s vale igual na primeira) |
| Fim da última faixa | Fica em `Finished`, na última. O device continua retido, como hoje |
| Remover faixa acima da atual | O índice acompanha; nada recarrega |
| Remover a faixa atual | Tratado como Next: carrega quem assumiu a posição. Se era a última, para |
| Remover a última faixa restante | Fila vazia, volta a `Idle`, device liberado |
| Limpar a lista | Para a reprodução e devolve o device ao sistema |
| Faixa marcada como falha, clicada à mão | Tenta de novo e limpa a marca se funcionar — o arquivo pode ter voltado, ou a saída pode ter mudado |
| Mesmo arquivo adicionado duas vezes | Entra duas vezes. A fila é uma fila, não um conjunto |

## Testes

### Rust

`queue.rs` puro, sem hardware nem arquivo:

- `on_next` a partir de cada posição, inclusive da última e da fila vazia
- `on_previous` nos dois lados do limite de 3 s, inclusive na primeira faixa
- `on_finished` na última faixa devolve `Stop`, e não `Load` fora do intervalo
- `next_playable` pulando corridas de faixas marcadas, inclusive quando todas falharam
- `after_removal` para os três casos: acima da atual, a própria atual, abaixo da atual
- todas as funções com fila vazia e com índice `None`

`engine.rs`, sem device:

- `append_tracks` preserva a ordem recebida e incrementa `queue_version`
- `remove_track` fora do intervalo é erro e **não** mexe na fila
- `clear_queue` volta a `Idle`
- falha de formato marca a faixa; falha de device **não** marca

Hardware (`make hw-test`, `--test-threads=1`):

- troca de faixa entre dois sample rates diferentes deixa o device no rate da faixa nova
- auto-avanço percorre a fila inteira e para na última, com o device restaurado no fim
- uma faixa inválida no meio da fila é pulada, e as boas depois dela tocam

### Swift

- `audioFiles(from:)`: pasta aninhada, mistura de áudio e não-áudio, ordenação, lista vazia
- `DisplayState`: `canGoNext`/`canGoPrevious` nas pontas e no meio
- `PlayerViewModel` com `FakePlayer`: ver `Finished` dispara exatamente **um** `advance()`, e não
  um por tique do poll enquanto o carregamento da próxima ainda corre
- a lista só é relida quando `queue_version` muda

O último é o que mais importa: sem ele, o poll de 10 Hz dispararia dez `advance()` por segundo
durante os centenas de milissegundos que a troca de faixa leva.

## Ordem de build

1. `queue.rs` puro, com os testes primeiro — nada mais depende de nada aqui
2. Fila no `Engine` (`Vec<QueueEntry>`, índice, `queue_version` no `SharedStatus`)
3. Superfície uniffi: os métodos novos, os campos novos no `Snapshot`, saída do `load(path)`
4. `audioFiles(from:)` em Swift, com os testes
5. `PlayerViewModel`: fila espelhada, gatilho do `advance`, guarda contra reentrada
6. `DisplayState`: `canGoNext`/`canGoPrevious`
7. `ContentView`: lista, botões, drop múltiplo, painel múltiplo
8. Testes de hardware da troca de faixa

## Fora de escopo

- **Gapless e pré-carga da próxima faixa.** Impossível sem reescrever o núcleo; ver a restrição
  no Contexto
- **Reordenar a lista arrastando.** Decidido fora
- **Shuffle e repeat.** Nem botão, nem estado
- **Persistir a fila entre execuções.** Abrir o app começa com a lista vazia
- **Tags reais nas linhas da lista.** Nome do arquivo basta; metadados ricos continuam só para a
  faixa atual
- **Seek dentro da faixa.** O engine não tem, e Previous rebobina recarregando

## Riscos

- **Reentrada do `advance()`.** O poll roda a 10 Hz e a troca de faixa leva centenas de ms. Sem
  guarda, o estado `Finished` dispararia vários avanços em sequência e a fila saltaria várias
  faixas de uma vez. É o defeito mais provável desta feature, e tem teste dedicado
- **A janela sem hog entre faixas.** Outro processo pode tomar o device nela. Tratado como falha
  de device: para e reporta, sem marcar a faixa
- **Fila grande.** `queue_items()` copia todos os caminhos pelo FFI. Mitigado por só correr
  quando `queue_version` muda; se um dia doer, o próximo passo é paginar, não voltar a espelhar
- **`current_index` e a lista podem divergir por um quadro.** O Swift lê `current_index` do
  snapshot e a lista de uma chamada separada. Um avanço entre as duas leituras deixa o destaque
  em um índice que a lista velha ainda não conhece. Como `queue_version` também vem do mesmo
  snapshot, a releitura acontece no tique seguinte — 100 ms de destaque errado, no pior caso,
  sem consequência para o áudio
