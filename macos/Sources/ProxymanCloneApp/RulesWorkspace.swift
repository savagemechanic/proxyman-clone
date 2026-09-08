import SwiftUI
import ProxymanCloneKit

struct RulesWorkspace: View {
    @State private var rules: [RewriteRule] = []
    @State private var selectedRuleID: String?
    @State private var statusText = "Rules not loaded"
    @State private var isLoading = false
    @State private var isSaving = false

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Text("Rewrite Rules")
                    .font(.title2.weight(.semibold))
                Spacer()
                Text(statusText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Button("Reload", systemImage: "arrow.clockwise") {
                    loadRules()
                }
                .disabled(isLoading || isSaving)
                Button("Save", systemImage: "square.and.arrow.down") {
                    saveRules()
                }
                .buttonStyle(.borderedProminent)
                .disabled(isLoading || isSaving)
            }
            .padding(14)

            Divider()

            HSplitView {
                ruleList
                    .frame(minWidth: 260, idealWidth: 300)
                ruleEditor
                    .frame(minWidth: 440, maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .task {
            if rules.isEmpty {
                loadRules()
            }
        }
    }

    private var ruleList: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Rules")
                    .font(.headline)
                Spacer()
                Button(action: addRule) {
                    Image(systemName: "plus")
                }
                .buttonStyle(.plain)
                .help("Add rule")
            }
            .padding(12)

            Divider()

            if rules.isEmpty {
                ContentUnavailableView(
                    "No rewrite rules",
                    systemImage: "arrow.triangle.2.circlepath",
                    description: Text("Add a rule to change requests or responses before delivery.")
                )
            } else {
                List(selection: $selectedRuleID) {
                    ForEach(rules) { rule in
                        HStack(spacing: 8) {
                            Image(systemName: rule.enabled ? "checkmark.circle.fill" : "circle")
                                .foregroundStyle(rule.enabled ? .primary : .secondary)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(rule.id)
                                    .lineLimit(1)
                                Text(ruleSummary(rule))
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                        }
                        .tag(rule.id)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var ruleEditor: some View {
        if let rule = selectedRuleBinding {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    HStack {
                        Text("Rule")
                            .font(.title2.weight(.semibold))
                        Spacer()
                        Button(action: { moveSelectedRule(by: -1) }) {
                            Image(systemName: "arrow.up")
                        }
                        .disabled(!canMoveSelectedRule(by: -1))
                        .help("Move rule earlier")
                        Button(action: { moveSelectedRule(by: 1) }) {
                            Image(systemName: "arrow.down")
                        }
                        .disabled(!canMoveSelectedRule(by: 1))
                        .help("Move rule later")
                        Button(role: .destructive, action: deleteSelectedRule) {
                            Label("Delete", systemImage: "trash")
                        }
                    }

                    GroupBox("Identity") {
                        VStack(alignment: .leading, spacing: 10) {
                            TextField("Rule ID", text: rule.id)
                            Toggle("Enabled", isOn: rule.enabled)
                        }
                        .padding(6)
                    }

                    GroupBox("Match") {
                        VStack(alignment: .leading, spacing: 10) {
                            TextField("Host contains (optional)", text: optionalText(rule.hostContains))
                            TextField("Path prefix (optional)", text: optionalText(rule.pathPrefix))
                            Text("Both matchers are ANDed. Empty matchers make the rule apply to every proxied request and its response.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        .padding(6)
                    }

                    GroupBox("Request Actions") {
                        VStack(alignment: .leading, spacing: 10) {
                            if rule.wrappedValue.actions.isEmpty {
                                Text("No request actions")
                                    .foregroundStyle(.secondary)
                            }

                            ForEach(Array(rule.wrappedValue.actions.indices), id: \.self) { index in
                                actionEditor(rule: rule, index: index)
                                if index < rule.wrappedValue.actions.count - 1 {
                                    Divider()
                                }
                            }

                            Menu("Add request action", systemImage: "plus") {
                                Button("Set Path") {
                                    appendAction(.setPath("/"), to: rule)
                                }
                                Button("Set Header") {
                                    appendAction(.setHeader(name: "X-Debug", value: "1"), to: rule)
                                }
                                Button("Remove Header") {
                                    appendAction(.removeHeader("Authorization"), to: rule)
                                }
                            }
                        }
                        .padding(6)
                    }

                    GroupBox("Response Actions") {
                        VStack(alignment: .leading, spacing: 10) {
                            if rule.wrappedValue.responseActions.isEmpty {
                                Text("No response actions")
                                    .foregroundStyle(.secondary)
                            }

                            ForEach(Array(rule.wrappedValue.responseActions.indices), id: \.self) { index in
                                responseActionEditor(rule: rule, index: index)
                                if index < rule.wrappedValue.responseActions.count - 1 {
                                    Divider()
                                }
                            }

                            Menu("Add response action", systemImage: "plus") {
                                Button("Set Status") {
                                    appendResponseAction(.setStatus(503), to: rule)
                                }
                                Button("Set Header") {
                                    appendResponseAction(.setHeader(name: "X-Debug-Response", value: "1"), to: rule)
                                }
                                Button("Remove Header") {
                                    appendResponseAction(.removeHeader("Server"), to: rule)
                                }
                            }

                            Text("Status rewrites preserve the upstream body. The engine rejects 1xx, 204, 205, and 304 status overrides. Framing headers such as Content-Length and Transfer-Encoding cannot be rewritten here.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        .padding(6)
                    }

                    Text("Rules run from top to bottom. Later request rules see request changes made by earlier rules; response actions use the same matched rule order.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .padding(18)
            }
        } else {
            ContentUnavailableView(
                "Select a rule",
                systemImage: "slider.horizontal.3",
                description: Text("Choose a rule on the left or create a new one.")
            )
        }
    }

    @ViewBuilder
    private func actionEditor(rule: Binding<RewriteRule>, index: Int) -> some View {
        let action = actionBinding(rule: rule, index: index)
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Picker("Action", selection: actionKindBinding(action)) {
                    ForEach(ActionKind.allCases) { kind in
                        Text(kind.title).tag(kind)
                    }
                }
                .frame(maxWidth: 220)
                Spacer()
                Button(action: { moveAction(in: rule, index: index, by: -1) }) {
                    Image(systemName: "arrow.up")
                }
                .disabled(index == 0)
                Button(action: { moveAction(in: rule, index: index, by: 1) }) {
                    Image(systemName: "arrow.down")
                }
                .disabled(index >= rule.wrappedValue.actions.count - 1)
                Button(role: .destructive, action: { removeAction(from: rule, index: index) }) {
                    Image(systemName: "trash")
                }
            }

            switch action.wrappedValue {
            case .setPath:
                TextField("New path", text: setPathValue(action))
            case .setHeader:
                HStack {
                    TextField("Header name", text: setHeaderName(action))
                    TextField("Header value", text: setHeaderValue(action))
                }
            case .removeHeader:
                TextField("Header name", text: removeHeaderName(action))
            }
        }
    }

    @ViewBuilder
    private func responseActionEditor(rule: Binding<RewriteRule>, index: Int) -> some View {
        let action = responseActionBinding(rule: rule, index: index)
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Picker("Action", selection: responseActionKindBinding(action)) {
                    ForEach(ResponseActionKind.allCases) { kind in
                        Text(kind.title).tag(kind)
                    }
                }
                .frame(maxWidth: 220)
                Spacer()
                Button(action: { moveResponseAction(in: rule, index: index, by: -1) }) {
                    Image(systemName: "arrow.up")
                }
                .disabled(index == 0)
                Button(action: { moveResponseAction(in: rule, index: index, by: 1) }) {
                    Image(systemName: "arrow.down")
                }
                .disabled(index >= rule.wrappedValue.responseActions.count - 1)
                Button(role: .destructive, action: { removeResponseAction(from: rule, index: index) }) {
                    Image(systemName: "trash")
                }
            }

            switch action.wrappedValue {
            case .setStatus:
                TextField("Status code", text: responseStatusText(action))
                    .frame(maxWidth: 180)
            case .setHeader:
                HStack {
                    TextField("Header name", text: responseHeaderName(action))
                    TextField("Header value", text: responseHeaderValue(action))
                }
            case .removeHeader:
                TextField("Header name", text: responseRemoveHeaderName(action))
            }
        }
    }

    private var selectedRuleBinding: Binding<RewriteRule>? {
        guard let selectedRuleID,
              let index = rules.firstIndex(where: { $0.id == selectedRuleID })
        else {
            return nil
        }
        return Binding(
            get: { rules[index] },
            set: { newValue in
                let previousID = rules[index].id
                rules[index] = newValue
                if selectedRuleID == previousID {
                    self.selectedRuleID = newValue.id
                }
            }
        )
    }

    private func optionalText(_ binding: Binding<String?>) -> Binding<String> {
        Binding(
            get: { binding.wrappedValue ?? "" },
            set: { value in
                let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
                binding.wrappedValue = trimmed.isEmpty ? nil : value
            }
        )
    }

    private func actionBinding(rule: Binding<RewriteRule>, index: Int) -> Binding<RewriteAction> {
        Binding(
            get: { rule.wrappedValue.actions[index] },
            set: { value in
                var copy = rule.wrappedValue
                copy.actions[index] = value
                rule.wrappedValue = copy
            }
        )
    }

    private func actionKindBinding(_ action: Binding<RewriteAction>) -> Binding<ActionKind> {
        Binding(
            get: { ActionKind(action.wrappedValue) },
            set: { kind in
                switch kind {
                case .setPath:
                    action.wrappedValue = .setPath("/")
                case .setHeader:
                    action.wrappedValue = .setHeader(name: "X-Debug", value: "1")
                case .removeHeader:
                    action.wrappedValue = .removeHeader("Authorization")
                }
            }
        )
    }

    private func setPathValue(_ action: Binding<RewriteAction>) -> Binding<String> {
        Binding(
            get: {
                if case .setPath(let value) = action.wrappedValue { return value }
                return ""
            },
            set: { action.wrappedValue = .setPath($0) }
        )
    }

    private func setHeaderName(_ action: Binding<RewriteAction>) -> Binding<String> {
        Binding(
            get: {
                if case .setHeader(let name, _) = action.wrappedValue { return name }
                return ""
            },
            set: { name in
                if case .setHeader(_, let value) = action.wrappedValue {
                    action.wrappedValue = .setHeader(name: name, value: value)
                }
            }
        )
    }

    private func setHeaderValue(_ action: Binding<RewriteAction>) -> Binding<String> {
        Binding(
            get: {
                if case .setHeader(_, let value) = action.wrappedValue { return value }
                return ""
            },
            set: { value in
                if case .setHeader(let name, _) = action.wrappedValue {
                    action.wrappedValue = .setHeader(name: name, value: value)
                }
            }
        )
    }

    private func removeHeaderName(_ action: Binding<RewriteAction>) -> Binding<String> {
        Binding(
            get: {
                if case .removeHeader(let name) = action.wrappedValue { return name }
                return ""
            },
            set: { action.wrappedValue = .removeHeader($0) }
        )
    }

    private func responseActionBinding(rule: Binding<RewriteRule>, index: Int) -> Binding<ResponseRewriteAction> {
        Binding(
            get: { rule.wrappedValue.responseActions[index] },
            set: { value in
                var copy = rule.wrappedValue
                copy.responseActions[index] = value
                rule.wrappedValue = copy
            }
        )
    }

    private func responseActionKindBinding(_ action: Binding<ResponseRewriteAction>) -> Binding<ResponseActionKind> {
        Binding(
            get: { ResponseActionKind(action.wrappedValue) },
            set: { kind in
                switch kind {
                case .setStatus:
                    action.wrappedValue = .setStatus(503)
                case .setHeader:
                    action.wrappedValue = .setHeader(name: "X-Debug-Response", value: "1")
                case .removeHeader:
                    action.wrappedValue = .removeHeader("Server")
                }
            }
        )
    }

    private func responseStatusText(_ action: Binding<ResponseRewriteAction>) -> Binding<String> {
        Binding(
            get: {
                if case .setStatus(let value) = action.wrappedValue { return String(value) }
                return ""
            },
            set: { text in
                guard let value = UInt16(text) else { return }
                action.wrappedValue = .setStatus(value)
            }
        )
    }

    private func responseHeaderName(_ action: Binding<ResponseRewriteAction>) -> Binding<String> {
        Binding(
            get: {
                if case .setHeader(let name, _) = action.wrappedValue { return name }
                return ""
            },
            set: { name in
                if case .setHeader(_, let value) = action.wrappedValue {
                    action.wrappedValue = .setHeader(name: name, value: value)
                }
            }
        )
    }

    private func responseHeaderValue(_ action: Binding<ResponseRewriteAction>) -> Binding<String> {
        Binding(
            get: {
                if case .setHeader(_, let value) = action.wrappedValue { return value }
                return ""
            },
            set: { value in
                if case .setHeader(let name, _) = action.wrappedValue {
                    action.wrappedValue = .setHeader(name: name, value: value)
                }
            }
        )
    }

    private func responseRemoveHeaderName(_ action: Binding<ResponseRewriteAction>) -> Binding<String> {
        Binding(
            get: {
                if case .removeHeader(let name) = action.wrappedValue { return name }
                return ""
            },
            set: { action.wrappedValue = .removeHeader($0) }
        )
    }

    private func appendAction(_ action: RewriteAction, to rule: Binding<RewriteRule>) {
        var copy = rule.wrappedValue
        copy.actions.append(action)
        rule.wrappedValue = copy
    }

    private func removeAction(from rule: Binding<RewriteRule>, index: Int) {
        var copy = rule.wrappedValue
        guard copy.actions.indices.contains(index) else { return }
        copy.actions.remove(at: index)
        rule.wrappedValue = copy
    }

    private func moveAction(in rule: Binding<RewriteRule>, index: Int, by offset: Int) {
        let destination = index + offset
        var copy = rule.wrappedValue
        guard copy.actions.indices.contains(index), copy.actions.indices.contains(destination) else { return }
        copy.actions.swapAt(index, destination)
        rule.wrappedValue = copy
    }

    private func appendResponseAction(_ action: ResponseRewriteAction, to rule: Binding<RewriteRule>) {
        var copy = rule.wrappedValue
        copy.responseActions.append(action)
        rule.wrappedValue = copy
    }

    private func removeResponseAction(from rule: Binding<RewriteRule>, index: Int) {
        var copy = rule.wrappedValue
        guard copy.responseActions.indices.contains(index) else { return }
        copy.responseActions.remove(at: index)
        rule.wrappedValue = copy
    }

    private func moveResponseAction(in rule: Binding<RewriteRule>, index: Int, by offset: Int) {
        let destination = index + offset
        var copy = rule.wrappedValue
        guard copy.responseActions.indices.contains(index), copy.responseActions.indices.contains(destination) else { return }
        copy.responseActions.swapAt(index, destination)
        rule.wrappedValue = copy
    }

    private func addRule() {
        var sequence = rules.count + 1
        var id = "rule-\(sequence)"
        let existing = Set(rules.map(\.id))
        while existing.contains(id) {
            sequence += 1
            id = "rule-\(sequence)"
        }
        rules.append(RewriteRule(id: id))
        selectedRuleID = id
    }

    private func deleteSelectedRule() {
        guard let selectedRuleID,
              let index = rules.firstIndex(where: { $0.id == selectedRuleID })
        else { return }
        rules.remove(at: index)
        self.selectedRuleID = rules.indices.contains(index) ? rules[index].id : rules.last?.id
    }

    private func canMoveSelectedRule(by offset: Int) -> Bool {
        guard let selectedRuleID,
              let index = rules.firstIndex(where: { $0.id == selectedRuleID })
        else { return false }
        return rules.indices.contains(index + offset)
    }

    private func moveSelectedRule(by offset: Int) {
        guard let selectedRuleID,
              let index = rules.firstIndex(where: { $0.id == selectedRuleID })
        else { return }
        let destination = index + offset
        guard rules.indices.contains(destination) else { return }
        rules.swapAt(index, destination)
    }

    private func ruleSummary(_ rule: RewriteRule) -> String {
        var parts: [String] = []
        if let host = rule.hostContains { parts.append("host: \(host)") }
        if let path = rule.pathPrefix { parts.append("path: \(path)") }
        if parts.isEmpty { parts.append("all requests") }
        parts.append("\(rule.actions.count) req")
        parts.append("\(rule.responseActions.count) res")
        return parts.joined(separator: " · ")
    }

    private func loadRules() {
        guard !isLoading else { return }
        isLoading = true
        statusText = "Loading…"
        Task {
            defer { isLoading = false }
            do {
                let loaded = try await EngineClient().fetchRewriteRules()
                rules = loaded
                if let selectedRuleID, loaded.contains(where: { $0.id == selectedRuleID }) {
                    self.selectedRuleID = selectedRuleID
                } else {
                    selectedRuleID = loaded.first?.id
                }
                statusText = "\(loaded.count) rule\(loaded.count == 1 ? "" : "s") loaded"
            } catch {
                statusText = "Load failed: \(error.localizedDescription)"
            }
        }
    }

    private func saveRules() {
        guard !isSaving else { return }
        isSaving = true
        statusText = "Saving…"
        let snapshot = rules
        Task {
            defer { isSaving = false }
            do {
                rules = try await EngineClient().replaceRewriteRules(snapshot)
                statusText = "Saved \(rules.count) rule\(rules.count == 1 ? "" : "s")"
            } catch {
                statusText = "Save failed: \(error.localizedDescription)"
            }
        }
    }
}

private enum ActionKind: String, CaseIterable, Identifiable {
    case setPath
    case setHeader
    case removeHeader

    var id: Self { self }

    var title: String {
        switch self {
        case .setPath: "Set Path"
        case .setHeader: "Set Header"
        case .removeHeader: "Remove Header"
        }
    }

    init(_ action: RewriteAction) {
        switch action {
        case .setPath: self = .setPath
        case .setHeader: self = .setHeader
        case .removeHeader: self = .removeHeader
        }
    }
}

private enum ResponseActionKind: String, CaseIterable, Identifiable {
    case setStatus
    case setHeader
    case removeHeader

    var id: Self { self }

    var title: String {
        switch self {
        case .setStatus: "Set Status"
        case .setHeader: "Set Header"
        case .removeHeader: "Remove Header"
        }
    }

    init(_ action: ResponseRewriteAction) {
        switch action {
        case .setStatus: self = .setStatus
        case .setHeader: self = .setHeader
        case .removeHeader: self = .removeHeader
        }
    }
}
