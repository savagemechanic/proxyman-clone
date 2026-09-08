import Foundation
import Network

public struct EngineStatus: Codable, Equatable, Sendable {
    public let protocolVersion: UInt16
    public let proxyState: String
    public let listenAddress: String?
    public let capturedTransactions: UInt64
    public let tlsInterceptionEnabled: Bool

    enum CodingKeys: String, CodingKey {
        case protocolVersion = "protocol_version"
        case proxyState = "proxy_state"
        case listenAddress = "listen_address"
        case capturedTransactions = "captured_transactions"
        case tlsInterceptionEnabled = "tls_interception_enabled"
    }
}

public struct HeaderField: Codable, Equatable, Sendable {
    public let name: String
    public let value: String
}

public struct BodyPreview: Codable, Equatable, Sendable {
    public let contentType: String?
    public let text: String?
    public let capturedBytes: UInt64
    public let totalBytes: UInt64
    public let truncated: Bool

    enum CodingKeys: String, CodingKey {
        case contentType = "content_type"
        case text
        case capturedBytes = "captured_bytes"
        case totalBytes = "total_bytes"
        case truncated
    }
}

public struct CapturedTransaction: Codable, Equatable, Identifiable, Sendable {
    public let id: UInt64
    public let startedAtUnixMs: UInt64
    public let scheme: String
    public let host: String
    public let method: String
    public let target: String
    public let requestHeaders: [HeaderField]
    public let requestBodyBytes: UInt64
    public let requestBodyPreview: BodyPreview?
    public let statusCode: UInt16?
    public let responseHeaders: [HeaderField]
    public let responseBodyBytes: UInt64
    public let responseBodyPreview: BodyPreview?
    public let state: String

    enum CodingKeys: String, CodingKey {
        case id
        case startedAtUnixMs = "started_at_unix_ms"
        case scheme
        case host
        case method
        case target
        case requestHeaders = "request_headers"
        case requestBodyBytes = "request_body_bytes"
        case requestBodyPreview = "request_body_preview"
        case statusCode = "status_code"
        case responseHeaders = "response_headers"
        case responseBodyBytes = "response_body_bytes"
        case responseBodyPreview = "response_body_preview"
        case state
    }
}

public struct RewriteRule: Codable, Equatable, Identifiable, Sendable {
    public var id: String
    public var enabled: Bool
    public var hostContains: String?
    public var pathPrefix: String?
    public var actions: [RewriteAction]

    public init(
        id: String,
        enabled: Bool = true,
        hostContains: String? = nil,
        pathPrefix: String? = nil,
        actions: [RewriteAction] = []
    ) {
        self.id = id
        self.enabled = enabled
        self.hostContains = hostContains
        self.pathPrefix = pathPrefix
        self.actions = actions
    }

    enum CodingKeys: String, CodingKey {
        case id
        case enabled
        case hostContains = "host_contains"
        case pathPrefix = "path_prefix"
        case actions
    }
}

public enum RewriteAction: Codable, Equatable, Sendable {
    case setPath(String)
    case setHeader(name: String, value: String)
    case removeHeader(String)

    private enum CodingKeys: String, CodingKey {
        case type
        case value
        case name
    }

    private enum Kind: String, Codable {
        case setPath = "set_path"
        case setHeader = "set_header"
        case removeHeader = "remove_header"
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(Kind.self, forKey: .type) {
        case .setPath:
            self = .setPath(try container.decode(String.self, forKey: .value))
        case .setHeader:
            self = .setHeader(
                name: try container.decode(String.self, forKey: .name),
                value: try container.decode(String.self, forKey: .value)
            )
        case .removeHeader:
            self = .removeHeader(try container.decode(String.self, forKey: .name))
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .setPath(let value):
            try container.encode(Kind.setPath, forKey: .type)
            try container.encode(value, forKey: .value)
        case .setHeader(let name, let value):
            try container.encode(Kind.setHeader, forKey: .type)
            try container.encode(name, forKey: .name)
            try container.encode(value, forKey: .value)
        case .removeHeader(let name):
            try container.encode(Kind.removeHeader, forKey: .type)
            try container.encode(name, forKey: .name)
        }
    }
}

public enum EngineClientError: Error, Sendable {
    case invalidResponse
    case engineError(String)
}

public actor EngineClient {
    private let host: NWEndpoint.Host
    private let port: NWEndpoint.Port

    public init(host: String = "127.0.0.1", port: UInt16 = 9099) {
        self.host = NWEndpoint.Host(host)
        self.port = NWEndpoint.Port(rawValue: port)!
    }

    public func fetchStatus() async throws -> EngineStatus {
        let data = try await send(jsonLine: #"{"type":"get_status"}"#)
        let envelope = try JSONDecoder().decode(EngineEnvelope.self, from: data)
        guard envelope.type == "status", let status = envelope.status else {
            throw envelope.errorOrInvalidResponse
        }
        return status
    }

    public func fetchTransactions(limit: Int = 250) async throws -> [CapturedTransaction] {
        let safeLimit = max(1, min(limit, 1_000))
        let data = try await send(jsonLine: #"{"type":"list_transactions","limit":\#(safeLimit)}"#)
        let envelope = try JSONDecoder().decode(EngineEnvelope.self, from: data)
        guard envelope.type == "transactions", let transactions = envelope.transactions else {
            throw envelope.errorOrInvalidResponse
        }
        return transactions
    }

    public func fetchRewriteRules() async throws -> [RewriteRule] {
        let data = try await send(jsonLine: #"{"type":"list_rewrite_rules"}"#)
        let envelope = try JSONDecoder().decode(EngineEnvelope.self, from: data)
        guard envelope.type == "rewrite_rules", let rules = envelope.rules else {
            throw envelope.errorOrInvalidResponse
        }
        return rules
    }

    @discardableResult
    public func replaceRewriteRules(_ rules: [RewriteRule]) async throws -> [RewriteRule] {
        let command = ReplaceRewriteRulesCommand(rules: rules)
        let encoded = try JSONEncoder().encode(command)
        guard let line = String(data: encoded, encoding: .utf8) else {
            throw EngineClientError.invalidResponse
        }
        let data = try await send(jsonLine: line)
        let envelope = try JSONDecoder().decode(EngineEnvelope.self, from: data)
        guard envelope.type == "rewrite_rules", let rules = envelope.rules else {
            throw envelope.errorOrInvalidResponse
        }
        return rules
    }

    private func send(jsonLine: String) async throws -> Data {
        let connection = NWConnection(host: host, port: port, using: .tcp)

        return try await withCheckedThrowingContinuation { continuation in
            let completion = ContinuationBox(continuation)

            connection.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    connection.send(content: Data((jsonLine + "\n").utf8), completion: .contentProcessed { error in
                        if let error {
                            completion.finish(.failure(error), connection: connection)
                            return
                        }

                        connection.receive(minimumIncompleteLength: 1, maximumLength: 8_388_608) { data, _, _, error in
                            if let error {
                                completion.finish(.failure(error), connection: connection)
                            } else if let data {
                                let firstLine: Data
                                if let newline = data.firstIndex(of: UInt8(ascii: "\n")) {
                                    firstLine = data.subdata(in: data.startIndex..<newline)
                                } else {
                                    firstLine = data
                                }
                                completion.finish(.success(firstLine), connection: connection)
                            } else {
                                completion.finish(.failure(EngineClientError.invalidResponse), connection: connection)
                            }
                        }
                    })
                case .failed(let error):
                    completion.finish(.failure(error), connection: connection)
                default:
                    break
                }
            }

            connection.start(queue: .global(qos: .userInitiated))
        }
    }
}

private struct ReplaceRewriteRulesCommand: Encodable {
    let type = "replace_rewrite_rules"
    let rules: [RewriteRule]
}

private final class ContinuationBox: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Data, Error>?

    init(_ continuation: CheckedContinuation<Data, Error>) {
        self.continuation = continuation
    }

    func finish(_ result: Result<Data, Error>, connection: NWConnection) {
        lock.lock()
        guard let continuation else {
            lock.unlock()
            return
        }
        self.continuation = nil
        lock.unlock()

        connection.cancel()
        continuation.resume(with: result)
    }
}

private struct EngineEnvelope: Codable {
    let type: String
    let status: EngineStatus?
    let transactions: [CapturedTransaction]?
    let rules: [RewriteRule]?
    let message: String?

    var errorOrInvalidResponse: EngineClientError {
        if type == "error" {
            return .engineError(message ?? "Unknown engine error")
        }
        return .invalidResponse
    }
}
