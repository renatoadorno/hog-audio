//! Afinação: o segundo modo de reprodução, em que o sinal passa por um filtro com a resposta
//! em frequência que a curva descreve.
//!
//! O modo bit-perfect não passa por aqui — nada neste módulo é alcançável quando a afinação
//! está desligada, e é essa separação que mantém o caminho digital curto intacto.

pub mod curve;
pub mod design;
pub mod fft;
pub mod process;
pub mod quantize;
