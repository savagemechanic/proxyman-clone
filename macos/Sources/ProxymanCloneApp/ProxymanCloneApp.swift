import Foundation
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

private enum TrafficFilter: String, CaseIterable, Identifiable {
    case all = "All"
    case http = "HTTP"
    case https = "HTTPS"
    case success = "2xx"
    case clientError = "4xx"
    case serverError = "5xx"

    var id: Self { self }
}

struct ContentView: View {
    @State private var statusText = "Engine disconnected"
    @State private var tlsText = "TLS interception unknown"
    @State private var transactions: [CapturedTransaction] = []
    @State private var selectedID: UInt64?
    @State private var isChecking = false
    @State private var searchText = ""
    @State private var trafficFilter: TrafficFilter = .all

    private var filteredTransactions: [CapturedTransaction] {
        transactions.filter { transaction in
            matchesFilter(transaction) && matchesSearch(transaction)
        }
    }

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
            VStack(spacing: 0) {
                filterBar
                Divider()
                trafficList
            }
            .navigationTitle("Traffic")
            .searchable(text: $searchText, prompt: "Method, host, path, status")
        } detail: {
            inspector
                .navigationTitle("Inspector")
        }
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Text(statusText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Button {
                    statusText = HARExportPanel.export(transactions: transactions)
                } label: {
                    Label("Export HAR", systemImage: "square.and.arrow.up")
                }
                .disabled(transactions.isEmpty)
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

    private var filterBar: some View {
        Picker("Traffic filter", selection: $trafficFilter) {
            ForEach(TrafficFilter.allCases) { filter in
                Text(filter.rawValue).tag(filter)
            }
        }
        .pickerStyle(.segmented)
        .padding(10)
    }

    @ViewBuilder
    private var trafficList: some View {
        if transactions.isEmpty {
            ContentUnavailableView(
                "No captured traffic",
                systemImage: "network",
                description: Text("Run the proxy engine, configure a client to use it, then make a request.")
            )
        } else if filteredTransactions.isEmpty {
            ContentUnavailableView.search(text: searchText.isEmpty ? trafficFilter.rawValue : searchText)
        } else {
            List(filteredTransactions, selection: $selectedID) { transaction in
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
                    bodySection("Request Body", preview: transaction.requestBodyPreview)
                    headerSection("Request Headers", headers: transaction.requestHeaders)
                    bodySection("Response Body", preview: transaction.responseBodyPreview)
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
            GridRow {
                Text("Status").foregroundStyle(.secondary)
                Text(transaction.statusCode.map(String.init) ?? transaction.state)
            }
            GridRow {
                Text("Host").foregroundStyle(.secondary)
                Text(transaction.host).textSelection(.enabled)
            }
            GridRow {
                Text("Scheme").foregroundStyle(.secondary)
                Text(transaction.scheme.uppercased())
            }
            GridRow {
                Text("Request body").foregroundStyle(.secondary)
                Text("\(transaction.requestBodyBytes) bytes")
            }
            GridRow {
                Text("Response body").foregroundStyle(.secondary)
                Text("\(transaction.responseBodyBytes) bytes")
            }
            GridRow {
                Text("Transaction ID").foregroundStyle(.secondary)
                Text(String(transaction.id)).textSelection(.enabled)
            }
        }
    }

    private func bodySection(_ title: String, preview: BodyPreview?) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(title)
                    .font(.headline)
                Spacer()
                if let preview {
                    Text(bodySummary(preview))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            if let preview, let text = displayBody(preview) {
                ScrollView(.horizontal) {
                    Text(text)
                        .font(.system(.body, design: .monospaced))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(10)
                }
                .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 8))
            } else if let preview, preview.totalBytes > 0 {
                Text("Binary or non-UTF-8 body preview · \(preview.contentType ?? "unknown content type")")
                    .foregroundStyle(.secondary)
            } else {
                Text("Empty")
                    .foregroundStyle(.secondary)
            }
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

    private func bodySummary(_ preview: BodyPreview) -> String {
        let type = preview.contentType?.split(separator: ";").first.map(String.init) ?? "unknown"
        if preview.truncated {
            return "\(type) · \(preview.capturedBytes)/\(preview.totalBytes) bytes shown"
        }
        return "\(type) · \(preview.totalBytes) bytes"
    }

    private func displayBody(_ preview: BodyPreview) -> String? {
        guard let text = preview.text else { return nil }
        guard preview.contentType?.localizedCaseInsensitiveContains("json") == true,
              let data = text.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data),
              JSONSerialization.isValidJSONObject(object),
              let prettyData = try? JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys]),
              let pretty = String(data: prettyData, encoding: .utf8)
        else {
            return text
        }
        return pretty
    }

    private func matchesSearch(_ transaction: CapturedTransaction) -> Bool {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return true }
        let status = transaction.statusCode.map(String.init) ?? transaction.state
        return transaction.method.localizedCaseInsensitiveContains(query)
            || transaction.host.localizedCaseInsensitiveContains(query)
            || transaction.target.localizedCaseInsensitiveContains(query)
            || status.localizedCaseInsensitiveContains(query)
    }

    private func matchesFilter(_ transaction: CapturedTransaction) -> Bool {
        switch trafficFilter {
        case .all:
            true
        case .http:
            transaction.scheme == "http"
        case .https:
            transaction.scheme == "https"
        case .success:
            transaction.statusCode.map { (200..<300).contains(Int($0)) } ?? false
        case .clientError:
            transaction.statusCode.map { (400..<500).contains(Int($0)) } ?? false
        case .serverError:
            transaction.statusCode.map { (500..<600).contains(Int($0)) } ?? false
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
