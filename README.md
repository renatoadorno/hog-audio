<p align="center">
  <img src="logos/hog-icon-dark.svg" width="128" alt="">
</p>

<h1 align="center">hog-audio</h1>

<p align="center">
  A hi-res music player, exclusively for macOS.<br>
  Bit-perfect: your FLAC and ALAC files play exactly as they are stored, at their original
  resolution, with the DAC dedicated to your music alone.
</p>

<p align="center">
  <strong>v0.1.0</strong> · macOS 14 or later · Apple Silicon · MIT license
</p>

<p align="center">
  <strong>English</strong> · <a href="README.pt-BR.md">Português</a>
</p>

<p align="center">
  <img src="docs/images/player.png" width="860"
       alt="The hog-audio window with the album Sugar, by Robin Schulz, in 24-bit / 96 kHz FLAC: the cover and transport controls on the left, the 15-track queue on the right">
</p>

> The app's interface and the command-line output are in Brazilian Portuguese.

## What it does

- **Plays in exclusive mode.** Takes over the output device in hog mode
  (`kAudioDevicePropertyHogMode`): while it plays, no other app can use the converter.
- **Locks the DAC to the file's sample rate.** The converter physically switches to the track's
  frequency. If the device does not support that rate, the player refuses to play instead of
  silently resampling.
- **Bypasses the system mixer.** It feeds an `AudioDeviceIOProc` directly with the bytes the
  decoder produced.
- **Sets the volume in hardware.** It uses the amplifier's own control, the same one as the system
  volume, and never changes a sample. A safety ceiling protects your ears and whatever is
  connected to the output, be it headphones, speakers or an external DAC.
- **Plays AAC and MP3 without adding loss.** The audio Apple's decoder delivers, as float32,
  reaches the device untouched; the only loss is the one from compression
  ([core ADR 0002](rust/docs/adr/0002-arquivo-com-perda-sem-perda-do-app.md), in Portuguese).
- **Keeps a queue.** Each track shows its cover, title, artist, album, duration and a quality
  badge ("24/96", "16/44.1", "AAC 256"). When a track ends, the next one starts.
- **Gives the device back as it found it.** When it finishes, it restores the sample rate, format
  and volume and releases exclusive mode, even when interrupted with Ctrl+C.

The screen always shows what reaches the DAC. The technical line reads "96 kHz · 24 bit · flac ·
hog ativo" (hog active) for a FLAC and "48 kHz · AAC 339 kbps · hog ativo" for a lossy track.

## Why "hog"

hog-audio plays through macOS's own audio system, Core Audio, with no third-party driver or
library. Sound goes out through the Mac's converter, which drives the headphone jack and the
speakers, or through any external DAC that macOS recognizes. So far it has been tested on the
built-in output of a MacBook Air M1.

And it plays exclusively. The name comes from *hog mode* (`kAudioDevicePropertyHogMode`), Core
Audio's exclusive mode: to hog something is to keep it all to yourself. That is what the player
does with the DAC while it plays, so no other app can interfere with the sound that reaches it.

## Formats

- **Bit-perfect:** FLAC, ALAC, WAV, AIFF and CAF up to 24 bits, at any sample rate the device
  accepts.
- **Lossy, with only the compression loss:** AAC-LC and MP3, when the device accepts float32 at
  the file's sample rate, as the Mac's built-in outputs do.
- **Refused, with the reason shown on screen:**
  - a sample rate the device does not support, such as a 192 kHz FLAC or a 22.05 kHz MP3 on the
    Mac's headphone output;
  - HE-AAC, Opus and other lossy codecs;
  - 32-bit integer or 64-bit float files on a float32 device, which cannot play them without
    loss;
  - files with more channels than the device.

## Install

There is no prebuilt download yet: the app is built from source.

Requirements:
- macOS 14 or later, tested on a MacBook Air M1;
- Rust 1.88 or later, tested with 1.98;
- the Swift 6 toolchain (Xcode or the Command Line Tools);
- ffmpeg, only for the tests and the proofs (`brew install ffmpeg`).

```
make install-app
```

This builds the app, installs it in `/Applications` (closing a running copy first) and opens it.
`make install-app INSTALL_DIR="$HOME/Applications"` installs it for the current user only, and
`OPEN_APP=0` skips opening it at the end.

The app is not sandboxed, on purpose: hog mode does not work inside the sandbox. That rules out
the App Store, which is not a goal of this project.

## Usage

- **Adding music:** ⌘O (files or folders, including subfolders), dragging from the Finder onto the
  window, or "Open With → HogAudio" in the Finder.
- **Opening with a double click:** in the Finder, use "Get Info" on a file → "Open with" →
  HogAudio → "Change All…", once for each file type.
- **Queue:** music is added to the end of the queue. If the queue was empty, the first track is
  loaded and ready to play. Delete removes the selected track, and "Limpar" (Clear) empties the
  queue.

While the player holds the device, **other apps cannot play through it**, even when the music is
paused. If there is another output, macOS sends their sound there: on a MacBook with headphones
plugged in, for example, other apps play through the speakers. Pausing keeps the device reserved,
so playback resumes instantly without another app taking it. Quitting the app puts everything
back.

## Command line

The same core also runs in the terminal, without a window:

```
make
./rust/target/release/hog-audio musicas/faixa.flac
./rust/target/release/hog-audio --info musicas/faixa.flac
```

`--info` shows what would be negotiated, without taking the device:

```
$ ./rust/target/release/hog-audio --info "02. Sugar (feat. Francesco Yates).flac"
fonte    : flac 96000 Hz / 24 bits / 2 canais / 219.1 s
device   : Alto-falantes (MacBook Air)
rate atual: 44100 Hz
rates    : 44100, 48000, 88200, 96000
formatos : 4 físicos disponíveis
           96000 Hz / 32 bits float / 2 canais
           88200 Hz / 32 bits float / 2 canais
           48000 Hz / 32 bits float / 2 canais
           44100 Hz / 32 bits float / 2 canais
negociado: 96000 Hz / 32 bits float
afinação : desligada — saída bit-perfect
```

Other options:
- `--volume 35` or `--volume -18dB`: the playback volume, as a percentage or as an exact
  attenuation;
- `--max-volume 60`: changes the safety ceiling, which defaults to 50%;
- `--tuning curve.txt`: turns on tuning (see below);
- `--dump file.raw`: writes the bytes that would go to the DAC to a file, without playing.

## Volume and safety

Full volume can hurt your ears and damage whatever is connected to the output, be it headphones,
speakers or an amplifier; the Mac's headphone output, for example, delivers enough voltage to do
so. That is why the volume is set **after the player takes the device and before any sound comes
out**, and restored to its previous value at the end. Without `--volume`, it is only changed when
it is above the ceiling.

The decibels come from the amplifier itself, not from a formula: asking for `-30dB` gives exactly
−30.0 dB on the device. The value shown is always read back from the hardware, because the
conversion the HAL reports does not match the curve it actually applies.

The code works around two Core Audio pitfalls:
- **The first volume change after a format reconfiguration is silently dropped.** The HAL returns
  `noErr`, and the next read returns the requested value, served from a cache. The player checks
  again with a delayed read; if an explicitly requested volume cannot be confirmed, it aborts
  playback instead of playing at the wrong volume.
- **While the device is held, macOS points the "default output" to another device.** Anything
  that reads the default output during playback, including `system_profiler`, sees the volume of
  the wrong device. That is why the player never relies on the default output while it holds a
  device.

## Tuning (`--tuning`)

A second playback mode that applies a frequency-response curve of your choice. **With tuning on,
the output is no longer bit-perfect**, because applying a filter means changing every sample. Hog
mode and the locked sample rate still apply.

The curve is a text file with one frequency and one gain per line. A CSV exported from a
measurement works as is, and `#` starts a comment:

```
# frequency (Hz)   gain (dB)
20      12.5
1000    -0.6
3200    10.0
20000  -10.6
```

- The filter is a 4096-tap minimum-phase FIR, designed from the curve's real cepstrum. The
  resulting response stays within 0.02 dB of the requested one across the whole audible band.
- The preamp is not optional: the whole curve is lowered until its peak touches 0 dBFS, because
  positive gain in the digital domain clips.
- On an integer-format device, the conversion back from float uses TPDF dither.
- The bit-perfect mode does not run a single line of the tuning code
  ([core ADR 0001](rust/docs/adr/0001-modo-afinado-ao-lado-do-bit-perfect.md), in Portuguese).

## How bit-perfect is proven

This is what sets the project apart from an ordinary player: any change to the audio path has to
keep passing these proofs.

```
make verify FILE=testdata/t96_24.flac BITS=24
make verify-pause FILE=testdata/t96_24.flac
make verify-volume FILE=testdata/t96_24.flac
make verify-lossy FILE=testdata/t44_aac.m4a
```

- **`verify`:** compares, sample by sample, the bytes the player would send to the DAC with the
  PCM that ffmpeg extracts from the same file. ffmpeg is the oracle, an independent
  implementation. It applies to stereo files at a rate the device supports.
- **`verify-pause`:** the stream with a pause in the middle comes out identical to the stream
  without one; no byte is lost or repeated.
- **`verify-volume`:** the recorded samples are identical with the volume at 20% and at 90%; the
  volume changes on the device, never in the signal.
- **`verify-lossy`:** for AAC and MP3, the player's output matches, byte for byte, the same file
  decoded by `afconvert`, which uses the same Apple decoder. It proves the app adds nothing after
  the decoder. It applies to stereo files.

Their limit: all of them run in `--dump` mode, which goes through the decoder, the ring buffer and
frame alignment, but does not run the real-time callback. That is why `io_proc` has its own tests,
in `rust/src/playback.rs`, which call it directly with a synthetic `AudioBufferList`.

The test audio files are not in the repository: `make fixtures` generates them with ffmpeg and
`afconvert`.

## Protection against noise

If the format the decoder produces is not exactly what the DAC expects, the result is not a mild
distortion: it is white noise at full volume. Three safeguards prevent it:
1. The frame size comes from the device itself, never from `bitsPerChannel / 8`, because 24-bit
   samples may sit in 32-bit containers.
2. After setting up delivery, the player reads back the format the decoder actually adopted and
   compares it with the device's. If they differ, it stops before playing.
3. In the callback, every byte sent is either file data or zero. A short buffer, an extra channel
   or a larger request than expected becomes silence, never uninitialized memory.

## Why float32 is still bit-perfect

The built-in outputs of Apple Silicon Macs only accept float32. That does not compromise
integrity: float32 has a 24-bit mantissa, so every integer up to 24 bits is represented exactly,
and the round trip returns the same value. Files above 24 bits are refused on a float32 device.
For AAC and MP3, the decoder already delivers float32, which goes to the device unconverted.

## If the process dies

Exclusive mode belongs to the process, and `coreaudiod` releases it when the process ends, even
with `kill -9`. The Mac never goes silent:
- **End of track, Ctrl+C, `SIGTERM`, `SIGHUP` or `SIGQUIT`:** the player releases exclusive mode
  and restores the sample rate.
- **`SIGKILL` or a crash:** the system releases exclusive mode, but the sample rate stays at the
  last one used. To fix it, play anything else or adjust it in Audio MIDI Setup.

## Known limitations

- **No resampling:** a track at a rate the device does not support is refused, by choice.
- **No gapless playback:** on every track change, the player releases and retakes the device, and
  may switch the DAC's sample rate.
- **Basic queue:** no seeking within a track, shuffle, repeat or reordering.
- **Output changes:** plugging in headphones mid-track does not move the sound to them; unplugging
  them stops playback and shows a warning.
- **32-bit float WAV, AIFF and CAF** files are wrongly refused on a float32 device (see the
  [ROADMAP](docs/ROADMAP.md), in Portuguese).
- **High-impedance headphones:** the output that raises the voltage for them exists on the
  MacBook Pro 14"/16" (2021) and later, the MacBook Air M2 and the Mac Studio. The MacBook Air M1
  does not have it.

What comes next is in the [ROADMAP](docs/ROADMAP.md) (in Portuguese).

## Structure

Two languages, each with one role
([ADR 0002](docs/adr/0002-consolidar-o-core-em-rust.md), in Portuguese):

```
rust/      the whole core: HAL, format, ring buffer, IOProc, decoding and the CLI
apps/      the SwiftUI app (apps/player), which talks to the core through uniffi, and a GPUI prototype
tools/     bit-perfect verifier, test file generator, icon and install scripts
logos/     the logo and icon as SVG
docs/      ROADMAP, ADRs and the record of past work
```

The core, by module:

```
rust/src/format.rs       decides the sample rate and format; pure code, no Core Audio
rust/src/volume.rs       interprets the requested volume and the ceiling; pure code
rust/src/transitions.rs  state machine rules; pure code
rust/src/ring.rs         lock-free queue between the decoder and the real-time thread
rust/src/source.rs       decoding through ExtAudioFile
rust/src/device.rs       HAL: exclusive mode, format lock, volume, IOProc, restore
rust/src/playback.rs     the state the IOProc reads and the real-time callback itself
rust/src/engine.rs       the layer that coordinates hardware, files and threads
rust/src/api.rs          the uniffi interface the Swift app sees, control only
rust/src/main.rs         the CLI
```

The decision "can this play without loss?" is kept apart from the hardware, in pure code: it is
the rule that matters most and the only one that can be checked without plugging in headphones.

## Development

In a fresh clone, generate the test files first; audio is not stored in the repository:

```
make fixtures    # generates testdata/ with ffmpeg and afconvert
make test        # fmt, clippy and the core and app tests
make hw-test     # the tests that take the real device, one at a time (they play tones)
make app         # builds apps/player/HogAudio.app without installing it
```

`make test` treats `clippy -D warnings` as a hard rule, not a suggestion: the `unsafe` code in the
HAL and the IOProc is where this project's most expensive bugs live. A test that depends on a
missing test file fails and says which command to run.

The IOProc runs on a real-time thread: inside it there is only memory copying and atomic
operations, never allocation, locking or printing. Anyone changing that path should start with
the comment at the top of `rust/src/playback.rs`.

Code comments, docs and commit messages are in Brazilian Portuguese.

## License

[MIT](LICENSE). Third-party code keeps its own license: `apps/gpui/vendor/gpui_macos`
(Apache-2.0, from Zed) and the prototype code adapted from Pawse (MIT, with the notice in the file
itself).
