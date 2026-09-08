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
        let envelope = try JSONDecoder().decode(EngineStatusEnvelope.self, from: data)
        guard envelope.type == "status", let status = envelope.status else {
            if envelope.type == "error" {
                throw EngineClientError.engineError(envelope.message ?? "Unknown engine error")
            }
            throw EngineClientError.invalidResponse
        }
        return status
    }

    private func send(jsonLine: String) async throws -> Data {
        let connection = NWConnection(host: host, port: port, using: .tcp)

        return try await withCheckedThrowingContinuation { continuation in
            var resumed = false
            func finish(_ result: Result<Data, Error>) {
                guard !resumed else { return }
                resumed = true
                connection.cancel()
                continuation.resume(with: result)
            }

            connection.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    connection.send(content: Data((jsonLine + "\n").utf8), completion: .contentProcessed { error in
                        if let error {
                            finish(.failure(error))
                            return
                        }

                        connection.receive(minimumIncompleteLength: 1, maximumLength: 1_048_576) { data, _, _, error in
                            if let error {
                                finish(.failure(error))
                            } else if let data {
                                let firstLine: Data
                                if let newline = data.firstIndex(of: UInt8(ascii: "\n")) {
                                    firstLine = data.subdata(in: data.startIndex..<newline)
                                } else {
                                    firstLine = data
                                }
                                finish(.success(firstLine))
                            } else {
                                finish(.failure(EngineClientError.invalidResponse))
                            }
                        }
                    })
                case .failed(let error):
                    finish(.failure(error))
                default:
                    break
                }
            }

            connection.start(queue: .global(qos: .userInitiated))
        }
    }
}

private struct EngineStatusEnvelope: Codable {
    let type: String
    let status: EngineStatus?
    let message: String?
}
