import HogPlayerKit
import SwiftUI

/// Uma linha da fila: miniatura, título, "Artista — Álbum", selo de qualidade e duração.
/// Struct própria com entradas estáveis — nos ticks de 10 Hz em que nada dela muda, o SwiftUI
/// pula o body.
struct QueueRow: View {
    let entry: QueueEntryInfo?
    let fallbackName: String
    let isCurrent: Bool
    let isPlaying: Bool
    let failure: String?

    private var problem: String? { failure ?? entry?.problem }

    var body: some View {
        HStack(spacing: 12) {
            thumbnail
            VStack(alignment: .leading, spacing: 2) {
                Text(entry?.title ?? fallbackName)
                    .font(.system(.body, design: .rounded, weight: isCurrent ? .semibold : .regular))
                    .lineLimit(1)
                if let subtitle = entry?.subtitle, !subtitle.isEmpty {
                    Text(subtitle)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            Spacer(minLength: 8)
            trailing
        }
        .padding(.vertical, 4)
        .opacity(problem == nil ? 1 : 0.5)
        .help(problem ?? "")
    }

    private var thumbnail: some View {
        ZStack {
            if let image = entry?.thumbnail {
                Image(decorative: image, scale: 2)
                    .resizable()
                    .scaledToFill()
            } else {
                Color.primary.opacity(0.08)
                    .overlay {
                        Image(systemName: "music.note")
                            .foregroundStyle(.secondary)
                    }
            }
            if isCurrent {
                Color.black.opacity(0.45)
                Image(systemName: isPlaying ? "waveform" : "speaker.fill")
                    .font(.system(size: 16, weight: .bold))
                    .foregroundStyle(Color.hogAccent)
            }
        }
        .frame(width: 44, height: 44)
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }

    @ViewBuilder
    private var trailing: some View {
        if problem != nil {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.orange)
        } else if let quality = entry?.quality {
            Text(quality)
                .font(.caption2.monospacedDigit().weight(.semibold))
                .foregroundStyle(.secondary)
                .padding(.horizontal, 8)
                .padding(.vertical, 2)
                .background(Color.primary.opacity(0.08), in: Capsule())
                .help(entry?.qualityDetail ?? "")
        }
        if let duration = entry?.duration {
            Text(duration)
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
                .frame(minWidth: 36, alignment: .trailing)
        }
    }
}
