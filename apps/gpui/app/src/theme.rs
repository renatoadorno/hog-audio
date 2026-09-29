//! Tokens de cor, espaço e tipo. As cores seguem as do SwiftUI (`Color.hogAccent` e as cores
//! semânticas do sistema), para a comparação medir a tecnologia e não um visual diferente.

use gpui::{Rgba, Styled, WindowAppearance, px, rgb, rgba};

pub const FONT: &str = ".SystemUIFont";
pub const MONO_FONT: &str = "Menlo";

/// Um estilo de texto do macOS, em pt, com a altura de linha dele. Sem altura explícita o GPUI
/// usa 1,618 × a fonte, e o texto não cabe nos slots medidos no SwiftUI.
#[derive(Clone, Copy)]
pub struct TypeStyle {
    pub size: f32,
    pub line_height: f32,
}

impl TypeStyle {
    pub fn apply<E: Styled>(self, element: E) -> E {
        element
            .text_size(px(self.size))
            .line_height(px(self.line_height))
    }
}

// Os tamanhos são os do macOS, não os do iOS: foi com eles que o SwiftUI mediu os 264 de
// `layout::CONTROLS_HEIGHT`.
pub const TITLE2: TypeStyle = TypeStyle {
    size: 17.0,
    line_height: 22.0,
};
pub const HEADLINE: TypeStyle = TypeStyle {
    size: 13.0,
    line_height: 16.0,
};
pub const SUBHEADLINE: TypeStyle = TypeStyle {
    size: 11.0,
    line_height: 14.0,
};
pub const CAPTION: TypeStyle = TypeStyle {
    size: 10.0,
    line_height: 13.0,
};
pub const CAPTION2: TypeStyle = TypeStyle {
    size: 10.0,
    line_height: 13.0,
};

/// A faixa de cima, sob a barra de título transparente, onde fica o botão Abrir (a toolbar do
/// SwiftUI).
pub const TITLEBAR_HEIGHT: f32 = 40.0;

// O bloco abaixo da capa soma `layout::CONTROLS_HEIGHT` (264): 16 + 64 + 16 + 32 + 16 + 56 +
// 16 + 24 + 8 + 16.
pub const GAP: f32 = 16.0;
pub const TITLE_BLOCK: f32 = 64.0;
pub const CLOCK_HEIGHT: f32 = 32.0;
pub const TRANSPORT_HEIGHT: f32 = 56.0;
pub const VOLUME_HEIGHT: f32 = 24.0;
pub const TECHNICAL_GAP: f32 = 8.0;
pub const TECHNICAL_HEIGHT: f32 = 16.0;

pub const STATUS_HEIGHT: f32 = 40.0;
pub const STATUS_GAP: f32 = 12.0;
pub const ROW_HEIGHT: f32 = 56.0;
pub const THUMB: f32 = 44.0;

#[derive(Clone, Copy)]
pub struct Palette {
    pub background: Rgba,
    pub foreground: Rgba,
    pub secondary: Rgba,
    pub tertiary: Rgba,
    pub fill: Rgba,
    pub panel: Rgba,
    pub veil: Rgba,
    pub accent: Rgba,
    pub accent_soft: Rgba,
    pub on_accent: Rgba,
    pub error: Rgba,
    pub error_soft: Rgba,
    pub warning: Rgba,
}

impl Palette {
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
        }
    }

    fn dark() -> Self {
        Self {
            background: rgb(0x1e1e1e),
            foreground: rgb(0xffffff),
            secondary: rgba(0xffffff8c),
            tertiary: rgba(0xffffff59),
            fill: rgba(0xffffff14),
            panel: rgba(0x2a2a2ab8),
            veil: rgba(0x1e1e1e99),
            accent: rgb(0xfa6112),
            accent_soft: rgba(0xfa61121f),
            on_accent: rgb(0xffffff),
            error: rgb(0xff453a),
            error_soft: rgba(0xff453a17),
            warning: rgb(0xff9f0a),
        }
    }

    fn light() -> Self {
        Self {
            background: rgb(0xececec),
            foreground: rgb(0x000000),
            secondary: rgba(0x0000008c),
            tertiary: rgba(0x00000059),
            fill: rgba(0x00000014),
            panel: rgba(0xf6f6f6b8),
            veil: rgba(0xecececa0),
            accent: rgb(0xfa6112),
            accent_soft: rgba(0xfa61121f),
            on_accent: rgb(0xffffff),
            error: rgb(0xff3b30),
            error_soft: rgba(0xff3b3017),
            warning: rgb(0xff9500),
        }
    }
}
