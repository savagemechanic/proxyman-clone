import AppKit
import Foundation
import ProxymanCloneKit

@MainActor
enum HARExportPanel {
    static func export(transactions: [CapturedTransaction]) -> String {
        guard !transactions.isEmpty else {
            return "Nothing to export"
        }

        let panel = NSSavePanel()
        panel.title = "Export HAR"
        panel.nameFieldStringValue = "proxyman-clone-session.har"
        panel.allowedFileTypes = ["har"]
        panel.isExtensionHidden = false
        panel.canCreateDirectories = true
        panel.message = "HAR files can contain authorization headers, cookies, request bodies, and other secrets. Store and share this export carefully."
        panel.prompt = "Export"

        guard panel.runModal() == .OK, let url = panel.url else {
            return "Export cancelled"
        }

        do {
            let data = try HARExporter.data(from: transactions)
            try data.write(to: url, options: .atomic)
            return "Exported \(transactions.count) transaction\(transactions.count == 1 ? "" : "s")"
        } catch {
            return "HAR export failed: \(error.localizedDescription)"
        }
    }
}
