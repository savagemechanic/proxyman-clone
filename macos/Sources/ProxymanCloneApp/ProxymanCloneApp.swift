import SwiftUI

@main
struct ProxymanCloneApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                .frame(minWidth: 980, minHeight: 640)
        }
        .windowStyle(.titleBar)
    }
}

struct ContentView: View {
    @State private var statusText = "Engine disconnected"
    @State private var isChecking = false

    var body: some View {
        NavigationSplitView {
            List {
                Label("All Traffic", systemImage: "arrow.left.arrow.right")
                Label("Pinned", systemImage: "pin")
            }
            .navigationTitle("Proxyman Clone")
        } content: {
            ContentUnavailableView(
                "No captured traffic",
                systemImage: "network",
                description: Text("Start the Rust proxy engine and configure a client to use it.")
            )
            .navigationTitle("Traffic")
        } detail: {
            VStack(spacing: 16) {
                Image(systemName: "waveform.path.ecg.rectangle")
                    .font(.system(size: 44))
                Text("Inspector")
                    .font(.title2.weight(.semibold))
                Text(statusText)
                    .foregroundStyle(.secondary)
                Button(isChecking ? "Checking…" : "Check Engine") {
                    checkEngine()
                }
                .disabled(isChecking)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .navigationTitle("Details")
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button(action: checkEngine) {
                    Label("Engine Status", systemImage: "bolt.horizontal.circle")
                }
                .disabled(isChecking)
            }
        }
    }

    private func checkEngine() {
        isChecking = true
        Task {
            defer { isChecking = false }
            do {
                let status = try await EngineClient().fetchStatus()
                statusText = "Engine \(status.proxyState) · protocol v\(status.protocolVersion)"
            } catch {
                statusText = "Engine unavailable: \(error.localizedDescription)"
            }
        }
    }
}
