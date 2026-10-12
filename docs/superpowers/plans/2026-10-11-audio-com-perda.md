# Áudio com perda (AAC-LC e MP3): plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** tocar AAC-LC e MP3 no modo exclusivo com o float32 do decodificador entregue intacto ao
device, e mostrar "AAC 339" na tela, sem mudar o caminho sem perda.

**Architecture:** o `FileFormat` troca o `bit_depth` por uma `Resolution` explícita
(`Lossless { bits }` ou `Lossy { bitrate_kbps }`). O `source.rs` classifica o arquivo, e o
`negotiate()` só aceita a faixa com perda num formato físico float32 no rate do arquivo. O
`engine.rs` não muda de lógica: ele já entrega no formato do device, e o `ExtAudioFile` repassa o
float32 do decodificador pelo mesmo `produce_direct`. A API pública ganha `bit_depth: Option<u32>`
e `bitrate_kbps: Option<u32>`; o Swift e o GPUI só ajustam a exibição.

**Tech Stack:** Rust (core, `coreaudio-sys`, `uniffi` 0.32), Swift 6 com Swift Testing
(`apps/player`), GPUI (`apps/gpui`, protótipo), `afconvert` e `ffmpeg` nas fixtures e nas provas.

**Spec:** `docs/superpowers/specs/2026-10-11-audio-com-perda-design.md`. Decisão em
`rust/docs/adr/0002-arquivo-com-perda-sem-perda-do-app.md`. Leia os dois antes da Task 1.

## Global Constraints

- **Regra central:** o app nunca altera uma amostra do que o decodificador entrega.
- **Codecs com perda aceitos:** só `kAudioFormatMPEG4AAC` (`'aac '`) e `kAudioFormatMPEGLayer3`
  (`'.mp3'`). HE-AAC (`'aach'`, `'aacp'`), Opus e os outros continuam recusados.
- **A faixa com perda só toca** em formato físico `SampleType::Float` de 32 bits que cubra o rate
  do arquivo.
- **O rate que o device não suporta continua recusado**, para qualquer formato.
- **Não mudam:** `rust/src/device.rs`, `playback.rs`, `ring.rs`, `recovery.rs` e
  `device_watch.rs`, o IOProc e a ordem hog → rate → formato → volume. O FLAC e os outros formatos
  sem perda seguem o mesmo código.
- **Só o device do usuário conta:** MacBook Air M1, cujo fone e caixas publicam apenas float32.
- **Idioma:** código em inglês; mensagens, strings de UI e nomes de teste em pt-BR, como o
  resto do repo. Docs em pt-BR, sem tabelas novas.
- **Commits:**
  - Conventional em pt-BR, no infinitivo, com escopo obrigatório: `tipo(escopo): descrição`.
  - Assunto de até 72 caracteres, no máximo 20 arquivos, sempre com `-m`.
  - O hook commit-guard nega fora disso, e o bloqueio pode descartar o working tree. Antes de
    cada commit, copie os arquivos alterados para o scratchpad e, se o commit for barrado,
    confira-os contra a cópia antes de tentar de novo.
  - Sem push e sem merge sem pedido do usuário.
- **Device:**
  - Avise o usuário antes de qualquer comando que tome o device: `make verify*`, `--dump` e
    `make hw-test`. O `--info` não toma.
  - Antes de qualquer som, peça o volume em ~10%; o fone costuma estar plugado.
  - O `make hw-test` roda com o app instalado fechado.
- **Ferramentas:**
  - Arquivos se leem com Read e se alteram com Edit/Write; nada de `sed -i`, heredoc ou `echo >`.
  - Código inline no Bash é negado pelo hook scriptsmith.
- **Gate:** `make test` roda `cargo fmt --check` e `cargo clippy --all-targets -- -D warnings`
  antes dos testes. Teste que depende de fixture e não a acha reprova; rode `make fixtures`.
- **Prova do vermelho:** teste verde não prova nada até o código sob teste ser quebrado de
  propósito e o teste ficar vermelho.
  - Cada task tem um passo "Provar o vermelho", com sabotagens que reproduzem o sintoma do risco,
    uma por ramo.
  - Antes de sabotar, copie o arquivo para o scratchpad (`cp arquivo $SCRATCH/arquivo.orig`) e
    restaure com `cp` de volta, nunca com `git checkout`, que apaga o que não foi commitado.
  - Depois de restaurar, `git diff --stat` mostra o mesmo de antes da sabotagem.
  - Sabotagem que não fica vermelha é achado: pare e reporte, sem afrouxar o teste.
- **Asserções:** igualdade exata contra um valor construído (a mensagem inteira, o selo inteiro).
  `contains`, `hasPrefix` e faixas só onde o valor depende do encoder (o bitrate VBR do AAC), e
  o teste diz isso.

## Review Focus

1. **AAC com o tipo de objeto MPEG-4 nos format flags:** `mFormatFlags = 2` (AAC-LC) coincide
   com o flag de 20 bits do ALAC. O arquivo tem de abrir como com perda, nunca como "20 bits".
   Teste `flags_do_aac_nao_viram_profundidade_de_bits` na Task 3.
2. **`.m4a` com ALAC:** o usuário perguntou por ele. Continua sem perda, 16 bits, codec "alac".
   Fixture `t44_alac.m4a` (Task 1) e teste `alac_em_m4a_continua_sem_perda` (Task 3).
3. **HE-AAC:** recusado com "aach" na mensagem, nunca com um número decimal. Testes
   `he_aac_e_recusado_com_o_nome_do_codec` (Task 3) e `heAacViraProblemaComONomeDoCodec` (Task 4).
4. **MP3 ou AAC em 22,05 kHz:** recusado com a mensagem de resample, como o FLAC de 192 kHz.
   Teste `com_perda_em_rate_nao_suportado_e_recusada_por_resample` (Task 2) e conferência com
   `--info` (Task 6).
5. **Bitrate ausente ou que arredonda para zero:** a tela mostra só o codec, nunca "0 kbps".
   Teste `bitrate_que_arredonda_para_zero_vira_ausente` (Task 3) e os casos sem bitrate nas
   Tasks 4 e 5.

---

### Task 0: Branch de trabalho

**Files:**
- Commit: `docs/superpowers/plans/2026-10-11-audio-com-perda.md`

- [ ] **Step 1: Criar a branch a partir da `main`**

```bash
git -C ~/work/audio-toolbox/hog-audio switch -c feat/audio-com-perda main
```

Esperado: `Switched to a new branch 'feat/audio-com-perda'`.

- [ ] **Step 2: Commitar este plano na branch**

```bash
git add docs/superpowers/plans/2026-10-11-audio-com-perda.md
git commit -m "docs(plano): registrar o plano de áudio com perda"
```

---

### Task 1: Fixtures de AAC-LC, HE-AAC, MP3 e ALAC

As fixtures de AAC saem do `afconvert`, o codificador da própria Apple. O ffmpeg não escreve
HE-AAC sem a libfdk, e o AAC da biblioteca real é o da Apple. O MP3 sai do ffmpeg (libmp3lame), que
o script já usa. Todas são estéreo, 44,1 kHz e 6 s, com 440 Hz à esquerda e 660 Hz à direita,
como as fixtures de formato. Os arquivos não são versionados: o `.gitignore` já ignora `*.m4a`
e o resto de `testdata/`.

**Files:**
- Modify: `tools/make_fixtures.sh`

**Interfaces:**
- Produces: `testdata/t44_aac.m4a` (AAC-LC), `testdata/t44_heaac.m4a` (HE-AAC),
  `testdata/t44_mp3.mp3` (MP3 CBR 192 kbps) e `testdata/t44_alac.m4a` (ALAC 16 bits).

- [ ] **Step 1: Acrescentar as funções ao `tools/make_fixtures.sh`**

Logo depois da função `com_tags()` (antes do `echo "fixtures de formato ..."`), inserir:

```bash
# A fonte comum das fixtures com perda: o mesmo estéreo 440/660 Hz das fixtures de formato.
fonte_estereo() {
  local destino=$1
  ffmpeg -v error -y \
    -f lavfi -i "sine=frequency=440:duration=6:sample_rate=44100" \
    -f lavfi -i "sine=frequency=660:duration=6:sample_rate=44100" \
    -filter_complex "[0:a][1:a]amerge=inputs=2[a]" -map "[a]" -c:a pcm_s16le "$destino"
}

# $1 = arquivo, $2 = codec do afconvert (aac | aach | alac), $3 = bitrate em bit/s (0 = padrão)
# O AAC sai do afconvert, o codificador da própria Apple: o ffmpeg não escreve HE-AAC sem a
# libfdk, e o AAC da biblioteca real é o da Apple.
pela_apple() {
  local nome=$1 codec=$2 bitrate=$3 destino="$DEST/$1"
  if [ -f "$destino" ] && [ "$FORCE" != "1" ]; then
    echo "  = $nome (já existe)"
    return
  fi
  local fonte="$TMP/fonte_$nome.wav"
  fonte_estereo "$fonte"
  local taxa=()
  [ "$bitrate" != "0" ] && taxa=(-b "$bitrate")
  afconvert -f m4af -d "$codec" ${taxa[@]+"${taxa[@]}"} "$fonte" "$destino"
  echo "  + $nome ($codec)"
}

# $1 = arquivo; MP3 CBR de 192 kbps, para o bitrate lido ser previsível.
mp3_cbr() {
  local nome=$1 destino="$DEST/$1"
  if [ -f "$destino" ] && [ "$FORCE" != "1" ]; then
    echo "  = $nome (já existe)"
    return
  fi
  local fonte="$TMP/fonte_$nome.wav"
  fonte_estereo "$fonte"
  ffmpeg -v error -y -i "$fonte" -c:a libmp3lame -b:a 192k "$destino"
  echo "  + $nome (mp3 192 kbps)"
}
```

E, logo depois da linha `command -v ffmpeg ...`, a checagem do `afconvert`:

```bash
command -v afconvert >/dev/null || { echo "falta afconvert (vem com o macOS)"; exit 1; }
```

- [ ] **Step 2: Chamar as funções**

Antes do `echo "fixtures de metadados ..."`, inserir:

```bash
echo "fixtures com perda (AAC-LC e MP3 tocam; HE-AAC é recusado) e ALAC em .m4a:"
pela_apple t44_aac.m4a   aac  256000
pela_apple t44_heaac.m4a aach 64000
pela_apple t44_alac.m4a  alac 0
mp3_cbr    t44_mp3.mp3
```

- [ ] **Step 3: Gerar e conferir**

Run: `make fixtures && afinfo testdata/t44_aac.m4a testdata/t44_heaac.m4a testdata/t44_alac.m4a testdata/t44_mp3.mp3 | grep "Data format"`

Esperado:

```
Data format:     2 ch,  44100 Hz, aac  (0x00000000) 0 bits/channel, 0 bytes/packet, 1024 frames/packet, 0 bytes/frame
Data format:     2 ch,  22050 Hz, aac  (0x00000000) ...
Data format:     2 ch,  44100 Hz, alac (0x00000001) from 16-bit source, 4096 frames/packet
Data format:     2 ch,  44100 Hz, .mp3 (0x00000000) 0 bits/channel, 0 bytes/packet, 1152 frames/packet, 0 bytes/frame
```

O HE-AAC aparece como "22050 Hz, aac" no `afinfo`, que mostra a camada base. O `ExtAudioFile`,
que é o que o core usa, vê `aach`; a Task 3 prova isso.

- [ ] **Step 4: Commit**

```bash
git add tools/make_fixtures.sh
git commit -m "test(fixtures): gerar AAC-LC, HE-AAC, MP3 e ALAC pela Apple"
```

---

### Task 2: Resolução explícita e regra da negociação

A mudança de tipo atravessa o crate inteiro (`source.rs`, `engine.rs`, `api.rs`, `main.rs`), então
esta task leva as mudanças mecânicas junto, para o crate compilar. O comportamento não muda:
`source.rs` ainda recusa todo arquivo sem bit depth até a Task 3. As bindings Swift e o GPUI só
voltam a compilar nas Tasks 4 e 5; não rode `make swift-test` nem os testes do GPUI antes delas.

**Files:**
- Modify: `rust/src/format.rs:35-40` (`FileFormat`), `:114-123` (`preserves_every_bit`),
  `:171-193` (laço e recusa do `negotiate`), testes no fim do arquivo
- Modify: `rust/src/source.rs:8`, `:131-141`, `:242-253`
- Modify: `rust/src/engine.rs:45-54` (`LoadedTrack`), `:389-396`, `:2114`
- Modify: `rust/src/api.rs:41-58` (`TrackFormat`, `TrackProbe`), `:493-518`, `:689-700`
- Modify: `rust/src/main.rs:37-44`

**Interfaces:**
- Produces (em `crate::format`):
  - `pub enum Resolution { Lossless { bits: u32 }, Lossy { bitrate_kbps: Option<u32> } }`,
    com `Clone, Copy, PartialEq, Eq, Debug`;
  - `pub struct FileFormat { pub sample_rate: f64, pub resolution: Resolution, pub channels: u32 }`;
  - `FileFormat::bit_depth(&self) -> Option<u32>` e `FileFormat::bitrate_kbps(&self) -> Option<u32>`.
- Produces (em `crate::engine::LoadedTrack`): `pub bit_depth: Option<u32>` e
  `pub bitrate_kbps: Option<u32>`.
- Produces (uniffi): `TrackFormat` com os campos na ordem `sample_rate, bit_depth: Option<u32>,
  bitrate_kbps: Option<u32>, channels, codec, device_name, total_seconds`; `TrackProbe` na ordem
  `sample_rate, bit_depth: Option<u32>, bitrate_kbps: Option<u32>, channels, codec,
  total_seconds`. No Swift, os inits seguem essa ordem (`bitDepth:`, `bitrateKbps:`).

- [ ] **Step 1: Escrever os testes da negociação com perda (`rust/src/format.rs`, módulo `tests`)**

Trocar o helper `file()` existente por este, e acrescentar `lossy()` e `mac_air_like()` logo
abaixo dele:

```rust
    fn file(rate: f64, bits: u32, channels: u32) -> FileFormat {
        FileFormat {
            sample_rate: rate,
            resolution: Resolution::Lossless { bits },
            channels,
        }
    }

    fn lossy(rate: f64) -> FileFormat {
        FileFormat {
            sample_rate: rate,
            resolution: Resolution::Lossy {
                bitrate_kbps: Some(256),
            },
            channels: 2,
        }
    }

    // O fone e as caixas do MacBook Air M1: só float32, em quatro rates discretos.
    fn mac_air_like() -> DeviceCaps {
        DeviceCaps {
            rates: vec![
                discrete(44100.0),
                discrete(48000.0),
                discrete(88200.0),
                discrete(96000.0),
            ],
            physical_formats: vec![
                float_format(0, 44100.0),
                float_format(1, 48000.0),
                float_format(2, 88200.0),
                float_format(3, 96000.0),
            ],
            output_channels: 2,
        }
    }
```

No fim do módulo `tests` (antes do bloco "validação do formato de entrega"), acrescentar:

```rust
    // --- faixa com perda (ADR 0002 do core) ------------------------------------------

    #[test]
    fn com_perda_toca_em_float32_no_rate_do_arquivo() {
        let d = negotiate(&lossy(48000.0), &mac_air_like());

        assert!(d.play, "{}", d.reason);
        assert_eq!(d.sample_rate, 48000.0);
        assert_eq!(d.physical_format_index, 1);
    }

    #[test]
    fn com_perda_escolhe_float_mesmo_com_inteiro_no_rate() {
        let caps = DeviceCaps {
            rates: vec![discrete(48000.0)],
            physical_formats: vec![int_format(0, 48000.0, 24), float_format(1, 48000.0)],
            output_channels: 2,
        };

        let d = negotiate(&lossy(48000.0), &caps);

        assert!(d.play);
        assert_eq!(d.physical_format_index, 1);
    }

    // Arredondar o float do decodificador para inteiro seria perda acrescentada pelo app.
    #[test]
    fn com_perda_em_device_so_inteiro_e_recusada() {
        let d = negotiate(&lossy(44100.0), &built_in_like());

        assert!(!d.play);
        assert_eq!(d.physical_format_index, -1);
        assert_eq!(
            d.reason,
            "nenhum formato físico do device em 44100 Hz é float32; entregar em inteiro \
             arredondaria o que o decodificador produziu"
        );
    }

    #[test]
    fn com_perda_nao_aceita_float_de_outra_largura() {
        let mut float64 = float_format(0, 44100.0);
        float64.bits_per_channel = 64;
        let caps = DeviceCaps {
            rates: vec![discrete(44100.0)],
            physical_formats: vec![float64],
            output_channels: 2,
        };

        assert!(!negotiate(&lossy(44100.0), &caps).play);
    }

    // MP3 e AAC antigos em 22,05 kHz: a mesma recusa do FLAC de 192 kHz, até o resample existir.
    #[test]
    fn com_perda_em_rate_nao_suportado_e_recusada_por_resample() {
        let d = negotiate(&lossy(22050.0), &mac_air_like());

        assert!(!d.play);
        assert_eq!(
            d.reason,
            "arquivo em 22050 Hz; o device suporta: 44100, 48000, 88200, 96000. Reproduzir \
             exigiria resample."
        );
    }

    #[test]
    fn sem_perda_continua_tocando_no_device_so_float32() {
        let d = negotiate(&file(44100.0, 16, 2), &mac_air_like());

        assert!(d.play);
        assert_eq!(d.physical_format_index, 0);
    }
```

- [ ] **Step 2: Rodar para ver falhar**

Run: `(cd rust && cargo test --lib format::)`
Esperado: erro de compilação, porque `Resolution` não existe e o `FileFormat` não tem o campo
`resolution`.

- [ ] **Step 3: Implementar em `rust/src/format.rs`**

Trocar o `FileFormat` (linhas 35-40) por:

```rust
/// A resolução do arquivo. A faixa com perda não tem profundidade de bits: o decodificador
/// entrega float32, e o app não pode alterá-lo (ADR 0002 do core).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resolution {
    Lossless { bits: u32 },
    Lossy { bitrate_kbps: Option<u32> },
}

#[derive(Clone, Copy, Debug)]
pub struct FileFormat {
    pub sample_rate: f64,
    pub resolution: Resolution,
    pub channels: u32,
}

impl FileFormat {
    pub fn bit_depth(&self) -> Option<u32> {
        match self.resolution {
            Resolution::Lossless { bits } => Some(bits),
            Resolution::Lossy { .. } => None,
        }
    }

    pub fn bitrate_kbps(&self) -> Option<u32> {
        match self.resolution {
            Resolution::Lossless { .. } => None,
            Resolution::Lossy { bitrate_kbps } => bitrate_kbps,
        }
    }
}
```

Trocar o `preserves_every_bit` (linhas 118-123), mantendo o `FLOAT_EXACT_BITS` e o comentário
dele acima, por:

```rust
// Com perda, o decodificador entrega float32: só um formato físico float32 o recebe sem que o
// app arredonde nada (ADR 0002 do core).
fn preserves_every_bit(fmt: &PhysicalFormatDesc, resolution: Resolution) -> bool {
    const DECODER_FLOAT_BITS: u32 = 32;
    match (resolution, fmt.sample_type) {
        (Resolution::Lossless { bits }, SampleType::Float) => bits <= FLOAT_EXACT_BITS,
        (Resolution::Lossless { bits }, SampleType::Integer) => fmt.bits_per_channel >= bits,
        (Resolution::Lossy { .. }, SampleType::Float) => {
            fmt.bits_per_channel == DECODER_FLOAT_BITS
        }
        (Resolution::Lossy { .. }, SampleType::Integer) => false,
    }
}
```

No laço do `negotiate`, trocar `if !preserves_every_bit(fmt, file.bit_depth) {` por
`if !preserves_every_bit(fmt, file.resolution) {`. E trocar o bloco `let Some(best) = best else`
inteiro por:

```rust
    let Some(best) = best else {
        let rate = rate_text(file.sample_rate);
        d.reason = match file.resolution {
            Resolution::Lossless { bits } => format!(
                "nenhum formato físico do device em {rate} Hz comporta {bits} bits sem perda"
            ),
            Resolution::Lossy { .. } => format!(
                "nenhum formato físico do device em {rate} Hz é float32; entregar em inteiro \
                 arredondaria o que o decodificador produziu"
            ),
        };
        return d;
    };
```

- [ ] **Step 4: Ajustar os consumidores para o crate compilar**

`rust/src/source.rs`, linha 8:

```rust
use crate::format::{FileFormat, Resolution};
```

`rust/src/source.rs`, no `Ok(Self { ... })` do `open` (linhas 131-141), o `format` passa a ser:

```rust
            format: FileFormat {
                sample_rate: asbd.mSampleRate,
                resolution: Resolution::Lossless { bits: bit_depth },
                channels: asbd.mChannelsPerFrame,
            },
```

`rust/src/source.rs`, no teste `le_formato_do_flac_de_teste`, trocar
`assert_eq!(f.bit_depth, 24);` por:

```rust
        assert_eq!(f.resolution, Resolution::Lossless { bits: 24 });
```

`rust/src/engine.rs`, no `LoadedTrack` (linhas 45-54), trocar `pub bit_depth: u32,` por:

```rust
    /// `None` na faixa com perda, que não tem profundidade de bits.
    pub bit_depth: Option<u32>,
    /// Só na faixa com perda: o bitrate médio que o arquivo publica.
    pub bitrate_kbps: Option<u32>,
```

`rust/src/engine.rs`, no `Ok(LoadedTrack { ... })` do `load_classified` (linha ~391), trocar
`bit_depth: file_format.bit_depth,` por:

```rust
            bit_depth: file_format.bit_depth(),
            bitrate_kbps: file_format.bitrate_kbps(),
```

`rust/src/engine.rs`, no teste `load_de_flac_valido_vai_para_loaded` (linha ~2114), trocar
`assert_eq!(track.bit_depth, 24);` por:

```rust
        assert_eq!(track.bit_depth, Some(24));
        assert_eq!(track.bitrate_kbps, None);
```

`rust/src/api.rs`: no `TrackFormat` e no `TrackProbe`, trocar `pub bit_depth: u32,` por:

```rust
    /// `None` na faixa com perda: o arquivo não tem profundidade de bits (ADR 0002 do core).
    pub bit_depth: Option<u32>,
    /// Só na faixa com perda: o bitrate médio que o arquivo publica.
    pub bitrate_kbps: Option<u32>,
```

`rust/src/api.rs`, no `track_format`, depois de `bit_depth: track.bit_depth,`:

```rust
        bitrate_kbps: track.bitrate_kbps,
```

`rust/src/api.rs`, no `probe_track`, trocar `bit_depth: format.bit_depth,` por:

```rust
        bit_depth: format.bit_depth(),
        bitrate_kbps: format.bitrate_kbps(),
```

`rust/src/api.rs`, no teste `sondar_devolve_o_formato_que_o_engine_vai_ver`, trocar
`assert_eq!(probe.bit_depth, 24);` por:

```rust
        assert_eq!(probe.bit_depth, Some(24));
        assert_eq!(probe.bitrate_kbps, None);
```

`rust/src/main.rs`: trocar `print_track_report` (linhas 37-44) por:

```rust
fn print_track_report(path: &str, track: &LoadedTrack) {
    println!("arquivo  : {path}");
    println!(
        "fonte    : {} {} Hz / {} / {} canais / {:.1} s",
        track.codec,
        track.sample_rate,
        resolution_text(track),
        track.channels,
        track.total_seconds
    );
    println!("device   : {}", track.device_name);
}

/// "24 bits" no arquivo sem perda; "339 kbps" no com perda, que não tem bits a mostrar.
fn resolution_text(track: &LoadedTrack) -> String {
    match (track.bit_depth, track.bitrate_kbps) {
        (Some(bits), _) => format!("{bits} bits"),
        (None, Some(kbps)) => format!("{kbps} kbps"),
        (None, None) => "com perda".to_string(),
    }
}
```

- [ ] **Step 5: Rodar os testes e o gate do Rust**

Run: `make fmt-check lint rust-test`
Esperado: tudo verde. Os seis testes novos passam e os antigos continuam passando. Se o `fmt-check`
reprovar, rode `make fmt` e repita.

- [ ] **Step 6: Conferir que o `--info` de um FLAC não mudou**

Run: `make rust && ./rust/target/release/hog-audio --info testdata/t96_24.flac`
Esperado: `fonte    : flac 96000 Hz / 24 bits / 2 canais / 6.0 s` e
`negociado: 96000 Hz / 32 bits float`. O `--info` não toma o device.

- [ ] **Step 7: Provar o vermelho**

Copie `rust/src/format.rs` para o scratchpad. Aplique uma sabotagem por vez, rode
`(cd rust && cargo test --lib format::)` e confira o vermelho; restaure por `cp` antes da
próxima.

1. **Faixa com perda arredondada para inteiro.** No `preserves_every_bit`, trocar
   `(Resolution::Lossy { .. }, SampleType::Integer) => false` por `=> true`.
   Esperado vermelho: `com_perda_em_device_so_inteiro_e_recusada` e
   `com_perda_escolhe_float_mesmo_com_inteiro_no_rate`.
2. **Float de qualquer largura.** Trocar o corpo do braço `(Resolution::Lossy { .. },
   SampleType::Float)` por `true`.
   Esperado vermelho: `com_perda_nao_aceita_float_de_outra_largura`.
3. **Faixa com perda pulando a checagem de rate.** Envolver o `if !rate_is_supported(...)
   { ... }` do `negotiate` em `if matches!(file.resolution, Resolution::Lossless { .. }) { ... }`.
   Esperado vermelho: `com_perda_em_rate_nao_suportado_e_recusada_por_resample`.

Depois de restaurar: `(cd rust && cargo test --lib format::)` verde e `git diff --stat` igual ao
de antes das sabotagens.

- [ ] **Step 8: Commit**

```bash
git add rust/src/format.rs rust/src/source.rs rust/src/engine.rs rust/src/api.rs rust/src/main.rs
git commit -m "feat(core): separar resolução sem perda e com perda na negociação"
```

---

### Task 3: Abrir AAC-LC e MP3 como faixa com perda

**Files:**
- Modify: `rust/src/source.rs` (classificação, `four_cc`, bitrate e testes)
- Modify: `rust/src/api.rs` (teste do `probe_track` de AAC)
- Modify: `rust/src/engine.rs` (teste de hardware da carga de AAC)

**Interfaces:**
- Consumes: `Resolution` e `FileFormat` da Task 2; as fixtures da Task 1.
- Produces: `AudioSource::open` devolvendo `Resolution::Lossy { bitrate_kbps }` para `'aac '` e
  `'.mp3'`, e `Err("aach: sem profundidade de bits e fora dos codecs com perda suportados
  (AAC-LC e MP3)")` para os outros codecs sem bit depth. O `codec_name()` passa a vir limpo:
  "aac", "mp3", "flac", "alac".

- [ ] **Step 1: Escrever os testes (`rust/src/source.rs`, módulo `tests`)**

```rust
    #[test]
    fn codigo_do_codec_sai_sem_espaco_e_sem_ponto() {
        assert_eq!(four_cc(kAudioFormatMPEG4AAC), "aac");
        assert_eq!(four_cc(kAudioFormatMPEGLayer3), "mp3");
        assert_eq!(four_cc(kAudioFormatMPEG4AAC_HE), "aach");
        assert_eq!(four_cc(kAudioFormatFLAC), "flac");
        assert_eq!(four_cc(1), "1");
    }

    // No AAC os format flags guardam o tipo de objeto MPEG-4 (2 = LC), que coincide com o flag
    // de 20 bits do ALAC: lidos como profundidade, um AAC viraria "20 bits sem perda".
    #[test]
    fn flags_do_aac_nao_viram_profundidade_de_bits() {
        assert_eq!(declared_bit_depth(kAudioFormatMPEG4AAC, 2, 0), 0);
        assert_eq!(declared_bit_depth(kAudioFormatMPEGLayer3, 0, 0), 0);
    }

    #[test]
    fn flac_e_alac_leem_a_profundidade_dos_flags() {
        assert_eq!(
            declared_bit_depth(kAudioFormatFLAC, kAppleLosslessFormatFlag_24BitSourceData, 0),
            24
        );
        assert_eq!(
            declared_bit_depth(
                kAudioFormatAppleLossless,
                kAppleLosslessFormatFlag_16BitSourceData,
                0
            ),
            16
        );
        assert_eq!(declared_bit_depth(kAudioFormatLinearPCM, 0, 16), 16);
    }

    #[test]
    fn bitrate_que_arredonda_para_zero_vira_ausente() {
        assert_eq!(to_kbps(0), None);
        assert_eq!(to_kbps(499), None);
        assert_eq!(to_kbps(339_163), Some(339));
        assert_eq!(to_kbps(191_998), Some(192));
        assert_eq!(to_kbps(u32::MAX), Some(4_294_967));
    }

    #[test]
    fn aac_lc_abre_como_faixa_com_perda_com_bitrate() {
        let source = AudioSource::open(&crate::fixtures::path("t44_aac.m4a"))
            .expect("deveria abrir o AAC-LC");
        let f = source.format();

        assert_eq!(source.codec_name(), "aac");
        assert_eq!(f.sample_rate, 44100.0);
        assert_eq!(f.channels, 2);
        let Resolution::Lossy {
            bitrate_kbps: Some(kbps),
        } = f.resolution
        else {
            panic!("deveria ser com perda e com bitrate: {:?}", f.resolution);
        };
        // Faixa, e não valor exato: o AAC sai em VBR, e o bitrate depende do encoder da Apple.
        assert!(kbps > 0 && kbps <= 400, "{kbps}");
        let seconds = source.total_frames() as f64 / f.sample_rate;
        assert!((seconds - 6.0).abs() < 0.05, "{seconds}");
    }

    #[test]
    fn mp3_abre_como_faixa_com_perda_com_bitrate() {
        let source = AudioSource::open(&crate::fixtures::path("t44_mp3.mp3"))
            .expect("deveria abrir o MP3");

        assert_eq!(source.codec_name(), "mp3");
        let Resolution::Lossy {
            bitrate_kbps: Some(kbps),
        } = source.format().resolution
        else {
            panic!("deveria ser com perda e com bitrate");
        };
        assert!((190..=194).contains(&kbps), "{kbps}");
    }

    #[test]
    fn he_aac_e_recusado_com_o_nome_do_codec() {
        let Err(erro) = AudioSource::open(&crate::fixtures::path("t44_heaac.m4a")) else {
            panic!("o HE-AAC fica fora por enquanto");
        };

        assert_eq!(
            erro,
            "aach: sem profundidade de bits e fora dos codecs com perda suportados \
             (AAC-LC e MP3)"
        );
    }

    #[test]
    fn alac_em_m4a_continua_sem_perda() {
        let source = AudioSource::open(&crate::fixtures::path("t44_alac.m4a"))
            .expect("deveria abrir o ALAC");

        assert_eq!(source.codec_name(), "alac");
        assert_eq!(source.format().resolution, Resolution::Lossless { bits: 16 });
    }
```

`rust/src/api.rs`, no módulo `tests`, depois de `sondar_devolve_o_formato_que_o_engine_vai_ver`:

```rust
    #[test]
    fn sondar_aac_devolve_bitrate_sem_bit_depth() {
        let probe =
            probe_track(crate::fixtures::path("t44_aac.m4a")).expect("deveria sondar o AAC");

        assert_eq!(probe.bit_depth, None);
        assert!(probe.bitrate_kbps.is_some_and(|kbps| kbps > 0));
        assert_eq!(probe.codec, "aac");
        assert_eq!(probe.sample_rate, 44_100.0);
    }
```

`rust/src/engine.rs`, no módulo `tests`, depois de `load_de_flac_valido_vai_para_loaded`:

```rust
    #[test]
    #[ignore = "precisa de um device de saída real"]
    fn load_de_aac_negocia_float32_no_rate_do_arquivo() {
        let engine = Engine::new();
        let track = engine
            .load(&crate::fixtures::path("t44_aac.m4a"))
            .expect("deveria carregar o AAC");

        assert_eq!(engine.state(), PlayerState::Loaded);
        assert_eq!(track.bit_depth, None);
        assert!(track.bitrate_kbps.is_some());
        let report = engine.device_report().expect("deveria ter negociado");
        assert!(report.negotiated.starts_with("44100 Hz"), "{}", report.negotiated);
        assert!(report.negotiated.contains("float"), "{}", report.negotiated);
    }
```

- [ ] **Step 2: Rodar para ver falhar**

Run: `(cd rust && cargo test --lib)`
Esperado: erro de compilação, porque `declared_bit_depth` e `to_kbps` não existem.

- [ ] **Step 3: Implementar em `rust/src/source.rs`**

Trocar a função `four_cc` (linhas 40-52) por:

```rust
fn four_cc(value: u32) -> String {
    let chars = value.to_be_bytes();
    if !chars.iter().all(|c| c.is_ascii_graphic() || *c == b' ') {
        return format!("{value}");
    }
    // Os códigos da Apple são `'aac '` e `'.mp3'`; mensagens e tela querem "aac" e "mp3".
    String::from_utf8_lossy(&chars)
        .trim_end()
        .trim_start_matches('.')
        .to_string()
}

/// A profundidade que o arquivo declara, ou 0 quando ele não tem uma. FLAC e ALAC deixam
/// `mBitsPerChannel` zerado e a guardam nos format flags; noutros codecs os flags significam
/// outra coisa (no AAC, o tipo de objeto MPEG-4) e não podem ser lidos assim.
#[allow(non_upper_case_globals)]
fn declared_bit_depth(format_id: u32, flags: u32, bits_per_channel: u32) -> u32 {
    if bits_per_channel != 0 {
        return bits_per_channel;
    }
    match format_id {
        kAudioFormatAppleLossless | kAudioFormatFLAC => bit_depth_from_source_flags(flags),
        _ => 0,
    }
}

/// Os codecs com perda que tocam com a saída do decodificador intacta (ADR 0002 do core). O
/// HE-AAC fica fora: o rate que ele declara pode ser o do núcleo, e o `ExtAudioFile`
/// resamplearia em silêncio.
fn is_supported_lossy(format_id: u32) -> bool {
    format_id == kAudioFormatMPEG4AAC || format_id == kAudioFormatMPEGLayer3
}

/// Bit/s em kbps, arredondado. Zero vira `None`: a tela mostra só o codec, nunca "0 kbps".
fn to_kbps(bits_per_second: u32) -> Option<u32> {
    let kbps = bits_per_second.saturating_add(500) / 1000;
    (kbps > 0).then_some(kbps)
}

/// O bitrate médio que o `AudioFile` por baixo do `ExtAudioFile` publica. `None` quando a
/// leitura falha: a faixa toca do mesmo jeito.
fn read_bitrate_kbps(file: ExtAudioFileRef) -> Option<u32> {
    // O `AudioFileID` pertence ao `ExtAudioFile`: não se fecha aqui.
    let mut audio_file: AudioFileID = std::ptr::null_mut();
    let mut size = std::mem::size_of::<AudioFileID>() as u32;
    let status = unsafe {
        ExtAudioFileGetProperty(
            file,
            kExtAudioFileProperty_AudioFile,
            &mut size,
            &mut audio_file as *mut _ as *mut _,
        )
    };
    if status != 0 || audio_file.is_null() {
        return None;
    }

    let mut bits_per_second: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        AudioFileGetProperty(
            audio_file,
            kAudioFilePropertyBitRate,
            &mut size,
            &mut bits_per_second as *mut _ as *mut _,
        )
    };
    if status != 0 {
        return None;
    }
    to_kbps(bits_per_second)
}
```

No `open`, trocar o trecho das linhas 98-111 (do `let bit_depth = ...` até o fim do
`if bit_depth == 0 { ... }`) por:

```rust
        let codec_name = four_cc(asbd.mFormatID);
        let bit_depth =
            declared_bit_depth(asbd.mFormatID, asbd.mFormatFlags, asbd.mBitsPerChannel);
        let resolution = if bit_depth != 0 {
            Resolution::Lossless { bits: bit_depth }
        } else if is_supported_lossy(asbd.mFormatID) {
            Resolution::Lossy {
                bitrate_kbps: read_bitrate_kbps(file),
            }
        } else {
            unsafe { ExtAudioFileDispose(file) };
            return Err(format!(
                "{codec_name}: sem profundidade de bits e fora dos codecs com perda suportados \
                 (AAC-LC e MP3)"
            ));
        };
```

E, no `Ok(Self { ... })`, o `format` passa a usar a `resolution` calculada:

```rust
            format: FileFormat {
                sample_rate: asbd.mSampleRate,
                resolution,
                channels: asbd.mChannelsPerFrame,
            },
```

- [ ] **Step 4: Rodar os testes e o gate**

Run: `make fmt-check lint rust-test`
Esperado: tudo verde, com os nove testes novos passando. O teste de hardware
`load_de_aac_negocia_float32_no_rate_do_arquivo` fica marcado como ignorado e roda na Task 6.

- [ ] **Step 5: Conferir com o `--info`, que não toma o device**

Run: `make rust && ./rust/target/release/hog-audio --info testdata/t44_aac.m4a && ./rust/target/release/hog-audio --info testdata/t44_heaac.m4a`

Esperado no AAC: `fonte    : aac 44100 Hz / <N> kbps / 2 canais / 6.0 s` e
`negociado: 44100 Hz / 32 bits float`.
Esperado no HE-AAC: `erro: aach: sem profundidade de bits e fora dos codecs com perda
suportados (AAC-LC e MP3)`, com saída 1.

- [ ] **Step 6: Provar o vermelho**

Copie `rust/src/source.rs` para o scratchpad. Uma sabotagem por vez, com
`(cd rust && cargo test --lib source::)`, e restauração por `cp` antes da próxima.

1. **AAC lido como ALAC (o risco dos flags).** No `declared_bit_depth`, trocar o `match` inteiro
   por `bit_depth_from_source_flags(flags)`, que é o que o core fazia antes.
   Esperado vermelho: `flags_do_aac_nao_viram_profundidade_de_bits`.
   Esperado verde: `flac_e_alac_leem_a_profundidade_dos_flags`. Ele cobre o outro ramo e é
   sabotado no item 2.
2. **FLAC e ALAC sem ler os flags.** Trocar o braço
   `kAudioFormatAppleLossless | kAudioFormatFLAC => ...` por `=> 0`.
   Esperado vermelho: `flac_e_alac_leem_a_profundidade_dos_flags`, `alac_em_m4a_continua_sem_perda`
   e `le_formato_do_flac_de_teste`.
3. **HE-AAC aceito.** No `is_supported_lossy`, acrescentar `|| format_id == kAudioFormatMPEG4AAC_HE`.
   Esperado vermelho: `he_aac_e_recusado_com_o_nome_do_codec`.
4. **MP3 recusado.** No `is_supported_lossy`, apagar `|| format_id == kAudioFormatMPEGLayer3`.
   Esperado vermelho: `mp3_abre_como_faixa_com_perda_com_bitrate`.
5. **"0 kbps" na tela.** No `to_kbps`, trocar `(kbps > 0).then_some(kbps)` por `Some(kbps)`.
   Esperado vermelho: `bitrate_que_arredonda_para_zero_vira_ausente`.
6. **Código do codec sujo.** No `four_cc`, apagar o `.trim_end()`.
   Esperado vermelho: `codigo_do_codec_sai_sem_espaco_e_sem_ponto` e `aac_lc_abre_como_faixa_com_perda_com_bitrate`.

Depois de restaurar: `make rust-test` verde e `git diff --stat` igual ao de antes.

- [ ] **Step 7: Commit**

```bash
git add rust/src/source.rs rust/src/api.rs rust/src/engine.rs
git commit -m "feat(core): abrir AAC-LC e MP3 como faixa com perda"
```

---

### Task 4: Exibição no app SwiftUI

As bindings (`apps/player/Sources/HogAudioBindings`, `HogAudioFFI`) não são versionadas; o
`make swift-test` as regenera a partir da staticlib.

**Files:**
- Modify: `apps/player/Sources/HogPlayerKit/Quality.swift`
- Modify: `apps/player/Sources/HogPlayerKit/DisplayState.swift:37-44`
- Modify: `apps/player/Sources/HogPlayerKit/QueueEntry.swift:45-50`
- Test: `apps/player/Tests/HogPlayerKitTests/QueueEntryTests.swift`,
  `DisplayTests.swift`, `ViewModelTests.swift:59-62`

**Interfaces:**
- Consumes: `TrackFormat(sampleRate:bitDepth:bitrateKbps:channels:codec:deviceName:totalSeconds:)`
  e `TrackProbe(sampleRate:bitDepth:bitrateKbps:channels:codec:totalSeconds:)`, com
  `bitDepth: UInt32?` e `bitrateKbps: UInt32?`.
- Produces: `qualityDetail(sampleRate: Double, bitDepth: UInt32?, bitrateKbps: UInt32?, codec: String) -> String`
  e `compactQuality(sampleRate: Double, bitDepth: UInt32?, bitrateKbps: UInt32?, codec: String) -> String`.

- [ ] **Step 1: Escrever os testes**

`QueueEntryTests.swift`: trocar a `sonda` e o teste `seloCompactoUsaBitsBarraKilohertz` por:

```swift
private let sonda = TrackProbe(
    sampleRate: 44100, bitDepth: 24, bitrateKbps: nil, channels: 2, codec: "flac",
    totalSeconds: 149.6
)

@Test func seloCompactoUsaBitsBarraKilohertz() {
    #expect(compactQuality(sampleRate: 44100, bitDepth: 24, bitrateKbps: nil, codec: "flac") == "24/44.1")
    #expect(compactQuality(sampleRate: 96000, bitDepth: 24, bitrateKbps: nil, codec: "flac") == "24/96")
    #expect(compactQuality(sampleRate: 192_000, bitDepth: 24, bitrateKbps: nil, codec: "flac") == "24/192")
    #expect(compactQuality(sampleRate: 44100, bitDepth: 16, bitrateKbps: nil, codec: "flac") == "16/44.1")
}

@Test func faixaComPerdaMostraCodecEBitrateNoLugarDosBits() {
    #expect(compactQuality(sampleRate: 48000, bitDepth: nil, bitrateKbps: 339, codec: "aac") == "AAC 339")
    #expect(qualityDetail(sampleRate: 48000, bitDepth: nil, bitrateKbps: 339, codec: "aac") == "48 kHz · AAC 339 kbps")
}

@Test func faixaComPerdaSemBitrateMostraSoOCodec() {
    #expect(compactQuality(sampleRate: 44100, bitDepth: nil, bitrateKbps: nil, codec: "mp3") == "MP3")
    #expect(qualityDetail(sampleRate: 44100, bitDepth: nil, bitrateKbps: nil, codec: "mp3") == "44.1 kHz · MP3")
}
```

No fim de `QueueEntryTests.swift`:

```swift
@Test func cargaRealDeAacMostraCodecEBitrate() async throws {
    let path = try fixture("t44_aac.m4a").path
    // O bitrate do AAC é VBR e depende do encoder: o esperado vem da mesma sondagem do core.
    let kbps = try #require(try probeTrack(path: path).bitrateKbps)
    let info = await loadQueueEntry(path: path)
    #expect(info.problem == nil)
    #expect(info.quality == "AAC \(kbps)")
    #expect(info.qualityDetail == "44.1 kHz · AAC \(kbps) kbps")
    #expect(info.duration == "0:06")
}

@Test func heAacViraProblemaComONomeDoCodec() async throws {
    let info = await loadQueueEntry(path: try fixture("t44_heaac.m4a").path)
    #expect(
        info.problem
            == "aach: sem profundidade de bits e fora dos codecs com perda suportados (AAC-LC e MP3)"
    )
    #expect(info.quality == nil)
}
```

`DisplayTests.swift`: nos dois `TrackFormat(...)` (a `faixa` e o `cd`), acrescentar
`bitrateKbps: nil,` depois de `bitDepth:`:

```swift
private let faixa = TrackFormat(
    sampleRate: 96000, bitDepth: 24, bitrateKbps: nil, channels: 2,
    codec: "flac", deviceName: "Fones de Ouvido Externos", totalSeconds: 208.7
)
```

```swift
    let cd = TrackFormat(
        sampleRate: 44100, bitDepth: 16, bitrateKbps: nil, channels: 2,
        codec: "flac", deviceName: "Fones de Ouvido Externos", totalSeconds: 208.7
    )
```

E no fim do arquivo:

```swift
@Test func aLinhaTecnicaDaFaixaComPerdaMostraCodecEBitrate() {
    let aac = TrackFormat(
        sampleRate: 48000, bitDepth: nil, bitrateKbps: 339, channels: 2,
        codec: "aac", deviceName: "Fones de Ouvido Externos", totalSeconds: 208.7
    )
    let d = displayState(snapshot: snap(.playing), metadata: tags, format: aac)
    #expect(d.technicalLine == "48 kHz · AAC 339 kbps · hog ativo")
}
```

`ViewModelTests.swift`, linhas 59-62:

```swift
    private func track() -> TrackFormat {
        TrackFormat(sampleRate: 44100, bitDepth: 16, bitrateKbps: nil, channels: 2,
                    codec: "flac", deviceName: "Fake", totalSeconds: 10)
    }
```

- [ ] **Step 2: Rodar para ver falhar**

Run: `make swift-test`
Esperado: erro de compilação em `compactQuality` e `qualityDetail`, que ainda não aceitam
`bitrateKbps:` e `codec:`.

- [ ] **Step 3: Implementar**

`Quality.swift`: trocar as duas funções públicas (linhas 15-24) por:

```swift
/// "44.1 kHz · 24 bit · flac" ou, na faixa com perda, "48 kHz · AAC 339 kbps": a linha técnica
/// sob a capa e o tooltip do selo da fila saem daqui, para as duas nunca descreverem a mesma
/// faixa de jeitos diferentes. `bitDepth` vazio é a faixa com perda (ADR 0002 do core).
public func qualityDetail(
    sampleRate: Double, bitDepth: UInt32?, bitrateKbps: UInt32?, codec: String
) -> String {
    let rate = "\(formatKilohertz(sampleRate)) kHz"
    if let bitDepth {
        return "\(rate) · \(bitDepth) bit · \(codec)"
    }
    if let bitrateKbps {
        return "\(rate) · \(codec.uppercased()) \(bitrateKbps) kbps"
    }
    return "\(rate) · \(codec.uppercased())"
}

/// "24/44.1" na convenção bits/kHz das lojas de hi-res; "AAC 339" na faixa com perda, que não
/// tem bits a mostrar.
public func compactQuality(
    sampleRate: Double, bitDepth: UInt32?, bitrateKbps: UInt32?, codec: String
) -> String {
    if let bitDepth {
        return "\(bitDepth)/\(formatKilohertz(sampleRate))"
    }
    if let bitrateKbps {
        return "\(codec.uppercased()) \(bitrateKbps)"
    }
    return codec.uppercased()
}
```

`DisplayState.swift`, a chamada de `qualityDetail`:

```swift
            qualityDetail(
                sampleRate: format.sampleRate,
                bitDepth: format.bitDepth,
                bitrateKbps: format.bitrateKbps,
                codec: format.codec
            ),
```

`QueueEntry.swift`, no `case .probed(let format):`:

```swift
            quality: compactQuality(
                sampleRate: format.sampleRate,
                bitDepth: format.bitDepth,
                bitrateKbps: format.bitrateKbps,
                codec: format.codec
            ),
            qualityDetail: qualityDetail(
                sampleRate: format.sampleRate,
                bitDepth: format.bitDepth,
                bitrateKbps: format.bitrateKbps,
                codec: format.codec
            ),
```

- [ ] **Step 4: Rodar o gate inteiro**

Run: `make test`
Esperado: fmt, clippy, Rust e Swift verdes. O Swift tinha 87 testes e passa a ter 92.

- [ ] **Step 5: Provar o vermelho**

Copie `Quality.swift` para o scratchpad. Uma sabotagem por vez, com
`(cd apps/player && swift test)`, e restauração por `cp`.

1. **"0 bit" na faixa com perda.** No `qualityDetail` e no `compactQuality`, trocar
   `if let bitDepth {` por `if let bitDepth = Optional(bitDepth ?? 0) {`.
   Esperado vermelho: `faixaComPerdaMostraCodecEBitrateNoLugarDosBits`,
   `faixaComPerdaSemBitrateMostraSoOCodec`, `cargaRealDeAacMostraCodecEBitrate` e
   `aLinhaTecnicaDaFaixaComPerdaMostraCodecEBitrate`.
2. **Bitrate ignorado.** Apagar os dois blocos `if let bitrateKbps { ... }`.
   Esperado vermelho: `faixaComPerdaMostraCodecEBitrateNoLugarDosBits`,
   `cargaRealDeAacMostraCodecEBitrate` e `aLinhaTecnicaDaFaixaComPerdaMostraCodecEBitrate`.

Depois de restaurar: `(cd apps/player && swift test)` verde e `git diff --stat` igual ao de antes.

- [ ] **Step 6: Commit**

```bash
git add apps/player/Sources/HogPlayerKit/Quality.swift apps/player/Sources/HogPlayerKit/DisplayState.swift apps/player/Sources/HogPlayerKit/QueueEntry.swift apps/player/Tests/HogPlayerKitTests/QueueEntryTests.swift apps/player/Tests/HogPlayerKitTests/DisplayTests.swift apps/player/Tests/HogPlayerKitTests/ViewModelTests.swift
git commit -m "feat(player): mostrar codec e bitrate da faixa com perda"
```

---

### Task 5: Protótipo GPUI acompanha a API

O `apps/gpui` usa a API pública do core pelo path `../../rust`, e sem isto deixa de compilar. A
regra de exibição é a mesma do Swift.

O gate `apps/gpui/tools/check_core_untouched.sh` (primeiro passo do `make gpui-test`) reprova
de propósito enquanto `rust/` difere da `main`. Na branch, rode os três comandos do cargo
direto. O `make gpui-test` completo passa depois do merge na `main`, que é decisão do usuário.

**Files:**
- Modify: `apps/gpui/model/src/port.rs:99-114` (`Format`)
- Modify: `apps/gpui/model/src/format.rs:30-39`
- Modify: `apps/gpui/model/src/entry.rs:33-87`
- Modify: `apps/gpui/model/src/display.rs:43-47`
- Modify: `apps/gpui/testing/src/lib.rs:206-215`
- Test: `apps/gpui/model/tests/format.rs`, `tests/entry.rs`, `tests/display.rs`

**Interfaces:**
- Consumes: `hog_audio::api::TrackFormat` e `TrackProbe` com `bit_depth: Option<u32>` e
  `bitrate_kbps: Option<u32>` (Task 2).
- Produces:
  - `quality_detail(sample_rate: f64, bit_depth: Option<u32>, bitrate_kbps: Option<u32>, codec: &str) -> String`;
  - `compact_quality(sample_rate: f64, bit_depth: Option<u32>, bitrate_kbps: Option<u32>, codec: &str) -> String`;
  - `port::Format { sample_rate, bit_depth: Option<u32>, bitrate_kbps: Option<u32>, codec }`;
  - `ProbeOutcome::Probed { sample_rate, bit_depth: Option<u32>, bitrate_kbps: Option<u32>, codec, total_seconds }`.

- [ ] **Step 1: Escrever os testes**

`apps/gpui/model/tests/format.rs`: trocar os dois testes de qualidade por:

```rust
#[test]
fn selo_compacto_usa_bits_barra_kilohertz() {
    assert_eq!(compact_quality(44_100.0, Some(24), None, "flac"), "24/44.1");
    assert_eq!(compact_quality(96_000.0, Some(24), None, "flac"), "24/96");
    assert_eq!(compact_quality(192_000.0, Some(24), None, "flac"), "24/192");
    assert_eq!(compact_quality(44_100.0, Some(16), None, "flac"), "16/44.1");
}

#[test]
fn detalhe_de_qualidade_mostra_a_casa_decimal_so_quando_existe() {
    assert_eq!(
        quality_detail(44_100.0, Some(24), None, "flac"),
        "44.1 kHz · 24 bit · flac"
    );
    assert_eq!(
        quality_detail(96_000.0, Some(24), None, "flac"),
        "96 kHz · 24 bit · flac"
    );
}

#[test]
fn faixa_com_perda_mostra_codec_e_bitrate_no_lugar_dos_bits() {
    assert_eq!(compact_quality(48_000.0, None, Some(339), "aac"), "AAC 339");
    assert_eq!(
        quality_detail(48_000.0, None, Some(339), "aac"),
        "48 kHz · AAC 339 kbps"
    );
}

#[test]
fn faixa_com_perda_sem_bitrate_mostra_so_o_codec() {
    assert_eq!(compact_quality(44_100.0, None, None, "mp3"), "MP3");
    assert_eq!(quality_detail(44_100.0, None, None, "mp3"), "44.1 kHz · MP3");
}
```

`apps/gpui/model/tests/entry.rs`: trocar a `sonda()` por:

```rust
fn sonda() -> ProbeOutcome {
    ProbeOutcome::Probed {
        sample_rate: 44_100.0,
        bit_depth: Some(24),
        bitrate_kbps: None,
        codec: "flac".to_string(),
        total_seconds: 149.6,
    }
}
```

E acrescentar depois de `carga_real_usa_a_mesma_sondagem_do_engine`:

```rust
#[test]
fn carga_real_de_aac_mostra_codec_e_bitrate() {
    let path = fixture("t44_aac.m4a");
    // O bitrate do AAC é VBR e depende do encoder: o esperado vem da mesma sondagem do core.
    let kbps = hog_audio::api::probe_track(path.clone())
        .expect("deveria sondar o AAC")
        .bitrate_kbps
        .expect("o AAC deveria ter bitrate");
    let info = load_queue_entry(&path);
    assert_eq!(info.problem, None);
    assert_eq!(info.quality, Some(format!("AAC {kbps}")));
    assert_eq!(info.quality_detail, Some(format!("44.1 kHz · AAC {kbps} kbps")));
}
```

`apps/gpui/model/tests/display.rs`: na `faixa()` e no `cd`, trocar `bit_depth: 24,` e
`bit_depth: 16,` por `bit_depth: Some(24), bitrate_kbps: None,` e
`bit_depth: Some(16), bitrate_kbps: None,`. E acrescentar no fim:

```rust
#[test]
fn a_linha_tecnica_da_faixa_com_perda_mostra_codec_e_bitrate() {
    let aac = Format {
        sample_rate: 48_000.0,
        bit_depth: None,
        bitrate_kbps: Some(339),
        codec: "aac".to_string(),
    };
    let d = display_state(
        &snapshot(PlayerState::Playing, 0.0, 0, Some(0), 1),
        Some(&tags()),
        Some(&aac),
    );
    assert_eq!(d.technical_line, "48 kHz · AAC 339 kbps · hog ativo");
}
```

- [ ] **Step 2: Rodar para ver falhar**

Run: `(cd apps/gpui && cargo test --locked --workspace)`
Esperado: erro de compilação: `bit_depth` com tipo trocado em `port.rs`, `entry.rs` e
`testing/src/lib.rs`, e as funções de qualidade com aridade antiga.

- [ ] **Step 3: Implementar**

`apps/gpui/model/src/port.rs`, o `Format` e o `From`:

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct Format {
    pub sample_rate: f64,
    /// `None` na faixa com perda.
    pub bit_depth: Option<u32>,
    pub bitrate_kbps: Option<u32>,
    pub codec: String,
}

impl From<&TrackFormat> for Format {
    fn from(format: &TrackFormat) -> Self {
        Self {
            sample_rate: format.sample_rate,
            bit_depth: format.bit_depth,
            bitrate_kbps: format.bitrate_kbps,
            codec: format.codec.clone(),
        }
    }
}
```

`apps/gpui/model/src/format.rs`, trocar `quality_detail` e `compact_quality` por:

```rust
/// "44.1 kHz · 24 bit · flac" ou, na faixa com perda, "48 kHz · AAC 339 kbps".
pub fn quality_detail(
    sample_rate: f64,
    bit_depth: Option<u32>,
    bitrate_kbps: Option<u32>,
    codec: &str,
) -> String {
    let rate = format!("{} kHz", format_kilohertz(sample_rate));
    match (bit_depth, bitrate_kbps) {
        (Some(bits), _) => format!("{rate} · {bits} bit · {codec}"),
        (None, Some(kbps)) => format!("{rate} · {} {kbps} kbps", codec.to_uppercase()),
        (None, None) => format!("{rate} · {}", codec.to_uppercase()),
    }
}

/// "24/44.1" ou, na faixa com perda, que não tem bits a mostrar, "AAC 339".
pub fn compact_quality(
    sample_rate: f64,
    bit_depth: Option<u32>,
    bitrate_kbps: Option<u32>,
    codec: &str,
) -> String {
    match (bit_depth, bitrate_kbps) {
        (Some(bits), _) => format!("{bits}/{}", format_kilohertz(sample_rate)),
        (None, Some(kbps)) => format!("{} {kbps}", codec.to_uppercase()),
        (None, None) => codec.to_uppercase(),
    }
}
```

`apps/gpui/model/src/entry.rs`: no `ProbeOutcome::Probed`, trocar `bit_depth: u32,` por
`bit_depth: Option<u32>,` e acrescentar `bitrate_kbps: Option<u32>,` logo abaixo. No
`queue_entry_info`, o braço passa a ser:

```rust
        ProbeOutcome::Probed {
            sample_rate,
            bit_depth,
            bitrate_kbps,
            codec,
            total_seconds,
        } => (
            Some(compact_quality(sample_rate, bit_depth, bitrate_kbps, &codec)),
            Some(quality_detail(sample_rate, bit_depth, bitrate_kbps, &codec)),
            Some(format_time(total_seconds)),
            None,
        ),
```

E no `load_queue_entry`, depois de `bit_depth: probe.bit_depth,`:

```rust
            bitrate_kbps: probe.bitrate_kbps,
```

`apps/gpui/model/src/display.rs`, a chamada:

```rust
            let mut parts = vec![quality_detail(
                format.sample_rate,
                format.bit_depth,
                format.bitrate_kbps,
                &format.codec,
            )];
```

`apps/gpui/testing/src/lib.rs`, no `track()`, trocar `bit_depth: 16,` por:

```rust
        bit_depth: Some(16),
        bitrate_kbps: None,
```

- [ ] **Step 4: Rodar fmt, clippy e testes do protótipo**

Run: `(cd apps/gpui && cargo fmt --all --check && cargo clippy --locked --workspace --all-targets -- -D warnings && cargo test --locked --workspace)`
Esperado: tudo verde (eram 178 testes, passam a ser 182). Se o `fmt --check` reprovar, rode
`cargo fmt --all` e repita. Não rode o `make gpui-test` aqui (ver o início da task).

- [ ] **Step 5: Provar o vermelho**

Copie `apps/gpui/model/src/format.rs` para o scratchpad. Sabotagem: nos dois `match`, trocar
`(Some(bits), _)` por `(bits, _)` e usar `bits.unwrap_or(0)` no `format!`, o que faz a faixa com
perda mostrar "0 bit". Rode `(cd apps/gpui && cargo test --locked --workspace)`.
Esperado vermelho: `faixa_com_perda_mostra_codec_e_bitrate_no_lugar_dos_bits`,
`faixa_com_perda_sem_bitrate_mostra_so_o_codec`, `carga_real_de_aac_mostra_codec_e_bitrate` e
`a_linha_tecnica_da_faixa_com_perda_mostra_codec_e_bitrate`. Restaure por `cp` e confira o
verde.

- [ ] **Step 6: Commit**

```bash
git add apps/gpui/model/src/port.rs apps/gpui/model/src/format.rs apps/gpui/model/src/entry.rs apps/gpui/model/src/display.rs apps/gpui/testing/src/lib.rs apps/gpui/model/tests/format.rs apps/gpui/model/tests/entry.rs apps/gpui/model/tests/display.rs
git commit -m "feat(gpui): acompanhar a API de faixa com perda do core"
```

---

### Task 6: A prova da faixa com perda e as provas de sempre

Esta task toma o device. **Antes do Step 2, pare e peça ao usuário:**
- o "ok" para tomar o device;
- o volume em ~10%, porque o `make hw-test` toca tons;
- o app instalado (`/Applications/HogAudio.app`) fechado.

**Files:**
- Modify: `Makefile` (alvo `verify-lossy` e o `.PHONY`)

**Interfaces:**
- Consumes: o binário `rust/target/release/hog-audio` com as Tasks 2 e 3; as fixtures da Task 1.
- Produces: `make verify-lossy FILE=<arquivo>`, que imprime
  `com perda: o dump é idêntico ao que o decodificador da Apple entrega` ou reprova.

- [ ] **Step 1: Acrescentar o alvo ao `Makefile`**

No `.PHONY` da linha 9, acrescentar `verify-lossy` depois de `verify-volume`. Depois do alvo
`verify-volume`, inserir o bloco abaixo. As linhas de receita começam com TAB, não com espaços:

```make
# make verify-lossy FILE=testdata/t44_aac.m4a
# Na faixa com perda o ffmpeg não serve de oráculo: o decodificador de AAC e MP3 dele difere do
# da Apple nos últimos bits. O afconvert usa o mesmo decodificador da Apple por outra ferramenta,
# então bater byte a byte prova que o app não acrescentou nada depois do decodificador. O ffmpeg
# só tira o cabeçalho do WAV, sem converter (pcm_f32le para f32le).
verify-lossy: rust
	@test -n "$(FILE)" || { echo "uso: make verify-lossy FILE=arquivo.m4a"; exit 2; }
	@afconvert -f WAVE -d LEF32 "$(FILE)" /tmp/hog_lossy_ref.wav
	@ffmpeg -v error -y -i /tmp/hog_lossy_ref.wav -c:a copy -f f32le /tmp/hog_lossy_ref.raw
	@./$(RUST_BIN) --dump /tmp/hog_lossy.raw "$(FILE)" >/dev/null
	@test -s /tmp/hog_lossy.raw || { echo "FALHOU: dump vazio"; exit 1; }
	@cmp /tmp/hog_lossy_ref.raw /tmp/hog_lossy.raw \
	  && echo "com perda: o dump é idêntico ao que o decodificador da Apple entrega"
```

- [ ] **Step 2: Rodar a prova nova (toma o device, sem som)**

Run: `make verify-lossy FILE=testdata/t44_aac.m4a && make verify-lossy FILE=testdata/t44_mp3.mp3`
Esperado: a mensagem de "idêntico" duas vezes.

**Se o `cmp` divergir:** pare, sem adaptar a prova. Rode
`cmp -l /tmp/hog_lossy_ref.raw /tmp/hog_lossy.raw | head` e
`ls -l /tmp/hog_lossy_ref.raw /tmp/hog_lossy.raw`, e leve ao usuário a primeira divergência e os
tamanhos. A spec prevê a alternativa: comparar contra uma decodificação direta do
`ExtAudioFile` num teste, uma prova mais fraca. Ela vira uma task nova, só com o "ok" dele.

- [ ] **Step 2b: Provar que a comparação pega o app mexendo nas amostras (toma o device, sem som)**

A sabotagem reproduz o sintoma, sem tocar no código: o modo afinado altera cada amostra
depois do decodificador. Gere uma curva mínima no scratchpad (com Write, duas linhas:
`20 3` e `20000 -3`) e rode:

`./rust/target/release/hog-audio --dump /tmp/hog_lossy_afinado.raw --tuning $SCRATCH/curva.txt testdata/t44_aac.m4a >/dev/null && cmp -s /tmp/hog_lossy_ref.raw /tmp/hog_lossy_afinado.raw; echo "cmp: $?"`

Esperado: `cmp: 1`. Os bytes diferem, e isso prova que a comparação do `verify-lossy` reprova
quando o app altera o PCM.

- [ ] **Step 3: Rodar as provas do caminho sem perda (tomam o device, sem som)**

Run: `make verify FILE=testdata/t96_24.flac BITS=24 && make verify FILE=testdata/t44_16.flac BITS=16 && make verify-pause FILE=testdata/t96_24.flac && make verify-volume FILE=testdata/t96_24.flac`

Esperado: `BIT-PERFECT — todas as amostras batem exatamente` duas vezes,
`pause: fluxo idêntico com e sem pausa` e `volume: amostras idênticas a 20% e 90%`.

- [ ] **Step 4: Conferir a recusa por rate de um MP3 em 22,05 kHz (sem tomar o device)**

Run: `ffmpeg -v error -y -f lavfi -i "sine=frequency=440:duration=2:sample_rate=22050" -ac 2 -c:a libmp3lame -b:a 64k /tmp/hog_t22.mp3 && ./rust/target/release/hog-audio --info /tmp/hog_t22.mp3; echo "saída: $?"`

Esperado: `erro: arquivo em 22050 Hz; o device suporta: 44100, 48000, 88200, 96000. Reproduzir
exigiria resample.` e `saída: 1`.

- [ ] **Step 5: Rodar os testes de hardware (tomam o device e tocam tons; volume em ~10%)**

Run: `make hw-test`
Esperado: tudo verde, incluindo `load_de_aac_negocia_float32_no_rate_do_arquivo`.

- [ ] **Step 6: Conferir que o caminho do áudio não mudou**

Run: `git diff main --stat -- rust/src/device.rs rust/src/playback.rs rust/src/ring.rs rust/src/recovery.rs rust/src/device_watch.rs`
Esperado: saída vazia.

- [ ] **Step 7: Commit**

```bash
git add Makefile
git commit -m "test(verify): provar que o app não altera o PCM com perda"
```

---

### Task 7: Documentação e escuta real

**Files:**
- Modify: `README.md` (seção "O que ele faz", "Por que float32 continua sendo bit-perfect",
  "Limites conhecidos" e "As três provas de bit-perfect")
- Modify: `docs/ROADMAP.md` (seção 0.1)
- Modify: `docs/README.md` (linha "Áudio com perda" da tabela de trabalhos)

- [ ] **Step 1: README, itens 1 e 2 de "O que ele faz"**

Trocar:

```markdown
1. Lê o formato real do arquivo com o decodificador do próprio macOS — FLAC, ALAC, WAV,
   AIFF, CAF, AAC e MP3, sem biblioteca externa.
2. Enumera o que o device de saída aceita e decide se existe caminho sem perda. **Se não
   existir, recusa tocar** em vez de resamplear em silêncio.
```

por:

```markdown
1. Lê o formato real do arquivo com o decodificador do próprio macOS — FLAC, ALAC, WAV,
   AIFF, CAF, AAC-LC e MP3, sem biblioteca externa.
2. Enumera o que o device de saída aceita e decide se existe caminho em que o app não altere
   nenhuma amostra do que o decodificador entrega. **Se não existir, recusa tocar** em vez de
   resamplear ou arredondar em silêncio. Num arquivo sem perda isso é bit-perfect; num AAC ou
   MP3, a única perda é a da compressão
   ([ADR 0002 do core](rust/docs/adr/0002-arquivo-com-perda-sem-perda-do-app.md)).
```

- [ ] **Step 2: README, fim da seção "Por que float32 continua sendo bit-perfect"**

Depois do parágrafo "Para arquivos acima de 24 bits o programa recusa o formato float32...",
acrescentar:

```markdown
AAC e MP3 não têm profundidade de bits: o decodificador da Apple entrega float32, e é esse
float32 que vai ao device, sem conversão. Por isso a faixa com perda só toca quando o device tem
um formato físico float32 no rate do arquivo; arredondá-la para inteiro seria perda acrescentada
pelo app. A tela mostra codec e bitrate ("AAC 256") no lugar dos bits.
```

- [ ] **Step 3: README, "Limites conhecidos"**

Depois do item "**Sem resample, por escolha**", acrescentar:

```markdown
- **Com perda, só AAC-LC e MP3**: HE-AAC, Opus e os outros codecs com perda são recusados, com
  o nome do codec na mensagem. AAC e MP3 em rate que o device não suporta (22,05 ou 32 kHz no
  fone do Mac) também, pela mesma regra de não resamplear.
```

- [ ] **Step 4: README, a prova da faixa com perda**

No fim da seção "As três provas de bit-perfect" (depois do parágrafo que termina em "...sem
device, mas exercitando o callback de verdade."), acrescentar:

````markdown
A faixa com perda tem uma prova própria:

```
make verify-lossy FILE=testdata/t44_aac.m4a
```

Ela compara o dump byte a byte com o mesmo arquivo decodificado pelo `afconvert` em float32. O
ffmpeg não serve de oráculo aqui: o decodificador de AAC e MP3 dele difere do da Apple nos
últimos bits. Como o `afconvert` usa o mesmo decodificador da Apple, a prova garante que o app
não acrescenta nada depois do decodificador, e não que o decodificador esteja certo.
````

- [ ] **Step 5: ROADMAP e índice dos docs**

`docs/ROADMAP.md`, seção 0.1: depois do último item da lista "Decidido em 11/10/2026" (o que
começa com "**O README é corrigido junto com o código:**") e antes da linha
"Para depois, com data de 11/10/2026:", inserir este parágrafo, com a data do dia
(`date +%d/%m/%Y`) no lugar de `DD/MM/AAAA`:

```markdown
Implementado em DD/MM/AAAA, na branch `feat/audio-com-perda`
(plano em `docs/superpowers/plans/2026-10-11-audio-com-perda.md`).
```

`docs/README.md`, na linha "Áudio com perda: AAC e MP3 (11/10)", trocar `— (a escrever)` por
`[plano](superpowers/plans/2026-10-11-audio-com-perda.md)`.

- [ ] **Step 6: Commit da documentação**

```bash
git add README.md docs/ROADMAP.md docs/README.md
git commit -m "docs(readme): descrever AAC e MP3 e a prova da faixa com perda"
```

- [ ] **Step 7: Escuta real, com o usuário (toma o device e toca)**

**Pare e peça ao usuário** o "ok" e o volume em ~10%. Depois toque, pela CLI, uma faixa AAC da
biblioteca, com volume explícito e baixo:

```bash
./rust/target/release/hog-audio --volume 10 "$HOME/Library/CloudStorage/GoogleDrive-renattoadorno@gmail.com/Meu Drive/midia/musicas/Alex Warren - On My Mind (2025) [24B-48kHz]/01. On My Mind.m4a"
```

Esperado na saída: `fonte    : aac 48000 Hz / 339 kbps / 2 canais`, `negociado: 48000 Hz / 32
bits float` e `exclusivo: sim`. O usuário confirma que o som sai limpo. Ctrl+C encerra, e o device
volta ao estado anterior (`fim      : device restaurado`).

Depois, no app: `make app && open apps/player/HogAudio.app`, abrir a mesma faixa e conferir o selo
"AAC 339" na fila e "48 kHz · AAC 339 kbps · hog ativo" na linha técnica. Instalar em
`/Applications` (`make install-app`) só com pedido do usuário.

- [ ] **Step 8: Fechar a branch**

Use a skill `superpowers:finishing-a-development-branch`. O merge na `main` e o push são decisões
do usuário. Depois do merge, rode o `make gpui-test` completo na `main`: o gate do core volta a
passar quando `rust/` não difere mais da `main`.
