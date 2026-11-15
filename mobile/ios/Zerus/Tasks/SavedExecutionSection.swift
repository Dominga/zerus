import SwiftUI
import ZerusKit

struct SavedExecutionSection: View {
    let state: SavedExecutionSectionState

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Saved execution").font(.headline)
            ForEach(state.notices, id: \.self) { key in
                Text(LocalizedStringKey(key)).font(.footnote)
            }
            if !state.available {
                Text("Execution details unavailable.").font(.footnote)
            } else if !state.hasCurrent {
                Text("No current attempt recorded.").font(.footnote)
            } else if let phase = state.phase {
                Text(LocalizedStringKey(phase)).font(.subheadline)
            }
            if state.available {
                ForEach(state.rows, id: \.label) { row in
                    HStack(alignment: .top, spacing: 10) {
                        Text(LocalizedStringKey(row.label)).font(.caption).frame(maxWidth: .infinity, alignment: .leading)
                        Group {
                            if let value = row.value { Text(verbatim: value) }
                            else { Text("Not recorded") }
                        }.font(.footnote).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
            Text("Saved evidence only. Live execution state and executor queue are unavailable.")
                .font(.caption).foregroundStyle(Theme.muted)
        }
    }
}
