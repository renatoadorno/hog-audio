//! Título, artista, álbum e capa pelo lofty. No SwiftUI isso vem do AVFoundation; o core não lê
//! tags. O lofty só lê metadados: decodificar áudio continua com o `ExtAudioFile` do core.

use std::borrow::Cow;
use std::path::Path;

use lofty::config::ParseOptions;
use lofty::picture::PictureType;
use lofty::prelude::{Accessor, TaggedFileExt};
use lofty::probe::Probe;

#[derive(Clone, Debug, PartialEq)]
pub struct Tags {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Os bytes da capa como vieram do arquivo (JPEG ou PNG), sem decodificar.
    pub artwork: Option<Vec<u8>>,
}

impl Tags {
    fn only_title(title: String) -> Self {
        Self {
            title,
            artist: None,
            album: None,
            artwork: None,
        }
    }
}

/// Nunca falha: sem tags legíveis, o título vira o nome do arquivo, como no SwiftUI.
pub fn read_tags(path: &str) -> Tags {
    let fallback = file_stem(path);
    // Sem as propriedades de áudio: aqui só se leem metadados (quem decodifica é o core), e o
    // AVFoundation também lê as tags de um MP4 sem trilha de áudio.
    let options = ParseOptions::new().read_properties(false);
    let Ok(tagged) = Probe::open(path).and_then(|probe| probe.options(options).read()) else {
        return Tags::only_title(fallback);
    };
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Tags::only_title(fallback);
    };
    let pictures = tag.pictures();
    let artwork = pictures
        .iter()
        .find(|picture| picture.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())
        .map(|picture| picture.data().to_vec());
    Tags {
        title: clean_text(tag.title()).unwrap_or(fallback),
        artist: clean_text(tag.artist()),
        album: clean_text(tag.album()),
        artwork,
    }
}

/// Tag vazia ou só com espaços conta como ausente.
pub fn clean_text(value: Option<Cow<'_, str>>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

pub fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}
