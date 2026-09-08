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

public struct CapturedTransaction: Codable, Equatable, Identifiable, Sendable {
    public let id: UInt64
    public let startedAtUnixMs: UInt64
    public let scheme: String
    public let host: String
    public let method: String
    public let target: String
    public let requestHeaders: [HeaderField]
    public let requestBodyBytes: UInt64
    public let statusCode: UInt16?
    public let responseHeaders: [HeaderField]
    public let responseBodyBytes: UInt64
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
        case statusCode = "status_code"
        case responseHeaders = "response_headers"
        case responseBodyBytes = "response_body_bytes"
        case state
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

                        connection.receive(minimumIncompleteLength: 1, maximumLength: 4_194_304) { data, _, _, error in
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
    let message: String?

    var errorOrInvalidResponse: EngineClientError {
        if type == "error" {
            return .engineError(message ?? "Unknown engine error")
        }
        return .invalidResponse
    }
}
