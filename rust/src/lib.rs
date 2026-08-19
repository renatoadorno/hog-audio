//! Núcleo do hog-audio: decodificação, negociação de formato, ring buffer e controle do
//! device em modo exclusivo. Tanto a CLI quanto a interface gráfica consomem esta
//! biblioteca; nenhuma das duas fala com o Core Audio diretamente.

uniffi::setup_scaffolding!();

pub mod api;
pub mod device;
pub mod engine;
pub mod ffi;
pub mod format;
pub mod playback;
pub mod ring;
pub mod source;
pub mod status;
pub mod transitions;
pub mod volume;
