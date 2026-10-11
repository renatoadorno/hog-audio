//! A conta de tamanho da janela, porte de `PlayerLayout.swift`: capa quadrada à esquerda,
//! controles de altura fixa abaixo dela e a fila com o que sobrar.

pub const PADDING: f32 = 24.0;
pub const COLUMN_SPACING: f32 = 24.0;
/// A altura medida na janela SwiftUI (261 pt com artista, álbum e linha técnica) mais 3 de
/// folga. A coluna GPUI é montada para caber nela; a Task 9 confere.
pub const CONTROLS_HEIGHT: f32 = 264.0;
pub const MIN_QUEUE_WIDTH: f32 = 340.0;
pub const COVER_MIN: f32 = 320.0;
pub const COVER_MAX: f32 = 560.0;
pub const MIN_WINDOW: (f32, f32) = (820.0, 640.0);
pub const DEFAULT_WINDOW: (f32, f32) = (920.0, 700.0);

/// Lado da capa: cresce com a altura sem roubar a largura mínima da fila.
pub fn cover_side(width: f32, height: f32) -> f32 {
    let by_height = height - 2.0 * PADDING - CONTROLS_HEIGHT;
    let by_width = width - 2.0 * PADDING - COLUMN_SPACING - MIN_QUEUE_WIDTH;
    by_height.min(by_width).clamp(COVER_MIN, COVER_MAX)
}
