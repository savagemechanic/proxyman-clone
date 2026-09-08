import SwiftUI
import ProxymanCloneKit

@main
struct ProxymanCloneApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                .frame(minWidth: 1_080, minHeight: 680)
        }
        .windowStyle(.titleBar)
    }
}

struct ContentView: View {
    @State private var statusText = "Engine disconnected"
    @State private var tlsText = "TLS interception unknown"
    @State private var transactions: [CapturedTransaction] = []
    @State private var selectedID: UInt64?
    @State private var isChecking = false

    private var selectedTransaction: CapturedTransaction? {
        transactions.first { $0.id == selectedID }
    }

    var body: some View {
        NavigationSplitView {
            List {
                Label("All Traffic", systemImage: "arrow.left.arrow.right")
                Label("Pinned", systemImage: "pin")
            }
            .navigationTitle("Proxyman Clone")
        } content: {
            trafficList
                .navigationTitle("Traffic")
        } detail: {
            inspector
                .navigationTitle("Inspector")
        }
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Text(statusText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Button(action: refresh) {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
                .disabled(isChecking)
            }
        }
        .task {
            while !Task.isCancelled {
                refresh()
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }

    @ViewBuilder
    private var trafficList: some View {
        if transactions.isEmpty {
            ContentUnavailableView(
                "No captured traffic",
                systemImage: "network",
                description: Text("Run the proxy engine, configure a client to use it, then make a request.")
            )
        } else {
            List(transactions, selection: $selectedID) { transaction in
                HStack(spacing: 10) {
                    Text(transaction.method)
                        .font(.system(.caption, design: .monospaced).weight(.semibold))
                        .frame(width: 52, alignment: .leading)
                    Text(transaction.host)
                        .lineLimit(1)
                    Text(transaction.target)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    Spacer()
                    if let status = transaction.statusCode {
                        Text(String(status))
                            .font(.system(.caption, design: .monospaced))
                    } else {
                        ProgressView()
                            .controlSize(.small)
                    }
                    Image(systemName: transaction.scheme == "https" ? "lock.fill" : "network")
                        .foregroundStyle(.secondary)
                }
                .tag(transaction.id)
            }
        }
    }

    @ViewBuilder
    private var inspector: some View {
        if let transaction = selectedTransaction {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    HStack {
                        Text(transaction.method)
                            .font(.system(.headline, design: .monospaced))
                        Text("\(transaction.scheme)://\(transaction.host)\(transaction.target)")
                            .textSelection(.enabled)
                    }

                    Divider()
                    metadataSection(transaction)
                    headerSection("Request Headers", headers: transaction.requestHeaders)
                    headerSection("Response Headers", headers: transaction.responseHeaders)
                }
                .padding(20)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        } else {
            VStack(spacing: 12) {
                Image(systemName: "waveform.path.ecg.rectangle")
                    .font(.system(size: 44))
                Text("Select a transaction")
                    .font(.title2.weight(.semibold))
                Label(tlsText, systemImage: "lock.shield")
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func metadataSection(_ transaction: CapturedTransaction) -> some View {
        Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 8) {
            GridRow { Text("Status").foregroundStyle(.secondary); Text(transaction.statusCode.map(String.init) ?? transaction.state) }
            GridRow { Text("Host").foregroundStyle(.secondary); Text(transaction.host).textSelection(.enabled) }
            GridRow { Text("Scheme").foregroundStyle(.secondary); Text(transaction.scheme.uppercased()) }
            GridRow { Text("Request body").foregroundStyle(.secondary); Text("\(transaction.requestBodyBytes) bytes") }
            GridRow { Text("Response body").foregroundStyle(.secondary); Text("\(transaction.responseBodyBytes) bytes") }
            GridRow { Text("Transaction ID").foregroundStyle(.secondary); Text(String(transaction.id)).textSelection(.enabled) }
        }
    }

    private func headerSection(_ title: String, headers: [HeaderField]) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .font(.headline)
            if headers.isEmpty {
                Text("None")
                    .foregroundStyle(.secondary)
            } else {
                ForEach(Array(headers.enumerated()), id: \.offset) { _, header in
                    HStack(alignment: .top) {
                        Text(header.name)
                            .font(.system(.body, design: .monospaced).weight(.medium))
                            .frame(width: 180, alignment: .leading)
                        Text(header.value)
                            .font(.system(.body, design: .monospaced))
                            .textSelection(.enabled)
                    }
                }
            }
        }
    }

    private func refresh() {
        guard !isChecking else { return }
        isChecking = true
        Task {
            defer { isChecking = false }
            do {
                let client = EngineClient()
                async let statusTask = client.fetchStatus()
                async let transactionsTask = client.fetchTransactions()
                let (status, captured) = try await (statusTask, transactionsTask)
                statusText = "Engine \(status.proxyState) · \(captured.count) shown"
                tlsText = status.tlsInterceptionEnabled ? "TLS interception enabled" : "TLS interception disabled"
                transactions = captured
                if let selectedID, !captured.contains(where: { $0.id == selectedID }) {
                    self.selectedID = nil
                }
            } catch {
                statusText = "Engine unavailable"
                tlsText = "TLS interception unknown"
            }
        }
    }
}
